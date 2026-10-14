//! Group tracks (M4 slice 3): grouping, ungrouping, taking tracks out of
//! a group, and moving tracks between groups. A group's tracks always
//! follow it in the list (`lumen_project::groups`), so every edit here sets
//! parents and a rough order, then puts the list back in that order. An
//! edit that would make the routing feed back (a group put inside a track
//! that its own output reaches), or nest groups more than three deep, is
//! refused and changes nothing.

use std::sync::Arc;

use lumen_project::groups;
use lumen_project::model::{Id, Project, Track, TrackKind};

use super::mixer::{Refused, forget_routes_to};
use super::{Document, plural, track_color};

/// Where moved tracks land: at `index` among the tracks that stay, inside
/// `group` (`None`: at the top).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub index: usize,
    pub group: Option<Id>,
}

impl Document {
    /// Ctrl+G: puts `ids` (and what is inside those that are groups) into a
    /// new group where the topmost of them was, inside the group that held
    /// them all. Returns and the master are left out. Returns the new
    /// group; refused when nothing can be grouped or groups would go too
    /// deep.
    pub fn group_tracks(&mut self, ids: &[Id]) -> Result<Id, Refused> {
        let tracks = &self.project().tracks;
        let groupable: Vec<Id> = tracks
            .iter()
            .filter(|t| ids.contains(&t.id) && groups::can_be_grouped(t.kind))
            .map(|t| t.id)
            .collect();
        let members = groups::outermost(tracks, &groupable);
        let first = *members.first().ok_or(Refused::Invalid)?;
        let outer = groups::common_group(tracks, &members);
        let number = tracks.iter().filter(|t| t.kind == TrackKind::Group).count() + 1;
        let color = track_color(tracks.len());
        let id = self.fresh_id();
        let group = Arc::new(Track {
            parent: outer,
            ..Track::new(id, format!("Group {number}"), TrackKind::Group, color)
        });
        self.restructure("Group tracks", |project| {
            for track in &mut project.tracks {
                if members.contains(&track.id) {
                    Arc::make_mut(track).parent = Some(id);
                }
            }
            let at = project.tracks.iter().position(|t| t.id == first);
            project
                .tracks
                .insert(at.unwrap_or(project.tracks.len()), group.clone());
        })
        .map(|()| id)
    }

    /// Ctrl+Shift+G: removes the groups among `ids`, their tracks moving up
    /// a level in their place; other tracks among `ids` come out of their
    /// group, landing right after it. One step either way.
    pub fn ungroup(&mut self, ids: &[Id]) -> Result<(), Refused> {
        let tracks = &self.project().tracks;
        let dissolved: Vec<Id> = tracks
            .iter()
            .filter(|t| ids.contains(&t.id) && t.kind == TrackKind::Group)
            .map(|t| t.id)
            .collect();
        // Tracks taken out, unless their own group goes anyway.
        let out: Vec<Id> = groups::outermost(tracks, ids)
            .into_iter()
            .filter(|id| !dissolved.contains(id))
            .filter(|&id| {
                tracks
                    .iter()
                    .find(|t| t.id == id)
                    .and_then(|t| t.parent)
                    .is_some_and(|p| !dissolved.contains(&p))
            })
            .collect();
        if dissolved.is_empty() && out.is_empty() {
            return Err(Refused::Invalid);
        }
        let label = if dissolved.is_empty() {
            plural(
                out.len(),
                "Take track out of group",
                "Take {} tracks out of groups",
            )
        } else {
            plural(dissolved.len(), "Ungroup", "Ungroup {} groups")
        };
        self.restructure(&label, |project| {
            for &id in out.iter().rev() {
                take_out(&mut project.tracks, id);
            }
            for &group in &dissolved {
                dissolve(&mut project.tracks, group);
            }
            forget_routes_to(project, &dissolved);
        })
    }

    /// Moves `ids` (with what is inside those that are groups, in their
    /// order) to `to`. Returns and the master stay at the top. Refused when
    /// `to` is inside the tracks moving, would feed back, or would nest
    /// groups too deep.
    pub fn move_tracks(&mut self, ids: &[Id], to: Place) -> Result<(), Refused> {
        let (label, change) = self.planned_move(ids, to)?;
        self.restructure(&label, change)
    }

    /// Whether [`move_tracks`](Self::move_tracks) would take that move,
    /// without making it (a drag's drop line greys out when not).
    pub fn check_move(&self, ids: &[Id], to: Place) -> Result<(), Refused> {
        let (_, change) = self.planned_move(ids, to)?;
        self.trial(&change).map(|_| ())
    }

    /// The step name and the change of a move.
    fn planned_move(
        &self,
        ids: &[Id],
        to: Place,
    ) -> Result<(String, impl Fn(&mut Project) + use<>), Refused> {
        let tracks = &self.project().tracks;
        let heads = groups::outermost(tracks, ids);
        let moving = groups::with_contents(tracks, &heads);
        if let Some(group) = to.group {
            let is_group = tracks
                .iter()
                .any(|t| t.id == group && t.kind == TrackKind::Group);
            if !is_group || moving.contains(&group) {
                return Err(Refused::Invalid);
            }
        }
        let label = plural(heads.len(), "Move track", "Move {} tracks");
        let change = move |project: &mut Project| {
            let (mut moved, mut rest): (Vec<_>, Vec<_>) = project
                .tracks
                .drain(..)
                .partition(|t| moving.contains(&t.id));
            for track in &mut moved {
                if heads.contains(&track.id) {
                    let parent = to.group.filter(|_| groups::can_be_grouped(track.kind));
                    if track.parent != parent {
                        Arc::make_mut(track).parent = parent;
                    }
                }
            }
            let at = to.index.min(rest.len());
            rest.splice(at..at, moved);
            project.tracks = rest;
        };
        Ok((label, change))
    }

    /// The project after `change`, with every track back under its group;
    /// refused if the routing would feed back, or if groups would go deeper
    /// than `MAX_NESTING` (and deeper than they already were: an older
    /// project nested deeper can still be edited).
    fn trial(&self, change: &impl Fn(&mut Project)) -> Result<Project, Refused> {
        let mut trial = self.project().clone();
        change(&mut trial);
        groups::normalize(&mut trial.tracks);
        if lumen_project::routing::has_loop(&trial) {
            return Err(Refused::Loop);
        }
        let deep = groups::nesting(&trial.tracks);
        if deep > groups::MAX_NESTING && deep > groups::nesting(&self.project().tracks) {
            return Err(Refused::TooDeep);
        }
        Ok(trial)
    }

    /// Applies a change to the track list as one step named `label`, then
    /// puts every track back under its group, unless [`trial`](Self::trial)
    /// refuses it. `change` runs twice (a trial first), so it must not
    /// depend on anything but the project.
    fn restructure(&mut self, label: &str, change: impl Fn(&mut Project)) -> Result<(), Refused> {
        self.trial(&change)?;
        self.history.apply(label, None, |project| {
            change(project);
            groups::normalize(&mut project.tracks);
        });
        Ok(())
    }
}

/// Takes track `id` (and what is inside it) out of its group, to right
/// after that group's run, one level up.
fn take_out(tracks: &mut Vec<Arc<Track>>, id: Id) {
    let Some(index) = tracks.iter().position(|t| t.id == id) else {
        return;
    };
    let Some(parent) = tracks[index].parent else {
        return;
    };
    let grandparent = tracks
        .iter()
        .find(|t| t.id == parent)
        .and_then(|t| t.parent);
    let run: Vec<Arc<Track>> = tracks.drain(groups::span(tracks, index)).collect();
    let Some(group) = tracks.iter().position(|t| t.id == parent) else {
        return;
    };
    let end = groups::span(tracks, group).end;
    tracks.splice(end..end, run);
    Arc::make_mut(&mut tracks[end]).parent = grandparent;
}

/// Removes group `id`; its tracks move up a level, where they were.
fn dissolve(tracks: &mut Vec<Arc<Track>>, id: Id) {
    let Some(index) = tracks.iter().position(|t| t.id == id) else {
        return;
    };
    let parent = tracks.remove(index).parent;
    for track in tracks.iter_mut() {
        if track.parent == Some(id) {
            Arc::make_mut(track).parent = parent;
        }
    }
}

/// Copies of the tracks headed by `heads` and everything inside them, with
/// fresh ids from `fresh` (clips too) and their groups pointing at the
/// copies. Returns `(original head, copied run)` pairs.
pub(super) fn copy_runs(
    tracks: &[Arc<Track>],
    heads: &[Id],
    mut fresh: impl FnMut() -> Id,
) -> Vec<(Id, Vec<Track>)> {
    let mut runs = Vec::new();
    for &head in heads {
        let Some(index) = tracks.iter().position(|t| t.id == head) else {
            continue;
        };
        let originals = &tracks[groups::span(tracks, index)];
        let new_ids: Vec<Id> = originals.iter().map(|_| fresh()).collect();
        let renamed = |id: Id| {
            originals
                .iter()
                .position(|t| t.id == id)
                .map_or(id, |i| new_ids[i])
        };
        let run = originals
            .iter()
            .zip(&new_ids)
            .map(|(original, &id)| {
                let mut copy = (**original).clone();
                copy.id = id;
                // The head stays where it was; those inside follow the copies.
                if original.id != head {
                    copy.parent = copy.parent.map(renamed);
                }
                for device in &mut copy.devices {
                    device.id = fresh();
                }
                for clip in &mut copy.clips {
                    let mut c = (**clip).clone();
                    c.id = fresh();
                    *clip = Arc::new(c);
                }
                copy
            })
            .collect();
        runs.push((head, run));
    }
    runs
}

#[cfg(test)]
mod tests {
    use lumen_project::model::Output;

    use super::*;

    /// Drums (a group) holding Kick and Snare, then Bass and a return.
    fn setup() -> (Document, [Id; 5]) {
        let mut doc = Document::new(crate::empty_project());
        let kick = doc.add_track(TrackKind::Audio, None);
        let snare = doc.add_track(TrackKind::Audio, None);
        let bass = doc.add_track(TrackKind::Midi, None);
        let ret = doc.add_track(TrackKind::Return, None);
        let drums = doc.group_tracks(&[kick, snare]).unwrap();
        (doc, [drums, kick, snare, bass, ret])
    }

    fn order(doc: &Document) -> Vec<Id> {
        doc.project().tracks.iter().map(|t| t.id).collect()
    }

    fn parent(doc: &Document, id: Id) -> Option<Id> {
        doc.project()
            .tracks
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.parent)
    }

    #[test]
    fn grouping_puts_the_tracks_in_a_new_group_where_they_were_in_one_step() {
        let (mut doc, [drums, kick, snare, bass, ret]) = setup();
        assert_eq!(order(&doc), [drums, kick, snare, bass, ret]);
        assert_eq!(parent(&doc, kick), Some(drums));
        assert_eq!(doc.project().tracks[0].name, "Group 1");
        assert_eq!(doc.history().undo_label(), Some("Group tracks"));
        // Nesting: the snare alone, inside the drums.
        let inner = doc.group_tracks(&[snare]).unwrap();
        assert_eq!(order(&doc), [drums, kick, inner, snare, bass, ret]);
        assert_eq!(parent(&doc, inner), Some(drums));
        // A return is left out; with nothing else, nothing happens.
        assert_eq!(doc.group_tracks(&[ret]), Err(Refused::Invalid));
        // Tracks far apart gather under the new group at the first's place.
        let outer = doc.group_tracks(&[kick, bass, ret]).unwrap();
        assert_eq!(parent(&doc, outer), None, "they share no group");
        assert_eq!(order(&doc), [drums, inner, snare, outer, kick, bass, ret]);
        assert_eq!(parent(&doc, ret), None, "returns stay at the top");
        doc.undo();
        doc.undo();
        doc.undo();
        assert_eq!(order(&doc), [kick, snare, bass, ret]);
        assert_eq!(parent(&doc, kick), None);
    }

    #[test]
    fn grouping_a_whole_group_wraps_it() {
        let (mut doc, [drums, kick, snare, ..]) = setup();
        let outer = doc.group_tracks(&[drums, kick]).unwrap();
        assert_eq!(&order(&doc)[..4], [outer, drums, kick, snare]);
        assert_eq!(parent(&doc, drums), Some(outer));
        assert_eq!(parent(&doc, kick), Some(drums), "inside stays inside");
    }

    #[test]
    fn ungrouping_moves_the_tracks_up_a_level_in_one_step() {
        let (mut doc, [drums, kick, snare, bass, ret]) = setup();
        let inner = doc.group_tracks(&[snare]).unwrap();
        doc.set_output(bass, Output::Track(inner)).unwrap();
        assert_eq!(doc.ungroup(&[drums]), Ok(()));
        assert_eq!(order(&doc), [kick, inner, snare, bass, ret]);
        assert_eq!(parent(&doc, inner), None);
        assert_eq!(
            parent(&doc, snare),
            Some(inner),
            "nested groups keep theirs"
        );
        assert_eq!(doc.history().undo_label(), Some("Ungroup"));
        // Routing to a group that goes falls back to automatic.
        doc.ungroup(&[inner]).unwrap();
        let bass_track = doc.project().tracks.iter().find(|t| t.id == bass).unwrap();
        assert_eq!(bass_track.output, Output::Auto);
        doc.undo();
        doc.undo();
        assert_eq!(order(&doc), [drums, kick, inner, snare, bass, ret]);
        // Nothing to ungroup.
        assert_eq!(doc.ungroup(&[bass]), Err(Refused::Invalid));
    }

    #[test]
    fn a_track_taken_out_lands_right_after_its_group() {
        let (mut doc, [drums, kick, snare, bass, ret]) = setup();
        doc.ungroup(&[kick]).unwrap();
        assert_eq!(order(&doc), [drums, snare, kick, bass, ret]);
        assert_eq!(parent(&doc, kick), None);
        assert_eq!(doc.history().undo_label(), Some("Take track out of group"));
        // Two at once keep their order.
        let (mut doc, [drums, kick, snare, ..]) = setup();
        doc.ungroup(&[kick, snare]).unwrap();
        assert_eq!(&order(&doc)[..3], [drums, kick, snare]);
    }

    #[test]
    fn moving_takes_groups_whole_and_enters_or_leaves_groups() {
        let (mut doc, [drums, kick, snare, bass, ret]) = setup();
        // The drums below the bass: the kick and snare come along.
        doc.move_tracks(
            &[drums],
            Place {
                index: 1,
                group: None,
            },
        )
        .unwrap();
        assert_eq!(order(&doc), [bass, drums, kick, snare, ret]);
        // The bass into the drums, between the kick and the snare.
        doc.move_tracks(
            &[bass],
            Place {
                index: 2,
                group: Some(drums),
            },
        )
        .unwrap();
        assert_eq!(order(&doc), [drums, kick, bass, snare, ret]);
        assert_eq!(parent(&doc, bass), Some(drums));
        // A group never goes inside itself.
        assert_eq!(
            doc.move_tracks(
                &[drums],
                Place {
                    index: 1,
                    group: Some(drums)
                }
            ),
            Err(Refused::Invalid)
        );
        // A return dropped into a group stays at the top.
        doc.move_tracks(
            &[ret],
            Place {
                index: 1,
                group: Some(drums),
            },
        )
        .unwrap();
        assert_eq!(parent(&doc, ret), None);
        assert!(lumen_project::groups::in_order(&doc.project().tracks));
    }

    #[test]
    fn deleting_a_group_takes_its_tracks_and_duplicating_copies_them() {
        let (mut doc, [drums, kick, snare, bass, ret]) = setup();
        let copies = doc.duplicate_tracks(&[drums, kick]);
        assert_eq!(copies.len(), 1, "the kick comes with its group");
        let tracks = &doc.project().tracks;
        assert_eq!(tracks.len(), 8);
        assert_eq!(tracks[3].id, copies[0], "right after the drums' run");
        assert_eq!(
            (tracks[4].parent, tracks[5].parent),
            (Some(copies[0]), Some(copies[0]))
        );
        assert_eq!(tracks[4].name, "Audio 1");
        assert!(lumen_project::validate(doc.project()).is_ok());
        doc.delete_tracks(&[copies[0]]);
        assert_eq!(order(&doc), [drums, kick, snare, bass, ret]);
        doc.delete_tracks(&[drums]);
        assert_eq!(order(&doc), [bass, ret]);
        assert_eq!(doc.history().undo_label(), Some("Delete 3 tracks"));
        doc.undo();
        assert_eq!(order(&doc), [drums, kick, snare, bass, ret]);
    }

    #[test]
    fn arming_a_group_arms_what_is_inside() {
        let (mut doc, [drums, kick, snare, bass, _]) = setup();
        doc.set_armed(&[drums], true);
        let armed =
            |doc: &Document, id: Id| doc.project().tracks.iter().any(|t| t.id == id && t.armed);
        assert!(armed(&doc, kick) && armed(&doc, snare));
        assert!(!armed(&doc, drums), "a group itself holds no arm");
        assert!(!armed(&doc, bass));
        doc.set_armed(&[drums], false);
        assert!(!armed(&doc, kick));
    }

    #[test]
    fn groups_go_three_deep_at_most() {
        // MT-MIX-3 run 1 (owner): Group > Group > Group > tracks.
        let (mut doc, [_, kick, snare, bass, _]) = setup();
        let second = doc.group_tracks(&[kick]).unwrap();
        let third = doc.group_tracks(&[kick]).unwrap();
        assert_eq!(groups::nesting(&doc.project().tracks), 3);
        let before = order(&doc);
        assert_eq!(doc.group_tracks(&[kick]), Err(Refused::TooDeep));
        assert_eq!(
            doc.group_tracks(&[third]),
            Err(Refused::TooDeep),
            "wrapping"
        );
        assert_eq!(order(&doc), before, "nothing changed");
        // A group dragged into the third level is refused; a track is not.
        let group = doc.group_tracks(&[bass]).unwrap();
        let into_third = Place {
            index: 4,
            group: Some(third),
        };
        assert_eq!(doc.check_move(&[group], into_third), Err(Refused::TooDeep));
        assert_eq!(doc.move_tracks(&[group], into_third), Err(Refused::TooDeep));
        assert_eq!(doc.check_move(&[snare], into_third), Ok(()));
        // Into the second level, a group with nothing nested is fine.
        let into_second = Place {
            index: 2,
            group: Some(second),
        };
        assert_eq!(doc.move_tracks(&[group], into_second), Ok(()));
        assert_eq!(parent(&doc, group), Some(second));
        assert!(groups::in_order(&doc.project().tracks));
    }

    #[test]
    fn a_move_that_would_feed_back_is_refused() {
        let (mut doc, [drums, kick, ..]) = setup();
        let other = doc.group_tracks(&[kick]).unwrap();
        doc.move_tracks(
            &[other],
            Place {
                index: 0,
                group: None,
            },
        )
        .unwrap();
        // The drums go out into `other`; putting `other` in the drums loops.
        doc.set_output(drums, Output::Track(other)).unwrap();
        let before = order(&doc);
        assert_eq!(
            doc.move_tracks(
                &[other],
                Place {
                    index: 1,
                    group: Some(drums)
                }
            ),
            Err(Refused::Loop)
        );
        assert_eq!(order(&doc), before);
    }
}
