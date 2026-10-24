//! Prepared clip-list metadata shared with the real-time player.

use std::collections::HashMap;
use std::sync::Arc;

use super::seams::{envelopes_meet, fades_meet, source_meets};
use super::{Clip, FadeFrames, Fades, MAX_TABLES, TABLE_STEPS, Table};

/// A clip list with its lookup, fade and join metadata prepared off-thread.
pub(super) struct ClipSet {
    pub(super) clips: Vec<Clip>,
    pub(super) levels: Vec<f32>,
    pub(super) by_id: Vec<(u64, u32)>,
    pub(super) swapped: Vec<bool>,
    pub(super) fades: Vec<Fades>,
    pub(super) adjacent: Vec<(usize, usize)>,
    pub(super) seams: Vec<[bool; 2]>,
    pub(super) seam_lead: Vec<i64>,
    pub(super) seam_total: Vec<i64>,
    pub(super) seam_root: Vec<usize>,
    pub(super) join_predecessor: Vec<Option<usize>>,
    pub(super) join_successor: Vec<Option<usize>>,
    pub(super) join_pairs: Vec<(usize, usize)>,
    pub(super) has_joins: bool,
    pub(super) join_frames: Vec<i64>,
    pub(super) join_outgoing: Vec<bool>,
}

impl ClipSet {
    /// Allocates; called off the audio thread. Fade tables are shared by curve.
    pub(super) fn new(clips: Vec<Clip>, tables: &mut HashMap<[u32; 4], Table>) -> Self {
        let mut by_id: Vec<(u64, u32)> = clips
            .iter()
            .enumerate()
            .map(|(i, clip)| (clip.id, i as u32))
            .collect();
        by_id.sort_unstable();
        if tables.len() > MAX_TABLES {
            tables.clear();
        }
        let mut table = |fade: &FadeFrames| {
            (fade.frames > 0).then(|| {
                let key = <[f32; 4]>::from(fade.curve).map(f32::to_bits);
                Arc::clone(
                    tables
                        .entry(key)
                        .or_insert_with(|| fade.curve.table(TABLE_STEPS).into()),
                )
            })
        };
        let fades = clips
            .iter()
            .map(|clip| Fades([table(&clip.shape.fade_in), table(&clip.shape.fade_out)]))
            .collect();
        let mut order: Vec<usize> = (0..clips.len()).collect();
        order.sort_unstable_by_key(|&i| (clips[i].track, clips[i].start));
        let adjacent = order
            .windows(2)
            .filter_map(|pair| {
                let (a, b) = (pair[0], pair[1]);
                let left = &clips[a];
                let right = &clips[b];
                (left.track == right.track
                    && Arc::ptr_eq(&left.audio, &right.audio)
                    && source_meets(left, right)
                    && left.gain == right.gain
                    && envelopes_meet(left, right)
                    && fades_meet(left, right))
                .then_some((a, b))
            })
            .collect();
        let mut join_pairs = Vec::new();
        let mut join_predecessor = vec![None; clips.len()];
        let mut join_successor = vec![None; clips.len()];
        for pair in order.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            if clips[left].track == clips[right].track && clips[right].shape.crossfade {
                join_pairs.push((left, right));
                join_predecessor[right] = Some(left);
                join_successor[left] = Some(right);
            }
        }
        let has_joins = !join_pairs.is_empty();
        Self {
            levels: vec![f32::NAN; clips.len()],
            swapped: vec![false; clips.len()],
            fades,
            adjacent,
            seams: vec![[false; 2]; clips.len()],
            seam_lead: vec![0; clips.len()],
            seam_total: clips.iter().map(|clip| clip.length).collect(),
            seam_root: (0..clips.len()).collect(),
            join_predecessor,
            join_successor,
            join_pairs,
            has_joins,
            join_frames: vec![0; clips.len()],
            join_outgoing: vec![false; clips.len()],
            clips,
            by_id,
        }
    }

    /// Carries levels and marks clips whose own placement or join dependency changed.
    pub(super) fn carry_levels(&mut self, old: &mut ClipSet) -> bool {
        self.swapped.fill(false);
        old.swapped.fill(true);
        let (mut i, mut j) = (0, 0);
        while let (Some(&(new_id, new_index)), Some(&(old_id, old_index))) =
            (self.by_id.get(i), old.by_id.get(j))
        {
            match new_id.cmp(&old_id) {
                std::cmp::Ordering::Less => {
                    self.mark_join_partners(new_index as usize);
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    old.mark_join_partners(old_index as usize);
                    j += 1;
                }
                std::cmp::Ordering::Equal => {
                    let (new_index, old_index) = (new_index as usize, old_index as usize);
                    self.levels[new_index] = old.levels[old_index];
                    let moved = !self.clips[new_index].placed_as(&old.clips[old_index]);
                    let predecessor_changed = partner_changed(
                        self,
                        self.join_predecessor[new_index],
                        old,
                        old.join_predecessor[old_index],
                    );
                    let successor_changed = partner_changed(
                        self,
                        self.join_successor[new_index],
                        old,
                        old.join_successor[old_index],
                    );
                    self.swapped[new_index] = moved;
                    old.swapped[old_index] = moved;
                    let joined = self.join_predecessor[new_index].is_some()
                        || self.join_successor[new_index].is_some()
                        || old.join_predecessor[old_index].is_some()
                        || old.join_successor[old_index].is_some();
                    if predecessor_changed || successor_changed || (moved && joined) {
                        self.swapped[new_index] = true;
                        old.swapped[old_index] = true;
                        self.mark_join_partners(new_index);
                        old.mark_join_partners(old_index);
                        for partner in [
                            self.join_predecessor[new_index],
                            self.join_successor[new_index],
                        ]
                        .into_iter()
                        .flatten()
                        {
                            self.swapped[partner] = true;
                        }
                        for partner in [
                            old.join_predecessor[old_index],
                            old.join_successor[old_index],
                        ]
                        .into_iter()
                        .flatten()
                        {
                            old.swapped[partner] = true;
                        }
                    }
                    i += 1;
                    j += 1;
                }
            }
        }
        old.swapped.contains(&true)
    }

    fn mark_join_partners(&mut self, index: usize) {
        if self.join_predecessor[index].is_some() || self.join_successor[index].is_some() {
            self.swapped[index] = true;
            if let Some(partner) = self.join_predecessor[index] {
                self.swapped[partner] = true;
            }
            if let Some(partner) = self.join_successor[index] {
                self.swapped[partner] = true;
            }
        }
    }
}

fn partner_changed(
    a_set: &ClipSet,
    a_partner: Option<usize>,
    b_set: &ClipSet,
    b_partner: Option<usize>,
) -> bool {
    match (a_partner, b_partner) {
        (None, None) => false,
        (Some(a), Some(b)) => {
            a_set.clips[a].id != b_set.clips[b].id || !a_set.clips[a].placed_as(&b_set.clips[b])
        }
        _ => true,
    }
}

impl Clip {
    fn placed_as(&self, other: &Clip) -> bool {
        self.start == other.start
            && self.offset == other.offset
            && self.length == other.length
            && self.shape == other.shape
            && self.envelope == other.envelope
            && Arc::ptr_eq(&self.audio, &other.audio)
    }
}
