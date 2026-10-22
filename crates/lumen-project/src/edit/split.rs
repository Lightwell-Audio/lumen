//! Pure clip splitting at arrangement ticks.

use std::collections::HashSet;
use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::model::{Id, Project};

/// Splits unlocked clips at interior ticks, keeping each source id on its
/// first part. The returned ids cover the resulting parts in track order.
pub fn clips(
    project: &mut Project,
    ids: &[Id],
    at: &[Ticks],
    fresh: &mut impl FnMut() -> Id,
) -> Vec<Id> {
    if ids.is_empty() || at.is_empty() {
        return Vec::new();
    }
    let selected: HashSet<Id> = ids.iter().copied().collect();
    let mut cuts = at.to_vec();
    cuts.sort_unstable();
    cuts.dedup();
    let tempo = super::tempo_of(project);
    let source_project = project.clone();
    let mut result = Vec::new();

    for track_index in 0..project.tracks.len() {
        let track = &project.tracks[track_index];
        let affected = track.clips.iter().any(|clip| {
            if !selected.contains(&clip.id) || clip.locked {
                return false;
            }
            let end = clip.start + clip.length;
            cuts.iter().any(|cut| clip.start < *cut && *cut < end)
        });
        if !affected {
            result.extend(
                track.clips.iter().filter(|clip| selected.contains(&clip.id)).map(|clip| clip.id),
            );
            continue;
        }

        let original = track.clips.clone();
        let mut replacement = Vec::with_capacity(original.len());
        for clip in &original {
            if !selected.contains(&clip.id) || clip.locked {
                replacement.push(Arc::clone(clip));
                continue;
            }
            let start = clip.start;
            let end = clip.start + clip.length;
            let boundaries: Vec<_> = cuts
                .iter()
                .copied()
                .filter(|cut| start < *cut && *cut < end)
                .collect();
            if boundaries.is_empty() {
                result.push(clip.id);
                replacement.push(Arc::clone(clip));
                continue;
            }

            let mut left = start;
            for (index, right) in boundaries
                .iter()
                .copied()
                .chain(std::iter::once(end))
                .enumerate()
            {
                let mut part = super::slice(&source_project, &tempo, clip, left, right)
                    .expect("interior split boundaries produce non-empty slices");
                if index > 0 {
                    part.id = fresh();
                }
                result.push(part.id);
                replacement.push(Arc::new(part));
                left = right;
            }
        }
        Arc::make_mut(&mut project.tracks[track_index]).clips = replacement;
    }
    result
}
