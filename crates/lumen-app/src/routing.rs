//! The engine's routing derived from the project (M4 slices 1 and 2,
//! ADR-0035): one node per track and one for the master, with each track's
//! input trim, polarity, volume, pan, whether it can be heard, its sends
//! and its output (`lumen_project::routing::destination`: its group, the
//! master, a group or return, or a device pair). Kept pure, so the
//! translation is tested, as `lanes::engine_clips` is.

use lumen_core::pan::PanLaw;
use lumen_engine::{Destination, NodeSpec, Routing, SendSpec};
use lumen_project::model::{Project, Track, TrackKind};
use lumen_project::routing::{self as rules, destination};

/// The routing that plays `project`.
pub fn routing(project: &Project) -> Routing {
    let audible = crate::document::audible(project);
    let nodes = rules::all_tracks(project)
        .map(|track| {
            let master = track.id == project.master.id;
            // Buses (groups, returns and the master) carry sound their
            // tracks have already panned: centred, they stay at unity, or
            // every group level took another 3 dB off what is inside it
            // (owner, after MT-MIX-3 run 1).
            let bus = matches!(
                track.kind,
                TrackKind::Group | TrackKind::Return | TrackKind::Master
            );
            NodeSpec {
                trim: db_to_gain(track.trim_db),
                invert: track.invert,
                volume: db_to_gain(track.gain_db),
                pan: track.pan,
                law: if bus {
                    PanLaw::Balance
                } else {
                    project.pan_law
                },
                audible: if master { !track.mute } else { audible(track) },
                // Silent sends stay in the routing, so turning one up glides
                // rather than changing the routing's shape.
                sends: track
                    .sends
                    .iter()
                    .map(|send| SendSpec {
                        to: send.to.0,
                        gain: send.level.gain(),
                        pre_fader: send.pre_fader,
                    })
                    .collect(),
                ..NodeSpec::unity(track.id.0, output(project, track))
            }
        })
        .collect();
    Routing { nodes }
}

fn output(project: &Project, track: &Track) -> Destination {
    match destination(project, track) {
        rules::Destination::Track(id) => Destination::Node(id.0),
        rules::Destination::Device(pair) => Destination::Hardware(pair),
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lumen_project::model::{Id, Level, Output};

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
        let master = doc.project().master.id;
        let routing = routing(doc.project());
        lumen_engine::compile_routing(&routing).unwrap();
        assert_eq!(node(&routing, master).output, Destination::Hardware(0));
        assert_eq!(node(&routing, master).law, PanLaw::Balance);
        let track = node(&routing, ids[0]);
        assert_eq!(track.output, Destination::Node(master.0));
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
    fn buses_are_unity_when_centred_and_tracks_follow_the_pan_law() {
        // Owner, after MT-MIX-3 run 1: tracks in groups played quieter.
        let (doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Group, TrackKind::Return]);
        let routing = routing(doc.project());
        assert_eq!(node(&routing, ids[0]).law, PanLaw::ConstantPower);
        for bus in [ids[1], ids[2], doc.project().master.id] {
            assert_eq!(node(&routing, bus).law, PanLaw::Balance);
            assert_eq!(PanLaw::Balance.gains(0.0), [1.0, 1.0]);
        }
    }

    #[test]
    fn solo_keeps_the_path_through_groups_open() {
        // MT-MIX-3 run 1: soloing a track inside a group silenced it.
        let (mut doc, ids) = doc_with(&[TrackKind::Audio; 3]);
        let outer = doc.group_tracks(&ids[..2]).unwrap();
        let inner = doc.group_tracks(&ids[..1]).unwrap();
        let heard = |doc: &Document, id| node(&routing(doc.project()), id).audible;
        doc.set_solo(&ids[..1], true, true);
        assert!(heard(&doc, ids[0]), "the soloed track");
        assert!(
            heard(&doc, inner) && heard(&doc, outer),
            "its groups stay open"
        );
        assert!(!heard(&doc, ids[1]), "its neighbour in the group is silent");
        assert!(!heard(&doc, ids[2]), "and the track outside");
        // A soloed group: everything inside it is heard.
        doc.set_solo(&[outer], true, true);
        assert!(heard(&doc, ids[0]) && heard(&doc, ids[1]) && heard(&doc, inner));
        assert!(!heard(&doc, ids[2]));
    }

    #[test]
    fn outputs_sends_polarity_trim_and_the_pan_law_reach_the_engine() {
        let (mut doc, ids) = doc_with(&[TrackKind::Group, TrackKind::Return, TrackKind::Audio]);
        let (group, ret, audio) = (ids[0], ids[1], ids[2]);
        let master = doc.project().master.id;
        doc.set_output(audio, Output::Track(group)).unwrap();
        doc.set_send(audio, ret, Level(-6.0)).unwrap();
        doc.set_send_pre_fader(audio, ret, true).unwrap();
        doc.set_invert(&[audio], true);
        doc.set_trim(audio, 6.0);
        doc.set_pan_law(PanLaw::Linear);
        doc.set_output(master, Output::Device(1)).unwrap();
        let routing = routing(doc.project());
        lumen_engine::compile_routing(&routing).unwrap();
        let track = node(&routing, audio);
        assert_eq!(track.output, Destination::Node(group.0));
        assert_eq!(track.sends.len(), 1);
        let send = track.sends[0];
        assert_eq!((send.to, send.pre_fader), (ret.0, true));
        assert!((send.gain - 10f32.powf(-6.0 / 20.0)).abs() < 1e-6);
        assert!(track.invert);
        assert!((track.trim - 10f32.powf(6.0 / 20.0)).abs() < 1e-5);
        assert_eq!(track.law, PanLaw::Linear);
        assert_eq!(node(&routing, ret).output, Destination::Node(master.0));
        assert_eq!(node(&routing, master).output, Destination::Hardware(1));
    }

    #[test]
    fn a_track_in_a_group_plays_into_the_group() {
        let (mut doc, ids) = doc_with(&[TrackKind::Group, TrackKind::Audio, TrackKind::Audio]);
        doc.apply("group", None, |p| {
            Arc::make_mut(&mut p.tracks[1]).parent = Some(ids[0]);
        });
        doc.set_output(ids[2], Output::Master).unwrap();
        let master = doc.project().master.id;
        let routing = routing(doc.project());
        lumen_engine::compile_routing(&routing).unwrap();
        assert_eq!(node(&routing, ids[0]).output, Destination::Node(master.0));
        assert_eq!(node(&routing, ids[1]).output, Destination::Node(ids[0].0));
        assert_eq!(node(&routing, ids[2]).output, Destination::Node(master.0));
    }

    #[test]
    fn a_silent_send_stays_in_the_routing_at_zero_gain() {
        let (mut doc, ids) = doc_with(&[TrackKind::Return, TrackKind::Audio]);
        doc.set_send(ids[1], ids[0], Level::SILENT).unwrap();
        let routing = routing(doc.project());
        assert_eq!(node(&routing, ids[1]).sends[0].gain, 0.0);
    }
}
