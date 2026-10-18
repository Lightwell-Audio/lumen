//! The engine's routing derived from the project (M4 slices 1 and 2,
//! ADR-0035): one node per track and one for the master, with each track's
//! input trim, polarity, volume, pan, whether it can be heard, its sends
//! and its output (`lumen_project::routing::destination`: its group, the
//! master, a group or return, or a device pair). Kept pure, so the
//! translation is tested, as `lanes::engine_clips` is.

use std::collections::{BTreeMap, BTreeSet};

use lumen_core::pan::PanLaw;
use lumen_engine::{CueOut, Destination, Device, NodeSpec, Routing, SendSpec};
use lumen_project::model::{Id, Output, Project, Track, TrackKind};
use lumen_project::routing::{self as rules, destination};

/// What is cued and how loud (M4 slice 4, the owner's C1), and where the
/// device's outputs are open (slice 5): session state, not part of the
/// project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cue {
    /// The master's switch is on Cue and a cue pair is open.
    pub on: bool,
    pub volume_db: f32,
    /// The tracks cued with their headphone buttons.
    pub tracks: BTreeSet<Id>,
    /// Where the device's output pairs sit in the stream.
    pub outputs: Outputs,
}

/// Where the device's output pairs sit among the stream's channels (M4
/// slice 5, the owner's B6): the main pair first, the cue pair next, then
/// every other pair something plays to. A track sent to a device pair that
/// is not open (yet) is silent until it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outputs {
    /// The main pair's device pair (0 is outputs 1 and 2).
    pub main: Option<u16>,
    pub cue: Option<u16>,
    /// Further device pairs, in the order they follow.
    pub extra: Vec<u16>,
}

impl Outputs {
    /// The stream pair device pair `pair` plays on, when it is open: the
    /// main pair is 0, the cue pair 1, further ones after them.
    pub fn stream_pair(&self, pair: u16) -> Option<u16> {
        if self.main == Some(pair) {
            return Some(0);
        }
        if self.cue == Some(pair) {
            return Some(1);
        }
        let after = 1 + u16::from(self.cue.is_some());
        self.extra
            .iter()
            .position(|&p| p == pair)
            .map(|k| after + k as u16)
    }
}

/// The device pairs `project` sends tracks (or the master) to.
pub fn device_pairs(project: &Project) -> BTreeSet<u16> {
    rules::all_tracks(project)
        .filter_map(|t| match t.output {
            Output::Device(pair) => Some(pair),
            _ => None,
        })
        .collect()
}

/// The cue pair's first channel as the engine sees it: after the main
/// pair (the owner's O1).
const CUE_CHANNEL: u16 = crate::settings::CUE_CHANNEL;

/// What the session knows of devices on tracks that the project does not
/// say: the engine's synthetic latent device, which end-to-end tests put on
/// tracks to see delay compensation at work (`testing`, M4 slice 6), and the
/// latency of each plugin effect (M6 slice 2c).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Devices {
    /// Frames of latency on each track that has some.
    pub latent: BTreeMap<Id, u32>,
    /// Frames of latency of each plugin, by device: what it last
    /// reported, or what the project saved while it loads or is missing.
    pub hosted: BTreeMap<Id, u32>,
}

/// Whether `track` plays mono material: an audio track whose clips are all
/// mono files. Its pan follows the project's pan law; every other track
/// balances (unity at the centre).
fn plays_mono(project: &Project, track: &Track) -> bool {
    use lumen_project::model::ClipContent;
    if track.kind != TrackKind::Audio || track.clips.is_empty() {
        return false;
    }
    track.clips.iter().all(|clip| match clip.content {
        ClipContent::Audio { media, .. } => project
            .media
            .iter()
            .find(|m| m.id == media)
            .is_some_and(|m| m.channels == 1),
        ClipContent::Midi { .. } => false,
    })
}

/// The routing that plays `project`, with `cue` and `devices`.
pub fn routing(project: &Project, cue: &Cue, devices: &Devices) -> Routing {
    let heard = lumen_project::solo::hearing(project);
    let nodes = rules::all_tracks(project)
        .map(|track| {
            // Only mono material pans by the project's law; stereo keeps
            // its level at the centre, as in Ableton, Logic and Pro Tools
            // (the owner's B, after M4 slice 7: a −3 dB centre on stereo
            // tracks made every file play and measure 3 dB under its own
            // level). Buses (groups, returns and the master) carry sound
            // their tracks have already panned (owner, after MT-MIX-3 run
            // 1), so they balance too.
            let law = if plays_mono(project, track) {
                project.pan_law
            } else {
                PanLaw::Balance
            };
            NodeSpec {
                trim: db_to_gain(track.trim_db),
                invert: track.invert,
                volume: db_to_gain(track.gain_db),
                pan: track.pan,
                law,
                audible: heard.of(track.id).output,
                sends_audible: heard.of(track.id).sends,
                cue: cue.on && cue.tracks.contains(&track.id),
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
                devices: devices
                    .latent
                    .get(&track.id)
                    .map(|&frames| Device::Delay {
                        frames,
                        listen: false,
                    })
                    .into_iter()
                    // A plugin effect is named for as long as the project
                    // holds it: the engine plays the host where one is
                    // attached and the same time without it (D1, D7).
                    .chain(
                        track
                            .devices
                            .iter()
                            .filter(|d| d.is_vst3() && !d.is_instrument())
                            .map(|d| Device::Hosted {
                                host: d.id.0,
                                latency: devices.hosted.get(&d.id).copied().unwrap_or(0),
                            }),
                    )
                    .collect(),
                // A plugin instrument plays its notes as late as it last
                // reported, and every other path waits for it (slice 6).
                source: track
                    .devices
                    .iter()
                    .filter(|d| d.is_vst3() && d.is_instrument())
                    .filter_map(|d| devices.hosted.get(&d.id).copied())
                    .max()
                    .unwrap_or(0),
                ..NodeSpec::unity(track.id.0, output(project, track, &cue.outputs))
            }
        })
        .collect();
    Routing {
        nodes,
        cue: cue.on.then(|| CueOut {
            channel: CUE_CHANNEL,
            volume: db_to_gain(cue.volume_db),
        }),
        // The loudness readout measures what the master plays (slice 5).
        measure: Some(project.master.id.0),
    }
}

fn output(project: &Project, track: &Track, outputs: &Outputs) -> Destination {
    // The master's Auto is the main pair, whichever device pair that is.
    if track.kind == TrackKind::Master && track.output == Output::Auto {
        return Destination::Hardware(0);
    }
    match destination(project, track) {
        rules::Destination::Track(id) => Destination::Node(id.0),
        rules::Destination::Device(pair) => outputs
            .stream_pair(pair)
            .map_or(Destination::None, Destination::Hardware),
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

    fn routing(project: &Project, cue: &Cue) -> Routing {
        super::routing(project, cue, &Devices::default())
    }

    #[test]
    fn a_latent_device_reaches_the_engine_and_is_compensated() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Audio]);
        doc.set_volume(ids[0], 0.0);
        let mut devices = Devices::default();
        devices.latent.insert(ids[0], 240);
        let routing = super::routing(doc.project(), &Cue::default(), &devices);
        assert_eq!(
            node(&routing, ids[0]).devices,
            [Device::Delay {
                frames: 240,
                listen: false
            }]
        );
        assert!(node(&routing, ids[1]).devices.is_empty());
        let plan = lumen_engine::compile_routing(&routing).unwrap();
        assert_eq!(plan.compensation().frames, 240);
        assert_eq!(plan.compensation().path, [(ids[0].0, 240)]);
    }

    #[test]
    fn a_plugin_instruments_latency_makes_every_other_track_wait_for_it() {
        use lumen_project::model::{DeviceKind, PluginRef};
        let (mut doc, ids) = doc_with(&[TrackKind::Midi, TrackKind::Audio]);
        let instrument = doc
            .add_device(
                ids[0],
                PluginRef::Vst3 {
                    class_id: "00112233445566778899AABBCCDDEEFF".into(),
                    name: "Instrument A".into(),
                    vendor: "Vendor A".into(),
                    kind: DeviceKind::Instrument,
                },
                crate::document::Slot::End,
            )
            .unwrap();
        let mut devices = Devices::default();
        devices.hosted.insert(instrument, 360);
        let routing = super::routing(doc.project(), &Cue::default(), &devices);
        assert_eq!(node(&routing, ids[0]).source, 360);
        assert!(
            node(&routing, ids[0]).devices.is_empty(),
            "an instrument is not a device in the chain"
        );
        assert_eq!(node(&routing, ids[1]).source, 0);
        let plan = lumen_engine::compile_routing(&routing).unwrap();
        assert_eq!(plan.compensation().frames, 360);
        assert_eq!(plan.compensation().path, [(ids[0].0, 360)]);
    }

    #[test]
    fn a_latent_device_on_a_return_is_compensated() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Return]);
        doc.set_send(ids[0], ids[1], Level(0.0)).unwrap();
        let mut devices = Devices::default();
        devices.latent.insert(ids[1], 480);
        let routing = super::routing(doc.project(), &Cue::default(), &devices);
        let plan = lumen_engine::compile_routing(&routing).unwrap();
        assert_eq!(plan.compensation().frames, 480);
    }

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
        let routing = routing(doc.project(), &Cue::default());
        lumen_engine::compile_routing(&routing).unwrap();
        assert_eq!(node(&routing, master).output, Destination::Hardware(0));
        assert_eq!(node(&routing, master).law, PanLaw::Balance);
        let track = node(&routing, ids[0]);
        assert_eq!(track.output, Destination::Node(master.0));
        assert!((track.volume - 10f32.powf(-6.0 / 20.0)).abs() < 1e-6);
        assert_eq!((track.pan, track.law), (-0.5, PanLaw::Balance));
        assert!(track.audible);
    }

    #[test]
    fn mute_and_solo_decide_what_is_heard() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio; 3]);
        let heard = |doc: &Document| {
            let routing = routing(doc.project(), &Cue::default());
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

    /// Puts a clip of a file with `channels` on track `id`.
    fn with_file(doc: &mut Document, id: Id, channels: u16) {
        let media = doc.fresh_id();
        doc.import(vec![crate::document::Imported {
            media,
            path: format!("C:/audio/{}.wav", media.0).into(),
            name: "file".into(),
            sample_rate: 48_000,
            channels,
            frames: 48_000,
            size_bytes: 192_044,
            at: lumen_core::time::Ticks::ZERO,
            into: crate::document::DropInto::Track(id),
        }]);
    }

    #[test]
    fn only_mono_tracks_follow_the_pan_law_and_stereo_keeps_its_level() {
        // The owner's B (after M4 slice 7): a −3 dB centre on stereo
        // tracks played every file 3 dB under its own level.
        let (mut doc, ids) = doc_with(&[TrackKind::Audio; 4]);
        with_file(&mut doc, ids[0], 1);
        with_file(&mut doc, ids[1], 2);
        with_file(&mut doc, ids[2], 1);
        with_file(&mut doc, ids[2], 2);
        let law = |doc: &Document, id| node(&routing(doc.project(), &Cue::default()), id).law;
        assert_eq!(law(&doc, ids[0]), PanLaw::ConstantPower, "mono");
        assert_eq!(law(&doc, ids[1]), PanLaw::Balance, "stereo");
        assert_eq!(law(&doc, ids[2]), PanLaw::Balance, "mono and stereo");
        assert_eq!(law(&doc, ids[3]), PanLaw::Balance, "empty");
        doc.set_pan_law(PanLaw::Linear);
        assert_eq!(law(&doc, ids[0]), PanLaw::Linear);
        assert_eq!(law(&doc, ids[1]), PanLaw::Balance);
    }

    #[test]
    fn buses_are_unity_when_centred() {
        // Owner, after MT-MIX-3 run 1: tracks in groups played quieter.
        let (doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Group, TrackKind::Return]);
        let routing = routing(doc.project(), &Cue::default());
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
        let heard = |doc: &Document, id| node(&routing(doc.project(), &Cue::default()), id).audible;
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
    fn cued_tracks_reach_the_engine_only_while_the_cue_is_on() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Audio]);
        doc.set_mute(&ids[..1], true);
        let mut cue = Cue {
            on: true,
            volume_db: -6.0,
            tracks: [ids[0]].into(),
            outputs: Outputs::default(),
        };
        let cued = routing(doc.project(), &cue);
        let out = cued.cue.expect("a cue bus");
        assert_eq!(out.channel, 2, "after the main pair");
        assert!((out.volume - db_to_gain(-6.0)).abs() < 1e-6);
        assert!(node(&cued, ids[0]).cue, "cued, muted or not");
        assert!(!node(&cued, ids[1]).cue);
        lumen_engine::compile_routing(&cued).unwrap();
        cue.on = false;
        let off = routing(doc.project(), &cue);
        assert!(off.cue.is_none() && !node(&off, ids[0]).cue);
    }

    #[test]
    fn a_soloed_return_reaches_the_engine_as_wet_only() {
        // M4 slice 4, case 8: the sends into a soloed return play, the
        // sending tracks' own outputs do not.
        let (mut doc, ids) = doc_with(&[TrackKind::Return, TrackKind::Audio]);
        doc.set_send(ids[1], ids[0], Level(-6.0)).unwrap();
        doc.set_solo(&ids[..1], true, true);
        let wet = routing(doc.project(), &Cue::default());
        let track = node(&wet, ids[1]);
        assert!(!track.audible && track.sends_audible);
        let ret = node(&wet, ids[0]);
        assert!(ret.audible && ret.sends_audible);
        doc.set_mute(&ids[1..], true);
        let track = node(&routing(doc.project(), &Cue::default()), ids[1]).clone();
        assert!(
            !track.audible && !track.sends_audible,
            "mute silences sends"
        );
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
        // Outputs 3 and 4 open after the main pair.
        let open = Cue {
            outputs: Outputs {
                main: Some(0),
                cue: None,
                extra: vec![1],
            },
            ..Cue::default()
        };
        let routing = routing(doc.project(), &open);
        lumen_engine::compile_routing(&routing).unwrap();
        let track = node(&routing, audio);
        assert_eq!(track.output, Destination::Node(group.0));
        assert_eq!(track.sends.len(), 1);
        let send = track.sends[0];
        assert_eq!((send.to, send.pre_fader), (ret.0, true));
        assert!((send.gain - 10f32.powf(-6.0 / 20.0)).abs() < 1e-6);
        assert!(track.invert);
        assert!((track.trim - 10f32.powf(6.0 / 20.0)).abs() < 1e-5);
        assert_eq!(track.law, PanLaw::Balance, "an empty track is not mono");
        assert_eq!(node(&routing, ret).output, Destination::Node(master.0));
        assert_eq!(node(&routing, master).output, Destination::Hardware(1));
    }

    #[test]
    fn device_pairs_play_where_the_stream_opened_them() {
        // The owner's B6: main, then cue, then further pairs in order.
        let outputs = Outputs {
            main: Some(0),
            cue: Some(1),
            extra: vec![3, 2],
        };
        assert_eq!(outputs.stream_pair(0), Some(0));
        assert_eq!(outputs.stream_pair(1), Some(1), "the cue pair");
        assert_eq!(outputs.stream_pair(3), Some(2));
        assert_eq!(outputs.stream_pair(2), Some(3));
        assert_eq!(outputs.stream_pair(5), None, "not open");
        let no_cue = Outputs {
            main: Some(2),
            cue: None,
            extra: vec![0],
        };
        assert_eq!(no_cue.stream_pair(2), Some(0), "main on outputs 5/6");
        assert_eq!(no_cue.stream_pair(0), Some(1));

        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Audio]);
        doc.set_output(ids[0], Output::Device(3)).unwrap();
        doc.set_output(ids[1], Output::Device(5)).unwrap();
        assert_eq!(device_pairs(doc.project()), [3, 5].into());
        let routing = routing(
            doc.project(),
            &Cue {
                outputs,
                ..Cue::default()
            },
        );
        assert_eq!(node(&routing, ids[0]).output, Destination::Hardware(2));
        assert_eq!(
            node(&routing, ids[1]).output,
            Destination::None,
            "silent until open"
        );
        // The master's Auto is the main pair, whichever it is.
        let master = doc.project().master.id;
        assert_eq!(node(&routing, master).output, Destination::Hardware(0));
    }

    #[test]
    fn a_track_in_a_group_plays_into_the_group() {
        let (mut doc, ids) = doc_with(&[TrackKind::Group, TrackKind::Audio, TrackKind::Audio]);
        doc.apply("group", None, |p| {
            Arc::make_mut(&mut p.tracks[1]).parent = Some(ids[0]);
        });
        doc.set_output(ids[2], Output::Master).unwrap();
        let master = doc.project().master.id;
        let routing = routing(doc.project(), &Cue::default());
        lumen_engine::compile_routing(&routing).unwrap();
        assert_eq!(node(&routing, ids[0]).output, Destination::Node(master.0));
        assert_eq!(node(&routing, ids[1]).output, Destination::Node(ids[0].0));
        assert_eq!(node(&routing, ids[2]).output, Destination::Node(master.0));
    }

    #[test]
    fn a_silent_send_stays_in_the_routing_at_zero_gain() {
        let (mut doc, ids) = doc_with(&[TrackKind::Return, TrackKind::Audio]);
        doc.set_send(ids[1], ids[0], Level::SILENT).unwrap();
        let routing = routing(doc.project(), &Cue::default());
        assert_eq!(node(&routing, ids[1]).sends[0].gain, 0.0);
    }
}
