use super::super::Target;
use super::super::hold::{self, Clock};
use super::MidiPlayer;
use super::set::MidiSet;

impl MidiPlayer {
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

        while self.retired.slots() >= 2
            && let Ok(mut newer) = self.incoming.pop()
        {
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

    fn retire(&mut self, old: Box<MidiSet>) {
        if let Err(rtrb::PushError::Full(old)) = self.retired.push(old) {
            self.stash = Some(old);
        }
    }

    fn carry(&mut self, new: &mut MidiSet, at: Option<(Clock<'_>, i64)>) {
        let clock = self.clock;
        let old = &mut *self.current;

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

            if rerouted || track.listen != before.listen {
                before.state.release_live();
            }
            if rerouted {
                let channel = before.channel();
                before.state.release_all(channel);
                match before.target {
                    Target::Port { .. } => self.out.forward(before, clock),
                    Target::Instrument => {
                        if let Some(spec) = before
                            .instrument
                            .as_mut()
                            .filter(|spec| same_kind || !spec.instrument.hosted())
                        {
                            let events = std::mem::take(&mut before.state.events);
                            spec.instrument.render(0, &events, &mut [], &mut []);
                            before.state.events = events;
                        }

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

            if track.leaving.is_none() {
                track.leaving = before.leaving.take();
            }
            std::mem::swap(&mut track.state, &mut before.state);

            let host_arrives = same_kind
                && track.target == Target::Instrument
                && track
                    .instrument
                    .as_ref()
                    .is_some_and(|spec| spec.instrument.hosted());

            if same_kind
                && let (Some(a), Some(b)) = (track.instrument.as_mut(), before.instrument.as_mut())
                && !a.instrument.hosted()
            {
                std::mem::swap(&mut a.instrument, &mut b.instrument);

                a.instrument.configure_values(&a.values);
                if a.preset != b.preset {
                    a.instrument.configure(a.preset);
                }
            }
            track.dirty = !track.same_as(before);

            if let Some(sound) = track.sound.take()
                && let Some(spec) = track.instrument.as_mut()
            {
                before.sound = spec.instrument.load_sound(sound).or(before.sound.take());
            }

            if host_arrives && !track.state.held.is_empty() {
                track.state.held.clear();
                track.dirty = true;
            }
        }

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
