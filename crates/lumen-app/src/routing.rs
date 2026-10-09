//! The engine's routing derived from the project (M4 slice 1, ADR-0035):
//! one node per track, with its volume, pan and whether it can be heard.
//! Kept pure, so the translation is tested, as `lanes::engine_clips` is.
//!
//! Until the model has a master track, outputs and sends (slice 2), every
//! track plays into a master node made here, which plays to the device's
//! first output pair; a track inside a group plays into the group.

use lumen_core::pan::PanLaw;
use lumen_engine::{Destination, NodeSpec, Routing};
use lumen_project::model::{Project, TrackKind};

/// The master's node id: past any id a project hands out.
pub const MASTER: u64 = u64::MAX;

/// The routing that plays `project`.
pub fn routing(project: &Project) -> Routing {
    let audible = crate::document::audible(project);
    let is_group = |id| {
        project
            .tracks
            .iter()
            .any(|t| t.id == id && t.kind == TrackKind::Group)
    };
    let mut nodes = vec![NodeSpec::unity(MASTER, Destination::Hardware(0))];
    for track in &project.tracks {
        let output = match track.parent {
            Some(group) if is_group(group) && !loops(project, track.id) => {
                Destination::Node(group.0)
            }
            _ => Destination::Node(MASTER),
        };
        nodes.push(NodeSpec {
            volume: db_to_gain(track.gain_db),
            pan: track.pan,
            law: PanLaw::default(),
            audible: audible(track),
            ..NodeSpec::unity(track.id.0, output)
        });
    }
    Routing { nodes }
}

/// Whether following `track`'s groups upwards comes back round (a damaged
/// file): it then plays straight to the master instead.
fn loops(project: &Project, track: lumen_project::model::Id) -> bool {
    let parent = |id| project.tracks.iter().find(|t| t.id == id)?.parent;
    let mut at = track;
    for _ in 0..=project.tracks.len() {
        match parent(at) {
            Some(up) if up == track => return true,
            Some(up) => at = up,
            None => return false,
        }
    }
    true
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lumen_project::model::Id;

    use super::*;
    use crate::document::Document;

    fn doc_with(kinds: &[TrackKind]) -> (Document, Vec<Id>) {
        let mut doc = Document::new(crate::empty_project());
        for &kind in kinds {
            doc.add_track(kind, None);
        }
        let ids = doc.project().tracks.iter().map(|t| t.id).collect();
        (doc, ids)
    }

    fn node(routing: &Routing, id: Id) -> &NodeSpec {
        routing.nodes.iter().find(|n| n.id == id.0).unwrap()
    }

    #[test]
    fn every_track_plays_into_the_master_with_its_volume_and_pan() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Audio]);
        doc.set_volume(ids[0], -6.0);
        doc.set_pan(ids[0], -0.5);
        let routing = routing(doc.project());
        lumen_engine::compile_routing(&routing).unwrap();
        assert_eq!(routing.nodes[0].output, Destination::Hardware(0));
        let track = node(&routing, ids[0]);
        assert_eq!(track.output, Destination::Node(MASTER));
        assert!((track.volume - 10f32.powf(-6.0 / 20.0)).abs() < 1e-6);
        assert_eq!((track.pan, track.law), (-0.5, PanLaw::ConstantPower));
        assert!(track.audible);
    }

    #[test]
    fn mute_and_solo_decide_what_is_heard() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio; 3]);
        let heard = |doc: &Document| {
            let routing = routing(doc.project());
            ids.iter()
                .map(|&id| node(&routing, id).audible)
                .collect::<Vec<_>>()
        };
        assert_eq!(heard(&doc), [true, true, true]);
        doc.set_mute(&ids[..1], true);
        assert_eq!(heard(&doc), [false, true, true]);
        doc.set_solo(&ids[2..], true, true);
        assert_eq!(heard(&doc), [false, false, true], "only the soloed track");
    }

    #[test]
    fn a_track_in_a_group_plays_into_the_group() {
        let (mut doc, ids) = doc_with(&[TrackKind::Group, TrackKind::Audio, TrackKind::Audio]);
        doc.apply("group", None, |p| {
            Arc::make_mut(&mut p.tracks[1]).parent = Some(ids[0]);
            // Not a group: ignored.
            Arc::make_mut(&mut p.tracks[2]).parent = Some(ids[1]);
        });
        let routing = routing(doc.project());
        lumen_engine::compile_routing(&routing).unwrap();
        assert_eq!(node(&routing, ids[0]).output, Destination::Node(MASTER));
        assert_eq!(node(&routing, ids[1]).output, Destination::Node(ids[0].0));
        assert_eq!(node(&routing, ids[2]).output, Destination::Node(MASTER));
    }

    #[test]
    fn groups_that_contain_each_other_play_to_the_master() {
        let (mut doc, ids) = doc_with(&[TrackKind::Group, TrackKind::Group]);
        doc.apply("damage", None, |p| {
            Arc::make_mut(&mut p.tracks[0]).parent = Some(ids[1]);
            Arc::make_mut(&mut p.tracks[1]).parent = Some(ids[0]);
        });
        let routing = routing(doc.project());
        lumen_engine::compile_routing(&routing).unwrap();
    }
}
