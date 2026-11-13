//! Taking a new track list and carrying what sounds across it by track id.

use super::super::Target;
use super::super::hold::{self, Clock};
use super::MidiPlayer;
use super::set::MidiSet;

impl MidiPlayer {
    /// Takes the newest track list if one arrived. With `at` (the clock and
    /// the position being played), notes are brought in line with it.
    /// Real-time safe.
    pub(super) fn take_new(&mut self, at: Option<(Clock<'_>, i64)>) {
        if let Some(old) = self.stash.take() {
            self.retire(old);
            if self.stash.is_some() {
                return;
            }
        }
        let Ok(mut new) = self.incoming.pop() else {
            return;
        };
        // Only the newest list matters.
        // Skipping a list needs room for it and for the one it replaces
        // in the end, or the stash would be overwritten and a list freed
        // here; newer lists then wait a block.
        while self.retired.slots() >= 2
            && let Ok(mut newer) = self.incoming.pop()
        {
            // A host the skipped list brought is the only handle to a
            // running process: it goes to the newer list's stand-in.
            new.pass_hosts_to(&mut newer);
            self.retire(std::mem::replace(&mut new, newer));
        }
        self.carry(&mut new, at);
        let old = std::mem::replace(&mut self.current, new);
        self.hosted = self.current.hosted();
        if self.incoming.is_empty() {
            self.hosted_pending
                .store(false, std::sync::atomic::Ordering::Relaxed);
        }
        self.retire(old);
    }

    /// Hands `old` back to be freed, keeping it if the queue is full. Only
    /// called while the stash is empty.
    fn retire(&mut self, old: Box<MidiSet>) {
        if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
            self.stash = Some(old);
        }
    }

    /// Moves what sounds from the old list into the new: trackers and
    /// voices carry by track id, notes that an edit removed or moved are
    /// released (`hold::sync`),
    /// and a track that left lets go of its notes.
    fn carry(&mut self, new: &mut MidiSet, at: Option<(Clock<'_>, i64)>) {
        let clock = self.clock;
        let old = &mut *self.current;
        // Tracks that went away let go of their hardware notes.
        for track in &mut old.tracks {
            if new
                .by_id
                .binary_search_by_key(&track.id, |&(id, _)| id)
                .is_err()
            {
                let channel = track.channel();
                track.state.release_all(channel);
                track.state.release_live();
                self.out.forward(track, clock);
            }
        }
        for track in new.tracks.iter_mut() {
            let found = old
                .by_id
                .binary_search_by_key(&track.id, |&(id, _)| id)
                .ok()
                .map(|k| old.by_id[k].1 as usize);
            let Some(j) = found else {
                track.dirty = true;
                // A track the old list did not have: the instrument it brought
                // takes its sound now, on the interface side of the swap.
                if let Some(sound) = track.sound.take()
                    && let Some(spec) = track.instrument.as_mut()
                {
                    spec.instrument.load_sound(sound);
                }
                continue;
            };
            let before = &mut old.tracks[j];
            let same_kind = matches!(
                (&track.instrument, &before.instrument),
                (Some(a), Some(b)) if a.kind == b.kind
            );
            let rerouted = track.target != before.target || !same_kind;
            // Live notes let go when the track stops listening or listens
            // to something else (here, or on the old output if rerouted).
            if rerouted || track.listen != before.listen {
                before.state.release_live();
            }
            if rerouted {
                // What sounded on the old output lets go there.
                let channel = before.channel();
                before.state.release_all(channel);
                match before.target {
                    Target::Port { .. } => self.out.forward(before, clock),
                    Target::Instrument => {
                        // The releases go to the old instrument; a plugin's
                        // only when it plays on (a host going away is not
                        // waited for).
                        if let Some(spec) = before
                            .instrument
                            .as_mut()
                            .filter(|spec| same_kind || !spec.instrument.hosted())
                        {
                            let events = std::mem::take(&mut before.state.events);
                            spec.instrument.render(0, &events, &mut [], &mut []);
                            before.state.events = events;
                        }
                        // An instrument of another kind (or none) takes its
                        // place: the old one plays its release out on the
                        // new list. A plugin's host goes with the old list.
                        if !same_kind
                            && before
                                .instrument
                                .as_ref()
                                .is_some_and(|spec| !spec.instrument.hosted())
                        {
                            track.leaving = before.instrument.take();
                        }
                    }
                }
                before.state.events.clear();
            }
            // One still ringing out from an earlier change rings on, unless a
            // newer one took its place (then it goes with the old list).
            if track.leaving.is_none() {
                track.leaving = before.leaving.take();
            }
            std::mem::swap(&mut track.state, &mut before.state);
            // A host the new list brings (not one carried over below) has
            // heard none of the notes held on the old instrument (a stand-in
            // while the plugin loaded).
            let host_arrives = same_kind
                && track.target == Target::Instrument
                && track
                    .instrument
                    .as_ref()
                    .is_some_and(|spec| spec.instrument.hosted());
            // The instrument the old list had plays on, except a host the
            // new list brings: that one is attached now, and the old list's
            // (a stand-in, or the host it replaces) goes with the old list.
            if same_kind
                && let (Some(a), Some(b)) = (track.instrument.as_mut(), before.instrument.as_mut())
                && !a.instrument.hosted()
            {
                std::mem::swap(&mut a.instrument, &mut b.instrument);
                if a.preset != b.preset {
                    a.instrument.configure(a.preset);
                }
            }
            track.dirty = !track.same_as(before);
            // A new sound for the instrument that plays on (M8 slice B5): it
            // is handed over here, and the one it stopped using goes back with
            // the old list, which the interface frees. Nothing is dropped on
            // this thread.
            if let Some(sound) = track.sound.take()
                && let Some(spec) = track.instrument.as_mut()
            {
                before.sound = spec.instrument.load_sound(sound).or(before.sound.take());
            }
            // Its held notes are forgotten, so the sync below strikes them
            // on the new host.
            if host_arrives && !track.state.held.is_empty() {
                track.state.held.clear();
                track.dirty = true;
            }
        }
        // Notes in line with the new data, at the position being played.
        if let Some((clock, pos)) = at {
            for track in &mut new.tracks {
                if track.dirty && track.sounds() {
                    let channel = track.channel();
                    hold::sync(
                        (&track.clips, &track.index),
                        &mut track.state,
                        channel,
                        clock,
                        pos,
                        track.kit.as_deref(),
                    );
                }
                track.dirty = false;
            }
        } else {
            for track in &mut new.tracks {
                track.dirty = false;
            }
        }
    }
}
