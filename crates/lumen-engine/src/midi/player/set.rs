use super::super::MidiTrack;

pub(in crate::midi) struct MidiSet {
    pub(super) tracks: Vec<MidiTrack>,
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

    pub(super) fn hosted(&self) -> bool {
        self.tracks.iter().any(|t| {
            t.instrument
                .as_ref()
                .is_some_and(|spec| spec.instrument.hosted())
        })
    }
}
