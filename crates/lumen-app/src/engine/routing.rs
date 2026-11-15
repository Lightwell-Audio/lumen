use std::collections::{BTreeMap, BTreeSet};

use lumen_core::pan::PanLaw;
use lumen_core::time::TempoMap;
use lumen_engine::{
    CueOut, Destination, Device, NodeAutomation, NodeSpec, Routing, SendSpec, SidechainSpec, Tap,
};
use lumen_project::automation::Target;

use crate::document::Overrides;
use crate::lane_bake::{self, bake};
use lumen_project::model::{Id, Output, Project, SidechainTap, Track, TrackKind};
use lumen_project::routing::{self as rules, destination};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cue {
    pub on: bool,
    pub volume_db: f32,
    pub tracks: BTreeSet<Id>,
    pub outputs: Outputs,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outputs {
    pub main: Option<u16>,
    pub cue: Option<u16>,
    pub extra: Vec<u16>,
}

impl Outputs {
    fn stream_pair(&self, pair: u16) -> Option<u16> {
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

pub fn device_pairs(project: &Project) -> BTreeSet<u16> {
    rules::all_tracks(project)
        .filter_map(|t| match t.output {
            Output::Device(pair) => Some(pair),
            _ => None,
        })
        .collect()
}

const CUE_CHANNEL: u16 = crate::settings::CUE_CHANNEL;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Devices {
    pub latent: BTreeMap<Id, u32>,
    pub hosted: BTreeMap<Id, u32>,
    pub builtin: BTreeMap<Id, u32>,
    pub overridden: Overrides,
}

fn plays_mono(project: &Project, track: &Track) -> bool {
    track.plays_mono(&project.media)
}

fn automation(
    track: &Track,
    over: &Overrides,
    tempo: Option<&TempoMap>,
    rate: u32,
) -> Option<NodeAutomation> {
    let tempo = tempo?;
    let on = |target| {
        track
            .lane(target)
            .filter(|l| over.plays(track, l) && track.has_target(target))
            .and_then(|l| bake(&l.lane, tempo, rate))
    };
    let lanes = NodeAutomation {
        trim: on(Target::Trim),
        mute: on(Target::Mute),
        volume: on(Target::Volume),
        pan: on(Target::Pan),
        sends: track
            .sends
            .iter()
            .map(|send| on(Target::Send { to: send.to }))
            .collect(),
        plugins: lane_bake::device_lanes(track, over, tempo, rate, |d| {
            (d.is_vst3() || crate::builtin::is_builtin(d)) && !d.is_instrument()
        })
        .into_iter()
        .map(|(device, lane)| (device.0, lane))
        .collect(),
    };
    (!lanes.is_empty()).then_some(lanes)
}

fn mute_lane(track: &Track, over: &Overrides) -> bool {
    track
        .lane(Target::Mute)
        .is_some_and(|l| over.plays(track, l))
}

pub fn routing(project: &Project, cue: &Cue, devices: &Devices, rate: u32) -> Routing {
    let tempo = TempoMap::new(project.tempo.clone()).ok();
    let unmuted;
    let rules_project =
        if rules::all_tracks(project).any(|t| t.mute && mute_lane(t, &devices.overridden)) {
            let mut copy = project.clone();
            for track in copy
                .tracks
                .iter_mut()
                .chain(std::iter::once(&mut copy.master))
            {
                if track.mute && mute_lane(track, &devices.overridden) {
                    std::sync::Arc::make_mut(track).mute = false;
                }
            }
            unmuted = copy;
            &unmuted
        } else {
            project
        };
    let heard = lumen_project::solo::hearing(rules_project);
    let builtin_values = crate::builtin::routing_values(project);
    let nodes = rules::all_tracks(project)
        .map(|track| {
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
                    .chain(track.devices.iter().filter_map(|d| {
                        if d.is_vst3() && !d.is_instrument() {
                            return Some(Device::Hosted {
                                host: d.id.0,
                                latency: devices.hosted.get(&d.id).copied().unwrap_or(0),
                            });
                        }
                        (crate::builtin::is_builtin(d) && !d.is_instrument()).then(|| {
                            Device::Builtin {
                                device: d.id.0,
                                latency: devices.builtin.get(&d.id).copied().unwrap_or(0),
                                bypass: d.bypass,
                            }
                        })
                    }))
                    .collect(),
                values: builtin_values
                    .iter()
                    .filter(|(device, _)| track.devices.iter().any(|d| d.id.0 == *device))
                    .cloned()
                    .collect(),
                source: track
                    .devices
                    .iter()
                    .filter(|d| d.is_vst3() && d.is_instrument())
                    .map(|d| d.id)
                    .chain(track.plugin_output.map(|o| o.device))
                    .filter_map(|d| devices.hosted.get(&d).copied())
                    .max()
                    .unwrap_or(0),
                sidechains: sidechains(project, track),
                automation: automation(track, &devices.overridden, tempo.as_ref(), rate),
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
        measure: Some(project.master.id.0),
    }
}

pub fn bounce_routing(
    project: &Project,
    track: lumen_project::model::Id,
    devices: &Devices,
    rate: u32,
) -> Option<Routing> {
    let mut full = routing(project, &Cue::default(), devices, rate);
    let mut node = full.nodes.drain(..).find(|n| n.id == track.0)?;
    let master = project.master.id.0;
    node.audible = true;
    node.sends_audible = false;
    node.cue = false;
    node.output = Destination::Node(master);
    node.sends.clear();
    node.sidechains.clear();
    if let Some(lanes) = &mut node.automation {
        lanes.sends.clear();
    }
    Some(Routing {
        nodes: vec![NodeSpec::unity(master, Destination::Hardware(0)), node],
        cue: None,
        measure: None,
    })
}

fn sidechains(project: &Project, track: &Track) -> Vec<SidechainSpec> {
    let mut found: Vec<SidechainSpec> = Vec::new();
    for key in track.devices.iter().filter_map(|d| d.sidechain) {
        if !rules::all_tracks(project).any(|t| t.id == key.from) {
            continue;
        }
        let spec = SidechainSpec {
            from: key.from.0,
            tap: match key.tap {
                SidechainTap::PreFx => Tap::PreFx,
                SidechainTap::PostFx => Tap::PostFx,
                SidechainTap::PostMixer => Tap::PostMixer,
            },
        };
        if !found.contains(&spec) {
            found.push(spec);
        }
    }
    found
}

fn output(project: &Project, track: &Track, outputs: &Outputs) -> Destination {
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

    const RATE: u32 = 48_000;

    fn routing(project: &Project, cue: &Cue) -> Routing {
        super::routing(project, cue, &Devices::default(), RATE)
    }

    #[test]
    fn a_latent_device_reaches_the_engine_and_is_compensated() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Audio]);
        doc.set_volume(ids[0], 0.0);
        let mut devices = Devices::default();
        devices.latent.insert(ids[0], 240);
        let routing = super::routing(doc.project(), &Cue::default(), &devices, RATE);
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
        let routing = super::routing(doc.project(), &Cue::default(), &devices, RATE);
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
    fn a_plugin_effects_sidechain_reaches_the_engine_once_for_each_source_and_tap() {
        use lumen_project::model::{DeviceKind, PluginRef, Sidechain};
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Audio]);
        let effect = |doc: &mut Document, name: &str| {
            doc.add_device(
                ids[0],
                PluginRef::Vst3 {
                    class_id: "00112233445566778899AABBCCDDEEFF".into(),
                    name: name.into(),
                    vendor: "Vendor A".into(),
                    kind: DeviceKind::Effect,
                },
                crate::document::Slot::End,
            )
            .unwrap()
        };
        let (a, b) = (effect(&mut doc, "Comp"), effect(&mut doc, "Gate"));
        let key = |tap| Some(Sidechain { from: ids[1], tap });
        doc.set_sidechain(a, key(SidechainTap::PostFx)).unwrap();
        doc.set_sidechain(b, key(SidechainTap::PostFx)).unwrap();
        let routing = routing(doc.project(), &Cue::default());
        assert_eq!(
            node(&routing, ids[0]).sidechains,
            [SidechainSpec {
                from: ids[1].0,
                tap: Tap::PostFx
            }],
            "two effects keyed alike share one feed"
        );
        lumen_engine::compile_routing(&routing).unwrap();
        doc.set_sidechain(b, key(SidechainTap::PreFx)).unwrap();
        let routing = super::routing(doc.project(), &Cue::default(), &Devices::default(), RATE);
        assert_eq!(node(&routing, ids[0]).sidechains.len(), 2);
        doc.set_sidechain(
            a,
            Some(Sidechain {
                from: Id(9_999),
                tap: SidechainTap::PostFx,
            }),
        )
        .unwrap();
        let routing = super::routing(doc.project(), &Cue::default(), &Devices::default(), RATE);
        assert_eq!(node(&routing, ids[0]).sidechains.len(), 1);
        lumen_engine::compile_routing(&routing).unwrap();
    }

    fn with_plugin(
        doc: &mut Document,
        track: Id,
        kind: lumen_project::model::DeviceKind,
        name: &str,
    ) -> Id {
        doc.add_device(
            track,
            lumen_project::model::PluginRef::Vst3 {
                class_id: "00112233445566778899AABBCCDDEEFF".into(),
                name: name.into(),
                vendor: "Vendor A".into(),
                kind,
            },
            crate::document::Slot::End,
        )
        .unwrap()
    }

    #[test]
    fn a_plugin_effects_parameter_lane_reaches_the_engine_with_its_host_on_the_samples_the_tempo_gives()
     {
        use lumen_project::model::DeviceKind;
        let (mut doc, ids) = doc_with(&[TrackKind::Audio]);
        let effect = with_plugin(&mut doc, ids[0], DeviceKind::Effect, "Filter");
        let target = Target::Device {
            device: effect,
            param: 9,
        };
        doc.set_lane(ids[0], target, lane(&[(0, 0.25), (4, 0.75)]), false)
            .unwrap();
        let routing = super::routing(doc.project(), &Cue::default(), &Devices::default(), RATE);
        let lanes = node(&routing, ids[0]).automation.as_ref().expect("lanes");
        assert_eq!(lanes.plugins.len(), 1);
        let (host, lane) = &lanes.plugins[0];
        assert_eq!(*host, effect.0, "named by the device's id, as the host is");
        assert_eq!(lane.param, 9);
        assert_eq!(lane.lane.points()[1].at, 2 * i64::from(RATE));
        lumen_engine::compile_routing(&routing).unwrap();
        doc.set_lane_enabled(ids[0], target, false).unwrap();
        let routing = super::routing(doc.project(), &Cue::default(), &Devices::default(), RATE);
        assert!(node(&routing, ids[0]).automation.is_none());
    }

    #[test]
    fn a_plugin_instruments_lanes_are_not_the_routings() {
        use lumen_project::model::DeviceKind;
        let (mut doc, ids) = doc_with(&[TrackKind::Midi]);
        let synth = with_plugin(&mut doc, ids[0], DeviceKind::Instrument, "Synth");
        doc.set_lane(
            ids[0],
            Target::Device {
                device: synth,
                param: 1,
            },
            lane(&[(0, 0.5)]),
            false,
        )
        .unwrap();
        let routing = super::routing(doc.project(), &Cue::default(), &Devices::default(), RATE);
        assert!(
            node(&routing, ids[0]).automation.is_none(),
            "an instrument's go with the track's MIDI"
        );
    }

    #[test]
    fn a_latent_device_on_a_return_is_compensated() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Return]);
        doc.set_send(ids[0], ids[1], Level(0.0)).unwrap();
        let mut devices = Devices::default();
        devices.latent.insert(ids[1], 480);
        let routing = super::routing(doc.project(), &Cue::default(), &devices, RATE);
        let plan = lumen_engine::compile_routing(&routing).unwrap();
        assert_eq!(plan.compensation().frames, 480);
    }

    #[test]
    fn a_bounce_routes_the_track_alone_with_its_own_sound_into_a_plain_master() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Return]);
        doc.set_volume(ids[0], -6.0);
        doc.set_pan(ids[0], 0.5);
        doc.set_send(ids[0], ids[1], Level(0.0)).unwrap();
        doc.set_mute(&ids[..1], true);
        let master = doc.project().master.id;
        let routing = super::bounce_routing(doc.project(), ids[0], &Devices::default(), RATE)
            .expect("the track is there");
        assert_eq!(routing.nodes.len(), 2, "the track and the master");
        assert_eq!(routing.cue, None);
        let track = node(&routing, ids[0]);
        assert!(track.audible, "a muted track is bounced as it sounds");
        assert!(track.sends.is_empty());
        assert_eq!(track.output, Destination::Node(master.0));
        assert!(
            (track.volume - db_to_gain(-6.0)).abs() < 1e-6,
            "its fader stays"
        );
        assert_eq!(track.pan, 0.5);
        let bus = node(&routing, master);
        assert_eq!(bus.output, Destination::Hardware(0));
        assert_eq!(bus.volume, 1.0, "the master does nothing");
        lumen_engine::compile_routing(&routing).unwrap();
        assert!(
            super::bounce_routing(doc.project(), Id(9_999), &Devices::default(), RATE).is_none()
        );
    }

    #[test]
    fn the_chain_plays_in_the_order_the_card_shows() {
        use lumen_project::model::{DeviceKind, PluginRef};
        let (mut doc, ids) = doc_with(&[TrackKind::Audio]);
        let plugin = with_plugin(&mut doc, ids[0], DeviceKind::Effect, "Filter");
        let builtin = doc
            .add_device(
                ids[0],
                PluginRef::Builtin {
                    name: lumen_devices::utility::NAME.to_owned(),
                },
                crate::document::Slot::End,
            )
            .unwrap();
        let order = |doc: &Document| -> Vec<&'static str> {
            let routing = super::routing(doc.project(), &Cue::default(), &Devices::default(), RATE);
            node(&routing, ids[0])
                .devices
                .iter()
                .map(|d| match d {
                    Device::Hosted { .. } => "plugin",
                    Device::Builtin { .. } => "builtin",
                    Device::Delay { .. } => "delay",
                })
                .collect()
        };
        assert_eq!(order(&doc), ["plugin", "builtin"]);
        doc.move_device(ids[0], builtin, ids[0], crate::document::Slot::At(0))
            .unwrap();
        assert_eq!(order(&doc), ["builtin", "plugin"]);
        lumen_engine::compile_routing(&super::routing(
            doc.project(),
            &Cue::default(),
            &Devices::default(),
            RATE,
        ))
        .unwrap();
        assert_ne!(plugin, builtin);
        let mut devices = Devices::default();
        devices.latent.insert(ids[0], 12);
        let routing = super::routing(doc.project(), &Cue::default(), &devices, RATE);
        assert!(matches!(
            node(&routing, ids[0]).devices[0],
            Device::Delay { .. }
        ));
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

    fn lane(points: &[(i64, f32)]) -> lumen_core::automation::Lane {
        lumen_core::automation::Lane::new(
            points
                .iter()
                .map(|&(q, v)| {
                    lumen_core::automation::Point::new(lumen_core::time::Ticks::quarters(q), v)
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn lanes_reach_the_engine_on_the_samples_the_tempo_puts_them_at() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Return]);
        doc.set_send(ids[0], ids[1], lumen_project::model::Level(-6.0))
            .unwrap();
        doc.set_lane(ids[0], Target::Volume, lane(&[(0, -6.0), (4, 0.0)]), false)
            .unwrap();
        doc.set_lane(
            ids[0],
            Target::Send { to: ids[1] },
            lane(&[(2, -3.0)]),
            false,
        )
        .unwrap();
        let routing = super::routing(doc.project(), &Cue::default(), &Devices::default(), RATE);
        let lanes = node(&routing, ids[0]).automation.as_ref().expect("lanes");
        let at: Vec<i64> = lanes
            .volume
            .as_ref()
            .unwrap()
            .points()
            .iter()
            .map(|p| p.at)
            .collect();
        assert_eq!(at, [0, 96_000]);
        assert!(lanes.pan.is_none() && lanes.trim.is_none() && lanes.mute.is_none());
        assert_eq!(lanes.sends.len(), 1);
        assert_eq!(lanes.sends[0].as_ref().unwrap().points()[0].at, 48_000);
        let routing = super::routing(doc.project(), &Cue::default(), &Devices::default(), 44_100);
        let lanes = node(&routing, ids[0]).automation.as_ref().unwrap();
        assert_eq!(lanes.volume.as_ref().unwrap().points()[1].at, 88_200);
        assert!(node(&routing, ids[1]).automation.is_none());
        lumen_engine::compile_routing(&routing).unwrap();
    }

    #[test]
    fn a_tempo_change_moves_the_samples_a_lane_lands_on() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio]);
        doc.set_lane(ids[0], Target::Pan, lane(&[(0, -1.0), (8, 1.0)]), false)
            .unwrap();
        let last = |doc: &Document| {
            let routing = super::routing(doc.project(), &Cue::default(), &Devices::default(), RATE);
            node(&routing, ids[0])
                .automation
                .as_ref()
                .unwrap()
                .pan
                .as_ref()
                .unwrap()
                .points()
                .last()
                .unwrap()
                .at
        };
        assert_eq!(last(&doc), 192_000);
        doc.set_tempo_at(lumen_core::time::Ticks::ZERO, 240.0, None);
        assert_eq!(last(&doc), 96_000);
        doc.set_tempo_at(lumen_core::time::Ticks::ZERO, 120.0, None);
        doc.apply("tempo", None, |p| {
            p.tempo.push(lumen_core::time::TempoPoint {
                at: lumen_core::time::Ticks::quarters(4),
                bpm: 240.0,
                ramp: false,
            })
        });
        assert_eq!(last(&doc), 96_000 + 48_000);
    }

    #[test]
    fn lanes_that_are_off_or_drive_nothing_the_track_has_are_not_sent() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Return]);
        doc.set_lane(ids[0], Target::Volume, lane(&[(0, -6.0)]), false)
            .unwrap();
        doc.set_lane_enabled(ids[0], Target::Volume, false).unwrap();
        let plan = routing(doc.project(), &Cue::default());
        assert!(
            node(&plan, ids[0]).automation.is_none(),
            "a lane that is off plays nothing"
        );
        doc.set_send(ids[0], ids[1], lumen_project::model::Level(0.0))
            .unwrap();
        doc.set_lane(
            ids[0],
            Target::Send { to: ids[1] },
            lane(&[(0, -6.0)]),
            false,
        )
        .unwrap();
        doc.apply("lose the send", None, |p| {
            std::sync::Arc::make_mut(&mut p.tracks[0]).sends.clear()
        });
        let plan = routing(doc.project(), &Cue::default());
        assert!(node(&plan, ids[0]).automation.is_none());
        lumen_engine::compile_routing(&plan).unwrap();
    }

    #[test]
    fn a_lane_taken_over_plays_nothing_and_the_controls_own_value_is_heard() {
        use lumen_project::model::DeviceKind;
        let (mut doc, ids) = doc_with(&[TrackKind::Audio]);
        let effect = with_plugin(&mut doc, ids[0], DeviceKind::Effect, "Filter");
        let param = Target::Device {
            device: effect,
            param: 3,
        };
        doc.set_lane(ids[0], Target::Volume, lane(&[(0, -6.0)]), false)
            .unwrap();
        doc.set_lane(ids[0], Target::Mute, lane(&[(0, 1.0)]), false)
            .unwrap();
        doc.set_lane(ids[0], param, lane(&[(0, 0.5)]), false)
            .unwrap();
        let play = |doc: &Document| {
            let devices = Devices {
                overridden: doc.overrides().clone(),
                ..Devices::default()
            };
            super::routing(doc.project(), &Cue::default(), &devices, RATE)
        };
        let lanes = |plan: &Routing| node(plan, ids[0]).automation.clone();
        let all = lanes(&play(&doc)).expect("all three play");
        assert!(all.volume.is_some() && all.mute.is_some() && all.plugins.len() == 1);

        doc.set_volume(ids[0], -12.0);
        doc.override_lane(ids[0], param);
        let some = lanes(&play(&doc)).expect("the mute lane still plays");
        assert!(some.volume.is_none() && some.plugins.is_empty() && some.mute.is_some());
        assert_eq!(node(&play(&doc), ids[0]).volume, db_to_gain(-12.0));

        doc.set_mute(&[ids[0]], false);
        let plan = play(&doc);
        assert!(lanes(&plan).is_none(), "nothing is left to play");
        assert!(node(&plan, ids[0]).audible, "its own switch: not muted");
        doc.set_mute(&[ids[0]], true);
        assert!(!node(&play(&doc), ids[0]).audible, "its own switch: muted");
        lumen_engine::compile_routing(&play(&doc)).unwrap();

        doc.reenable_all();
        let again = lanes(&play(&doc)).expect("back");
        assert!(again.volume.is_some() && again.mute.is_some() && again.plugins.len() == 1);
    }

    #[test]
    fn a_mute_lane_not_the_tracks_own_switch_says_when_it_is_heard() {
        let (mut doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Audio]);
        doc.set_mute(&[ids[0]], true);
        let plan = routing(doc.project(), &Cue::default());
        assert!(!node(&plan, ids[0]).audible, "muted by its switch");
        doc.set_lane(ids[0], Target::Mute, lane(&[(0, 0.0), (4, 1.0)]), false)
            .unwrap();
        let plan = routing(doc.project(), &Cue::default());
        assert!(node(&plan, ids[0]).audible, "the lane decides");
        assert!(
            node(&plan, ids[0])
                .automation
                .as_ref()
                .unwrap()
                .mute
                .is_some()
        );
        doc.set_solo(&[ids[1]], true, false);
        let plan = routing(doc.project(), &Cue::default());
        assert!(!node(&plan, ids[0]).audible);
        doc.set_solo(&[ids[1]], false, false);
        doc.set_lane_enabled(ids[0], Target::Mute, false).unwrap();
        let plan = routing(doc.project(), &Cue::default());
        assert!(!node(&plan, ids[0]).audible);
    }

    #[test]
    fn the_engine_form_of_a_lane_agrees_with_the_model_everywhere() {
        agrees(false);
    }

    #[test]
    fn the_engine_form_of_a_lane_agrees_with_the_model_along_a_tempo_ramp() {
        agrees(true);
    }

    fn agrees(ramp: bool) {
        use lumen_core::automation::{Lane, Point, Shape};
        use lumen_core::curve::Curve;
        use lumen_core::time::{TempoMap, TempoPoint, Ticks};
        let tempo = TempoMap::new(vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 93.0,
                ramp,
            },
            TempoPoint {
                at: Ticks::quarters(6),
                bpm: 141.5,
                ramp: false,
            },
        ])
        .unwrap();
        let mut seed = 5u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as f32 / (1u64 << 31) as f32
        };
        for case in 0..60 {
            let mut at = 0i64;
            let points: Vec<Point> = (0..2 + case % 12)
                .map(|_| {
                    at += lumen_core::time::PPQ / 4
                        + (next() * 4.0 * lumen_core::time::PPQ as f32) as i64;
                    let shape = match (next() * 3.0) as u32 {
                        0 => Shape::Linear,
                        1 => Shape::Curve(Curve::FAST),
                        _ => Shape::Step,
                    };
                    Point::new(Ticks(at), next() * 20.0 - 14.0).with_shape(shape)
                })
                .collect();
            let model = Lane::new(points.clone()).unwrap();
            let baked = bake(&model, &tempo, RATE).unwrap();
            let end = tempo.sample_at(points.last().unwrap().at, RATE);
            for sample in (0..end + 5_000).step_by(37) {
                if baked.points().iter().any(|p| (p.at - sample).abs() < 3) {
                    continue;
                }
                let want = model.value_at(tempo.ticks_at_sample(sample, RATE)).unwrap();
                let got = baked.value_at(sample);
                assert!(
                    (want - got).abs() < 0.01,
                    "case {case} at sample {sample}: {want} against {got}"
                );
            }
        }
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
            tempo: None,
            at: lumen_core::time::Ticks::ZERO,
            into: crate::document::DropInto::Track(id),
        }]);
    }

    #[test]
    fn only_mono_tracks_follow_the_pan_law_and_stereo_keeps_its_level() {
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
        let (doc, ids) = doc_with(&[TrackKind::Audio, TrackKind::Group, TrackKind::Return]);
        let routing = routing(doc.project(), &Cue::default());
        for bus in [ids[1], ids[2], doc.project().master.id] {
            assert_eq!(node(&routing, bus).law, PanLaw::Balance);
            assert_eq!(PanLaw::Balance.gains(0.0), [1.0, 1.0]);
        }
    }

    #[test]
    fn solo_keeps_the_path_through_groups_open() {
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
