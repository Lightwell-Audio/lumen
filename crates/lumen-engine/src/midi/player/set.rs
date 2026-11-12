//! A track list, sorted by id for carrying state into the next.

use super::super::MidiTrack;

pub(in crate::midi) struct MidiSet {
    pub(super) tracks: Vec<MidiTrack>,
    /// `(id, index)` sorted by id.
    pub(super) by_id: Vec<(u64, u32)>,
}

impl MidiSet {
    pub(super) fn new(tracks: Vec<MidiTrack>) -> Self {
        let mut by_id: Vec<(u64, u32)> = tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, i as u32))
            .collect();
        by_id.sort_unstable();
        Self { tracks, by_id }
    }

    /// Moves each plugin host this list holds into the same track's
    /// stand-in in `newer` (same id, same kind, an instrument that has no
    /// host), for a list that is skipped and freed unplayed. Real-time safe.
    pub(super) fn pass_hosts_to(&mut self, newer: &mut MidiSet) {
        for track in &mut self.tracks {
            let Some(mine) = track.instrument.as_mut().filter(|s| s.instrument.hosted()) else {
                continue;
            };
            let Ok(k) = newer.by_id.binary_search_by_key(&track.id, |&(id, _)| id) else {
                continue;
            };
            let at = newer.by_id[k].1 as usize;
            if let Some(theirs) = newer.tracks[at].instrument.as_mut()
                && theirs.kind == mine.kind
                && !theirs.instrument.hosted()
            {
                std::mem::swap(&mut mine.instrument, &mut theirs.instrument);
            }
        }
    }

    /// Some track's instrument runs in a plugin host.
    pub(super) fn hosted(&self) -> bool {
        self.tracks.iter().any(|t| {
            t.instrument
                .as_ref()
                .is_some_and(|spec| spec.instrument.hosted())
        })
    }
}
