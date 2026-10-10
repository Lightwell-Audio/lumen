//! Group tracks (M4 slice 3): the tree that `parent` describes, kept in the
//! track list's order. A group's tracks follow it directly, nested ones
//! inside theirs, as in Ableton: so a group and everything in it is always
//! one run of the list (its *span*), and moving, deleting or duplicating a
//! group can take the run whole.
//!
//! Edits set parents and a rough order, then call [`normalize`], which puts
//! every track back under its group without changing the order of
//! siblings.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use crate::model::{Id, Track, TrackKind};

/// Groups go three deep at most (the owner, MT-MIX-3 run 1): a group can be
/// inside two others, no more. Edits keep to it; files are not rejected
/// for it (a deeper project made before still opens and plays).
pub const MAX_NESTING: usize = 3;

/// How deep groups go: 1 for groups with none around them, 0 for none.
pub fn nesting(tracks: &[Arc<Track>]) -> usize {
    tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Group)
        .map(|t| depth(tracks, t.id) + 1)
        .max()
        .unwrap_or(0)
}

/// Whether a track of `kind` can be inside a group. Returns and the master
/// stay at the top (Ableton's rule).
pub fn can_be_grouped(kind: TrackKind) -> bool {
    matches!(kind, TrackKind::Audio | TrackKind::Midi | TrackKind::Group)
}

/// How many groups `id` is inside.
pub fn depth(tracks: &[Arc<Track>], id: Id) -> usize {
    ancestors(tracks, id).count()
}

/// The groups `id` is inside, its own group first. Stops on a cycle.
pub fn ancestors(tracks: &[Arc<Track>], id: Id) -> impl Iterator<Item = Id> + '_ {
    let parent = move |id: Id| tracks.iter().find(|t| t.id == id).and_then(|t| t.parent);
    let mut seen = HashSet::from([id]);
    std::iter::successors(parent(id), move |&p| parent(p)).take_while(move |p| seen.insert(*p))
}

/// Whether `id` is inside `group`, directly or in a nested group.
pub fn is_inside(tracks: &[Arc<Track>], id: Id, group: Id) -> bool {
    ancestors(tracks, id).any(|a| a == group)
}

/// The run of the list that the track at `index` heads: itself, and
/// everything inside it when it is a group.
pub fn span(tracks: &[Arc<Track>], index: usize) -> Range<usize> {
    let Some(head) = tracks.get(index) else {
        return index..index;
    };
    let end = tracks[index + 1..]
        .iter()
        .position(|t| !is_inside(tracks, t.id, head.id))
        .map_or(tracks.len(), |n| index + 1 + n);
    index..end
}

/// `ids` and everything inside the groups among them, in list order.
pub fn with_contents(tracks: &[Arc<Track>], ids: &[Id]) -> Vec<Id> {
    tracks
        .iter()
        .filter(|t| ids.contains(&t.id) || ancestors(tracks, t.id).any(|a| ids.contains(&a)))
        .map(|t| t.id)
        .collect()
}

/// The tracks of `ids` that are not inside another of `ids`, in list order:
/// what a move, a duplicate or a new group takes whole.
pub fn outermost(tracks: &[Arc<Track>], ids: &[Id]) -> Vec<Id> {
    tracks
        .iter()
        .filter(|t| ids.contains(&t.id) && !ancestors(tracks, t.id).any(|a| ids.contains(&a)))
        .map(|t| t.id)
        .collect()
}

/// The innermost group that holds every one of `ids` (`None`: the top).
pub fn common_group(tracks: &[Arc<Track>], ids: &[Id]) -> Option<Id> {
    let mut chains = ids
        .iter()
        .map(|&id| ancestors(tracks, id).collect::<Vec<_>>());
    let first = chains.next()?;
    let rest: Vec<Vec<Id>> = chains.collect();
    first
        .into_iter()
        .find(|g| rest.iter().all(|chain| chain.contains(g)))
}

/// Whether every track sits inside its group's run: the order the
/// arrangement draws. Parents that are missing or not groups are left to
/// validation.
pub fn in_order(tracks: &[Arc<Track>]) -> bool {
    // The groups open at this point of the list, outermost first.
    let mut open: Vec<Id> = Vec::new();
    for track in tracks {
        match track.parent {
            None => open.clear(),
            Some(parent) => match open.iter().rposition(|&g| g == parent) {
                Some(at) => open.truncate(at + 1),
                None => return false,
            },
        }
        if track.kind == TrackKind::Group {
            open.push(track.id);
        }
    }
    true
}

/// Puts every track right under its group, nested ones inside theirs,
/// keeping the order of tracks that share a group. Tracks whose group is
/// missing go to the top; tracks caught in a nesting cycle keep their
/// place at the end (validation rejects them).
pub fn normalize(tracks: &mut Vec<Arc<Track>>) {
    if in_order(tracks) {
        return;
    }
    let groups: HashSet<Id> = tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Group)
        .map(|t| t.id)
        .collect();
    let mut children: HashMap<Option<Id>, Vec<usize>> = HashMap::new();
    for (i, t) in tracks.iter().enumerate() {
        let parent = t.parent.filter(|p| groups.contains(p));
        children.entry(parent).or_default().push(i);
    }
    let mut order = Vec::with_capacity(tracks.len());
    let mut placed = vec![false; tracks.len()];
    let mut stack: Vec<usize> = children
        .get(&None)
        .map_or(Vec::new(), |c| c.iter().rev().copied().collect());
    while let Some(i) = stack.pop() {
        if std::mem::replace(&mut placed[i], true) {
            continue;
        }
        order.push(i);
        if let Some(kids) = children.get(&Some(tracks[i].id)) {
            stack.extend(kids.iter().rev());
        }
    }
    order.extend((0..tracks.len()).filter(|&i| !placed[i]));
    let mut old: Vec<Option<Arc<Track>>> = tracks.drain(..).map(Some).collect();
    tracks.extend(order.into_iter().filter_map(|i| old[i].take()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Color;

    /// Tracks from `(id, parent, is_group)`.
    fn tracks(spec: &[(u64, Option<u64>, bool)]) -> Vec<Arc<Track>> {
        spec.iter()
            .map(|&(id, parent, group)| {
                let kind = if group {
                    TrackKind::Group
                } else {
                    TrackKind::Audio
                };
                Arc::new(Track {
                    parent: parent.map(Id),
                    ..Track::new(Id(id), format!("{id}"), kind, Color(0))
                })
            })
            .collect()
    }

    fn ids(tracks: &[Arc<Track>]) -> Vec<u64> {
        tracks.iter().map(|t| t.id.0).collect()
    }

    /// Drums (1) holds Kick (2) and Hats (3), a group holding 4 and 5;
    /// Bass (6) is at the top.
    fn drums() -> Vec<Arc<Track>> {
        tracks(&[
            (1, None, true),
            (2, Some(1), false),
            (3, Some(1), true),
            (4, Some(3), false),
            (5, Some(3), false),
            (6, None, false),
        ])
    }

    #[test]
    fn depth_spans_and_contents_follow_the_nesting() {
        let t = drums();
        assert_eq!([1, 2, 3, 4, 6].map(|id| depth(&t, Id(id))), [0, 1, 1, 2, 0]);
        assert_eq!(span(&t, 0), 0..5, "the group and everything in it");
        assert_eq!(span(&t, 2), 2..5, "a nested group");
        assert_eq!(span(&t, 1), 1..2, "a track is its own span");
        assert_eq!(span(&t, 5), 5..6);
        assert_eq!(
            with_contents(&t, &[Id(3), Id(6)]),
            [Id(3), Id(4), Id(5), Id(6)]
        );
        assert_eq!(outermost(&t, &[Id(4), Id(3), Id(1)]), [Id(1)]);
        assert_eq!(outermost(&t, &[Id(4), Id(2)]), [Id(2), Id(4)]);
        assert_eq!(common_group(&t, &[Id(4), Id(5)]), Some(Id(3)));
        assert_eq!(common_group(&t, &[Id(4), Id(2)]), Some(Id(1)));
        assert_eq!(common_group(&t, &[Id(4), Id(6)]), None);
        assert!(is_inside(&t, Id(5), Id(1)));
        assert_eq!(nesting(&t), 2, "Hats inside Drums");
        assert_eq!(nesting(&t[5..]), 0);
        assert!(!is_inside(&t, Id(6), Id(1)));
    }

    #[test]
    fn order_means_every_track_inside_its_group_run() {
        assert!(in_order(&drums()));
        // Kick after Bass: outside its group's run.
        let mut t = drums();
        let kick = t.remove(1);
        t.push(kick);
        assert!(!in_order(&t));
        normalize(&mut t);
        assert_eq!(ids(&t), [1, 3, 4, 5, 2, 6], "back in, after its sibling");
        // A track before its group.
        let mut t = drums();
        t.swap(0, 1);
        assert!(!in_order(&t));
        normalize(&mut t);
        assert_eq!(ids(&t), ids(&drums()));
    }

    #[test]
    fn normalize_keeps_order_and_survives_bad_parents() {
        let mut t = drums();
        normalize(&mut t);
        assert_eq!(ids(&t), ids(&drums()), "already in order: unchanged");
        // A missing group: the track goes to the top, in its place.
        let mut t = tracks(&[(1, None, false), (2, Some(9), false), (3, None, false)]);
        normalize(&mut t);
        assert_eq!(ids(&t), [1, 2, 3]);
        // A cycle is kept, at the end, for validation to report.
        let mut t = tracks(&[(1, Some(2), true), (2, Some(1), true), (3, None, false)]);
        normalize(&mut t);
        assert_eq!(ids(&t), [3, 1, 2]);
        assert_eq!(depth(&t, Id(1)), 1, "ancestors stop on a cycle");
    }
}
