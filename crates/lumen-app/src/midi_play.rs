//! The engine's MIDI tracks derived from the project (M5 slice 1, ADR-0045):
//! each MIDI track's clips as notes and controller messages in ticks, with
//! loop repeats, where they go (the track's instrument, or a MIDI output
//! port) and which instrument plays them. Pure, like `lanes::engine_clips`
//! and `routing::routing`, so the translation is tested.
//!
//! Clips that meet at a seam play as one run, so a note held across the
//! seam sustains (`midi_runs`, ADR-0058). Runs that did not change keep
//! their note lists (shared, so the engine's comparison is a pointer check
//! and a 100,000-note clip is not copied on every edit elsewhere).

use lumen_core::time::Ticks;
use lumen_devices::{Preset, Synth, synth};
use lumen_engine::{INPUT_SLOTS, InstrumentSpec, KEYBOARD, Listen, MidiTrack, Target};
use lumen_project::model::{self, ClipContent, FromSource, Id, Monitor, Project, TrackKind};

pub use crate::midi_runs::Cache;

/// The MIDI outputs the engine's port numbers stand for, by name. Names are
/// only ever added, so a number keeps its port for the whole run; the
/// sender looks them up when it sends.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PortTable {
    names: Vec<String>,
}

impl PortTable {
    /// The number for `name`, adding it the first time.
    pub fn index(&mut self, name: &str) -> u16 {
        let at = self
            .names
            .iter()
            .position(|n| n == name)
            .unwrap_or_else(|| {
                self.names.push(name.to_owned());
                self.names.len() - 1
            });
        at.min(usize::from(u16::MAX)) as u16
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }
}

/// The names of the MIDI outputs the project's tracks send to.
pub fn used_ports(project: &Project) -> std::collections::BTreeSet<String> {
    project
        .tracks
        .iter()
        .filter_map(|t| t.midi_to.as_ref().map(|o| o.port.clone()))
        .collect()
}

/// The instrument a track's synth device asks for. A preset this version
/// does not know (a project from a later one) plays as the default sound.
fn instrument(track: &model::Track, hosts: &mut dyn Hosts) -> Option<InstrumentSpec> {
    // A plugin instrument: a host's, or a stand-in while there is none.
    if let Some(device) = track.sound_source()
        && device.is_vst3()
    {
        return Some(hosts.instrument(track.id));
    }
    let device = track.instrument()?;
    let preset = device
        .preset()
        .and_then(Preset::from_id)
        .unwrap_or_default();
    Some(InstrumentSpec {
        kind: synth::KIND,
        preset: preset.number(),
        instrument: Box::new(Synth::new(preset)),
    })
}

/// Where a track's plugin instrument comes from (M6 slice 2c).
pub trait Hosts {
    /// The instrument slot for the plugin on MIDI track `track`.
    fn instrument(&mut self, track: Id) -> InstrumentSpec;

    /// The auxiliary output buses of the plugin instrument `device` that its
    /// running host has on, one bit for each bus (bit 3 is the fourth). None
    /// where it has no host.
    fn outputs_on(&self, _device: Id) -> u32 {
        0
    }
}

impl Hosts for crate::hosting::Hosting {
    fn instrument(&mut self, track: Id) -> InstrumentSpec {
        crate::hosting::Hosting::instrument(self, track)
    }

    fn outputs_on(&self, device: Id) -> u32 {
        crate::hosting::Hosting::outputs_on(self, device)
    }
}

/// No hosts: every plugin instrument is a stand-in that plays nothing.
#[cfg(test)]
pub struct NoHosts;

#[cfg(test)]
impl Hosts for NoHosts {
    fn instrument(&mut self, _: Id) -> InstrumentSpec {
        crate::hosting::stand_in()
    }
}

/// What decides which tracks play what is played live (M5 slice 2): the
/// selected tracks and the inputs switched on for tracks. Compared each
/// frame, so the engine's list is sent again only when one changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inputs {
    /// The selected tracks.
    pub selected: Vec<Id>,
    /// Each input switched on for tracks: its id name and its slot.
    pub ports: Vec<(String, u16)>,
    /// Tracks being recorded over (Replace): what they hold from the
    /// take's start on is not played.
    pub replacing: Vec<Replacing>,
    /// Where the recording under way shows what is played (start and end
    /// ticks): only there do keys played win their pitch in the engine.
    pub record_window: Option<(Ticks, Ticks)>,
    /// The take's original loop, punch and latency boundaries.
    pub record_timing: Option<lumen_engine::RecordTiming>,
}

/// A track a Replace take is being recorded on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacing {
    pub track: Id,
    /// Where the take begins: earlier material is masked until `until`.
    pub from: Ticks,
    /// Where punch-out ends the Replace window. `None` means recording has
    /// no known end yet, so old material stays silent after `from`.
    pub until: Option<Ticks>,
    /// The take's own clip, which plays on (loop passes hear each other).
    pub take: Option<Id>,
}

impl Inputs {
    /// The part of `clip` on `track` that plays: all of it, unless a Replace
    /// take is over its end, which cuts it short or silences it. `None` when
    /// nothing of it plays.
    fn playing_length(&self, track: Id, clip: &model::Clip) -> Option<Ticks> {
        let cut = self
            .replacing
            .iter()
            .find(|r| r.track == track && r.take != Some(clip.id))
            .filter(|r| r.until.is_none())
            .map_or(clip.length, |r| clip.length.min(r.from - clip.start));
        (cut.0 > 0).then_some(cut)
    }

    /// A bounded Replace window masks notes inside the recording interval,
    /// while retaining the clip on both sides. The take remains audible.
    fn suppression(&self, track: Id) -> Option<crate::midi_runs::Suppression> {
        self.replacing
            .iter()
            .find(|r| r.track == track)
            .and_then(|r| {
                r.until.map(|until| crate::midi_runs::Suppression {
                    from: r.from,
                    until,
                    exempt: r.take,
                })
            })
    }
}

impl Inputs {
    /// The bit of the input called `name`, if it is present and switched
    /// on for tracks (and its slot fits a mask).
    fn bit(&self, name: &str) -> u64 {
        self.ports
            .iter()
            .find(|(n, _)| n == name)
            .map_or(0, |&(_, slot)| slot_bit(slot))
    }
}

fn slot_bit(slot: u16) -> u64 {
    if slot < u16::from(INPUT_SLOTS) {
        1 << slot
    } else {
        0
    }
}

/// What a MIDI track plays of the live input. It listens when its monitor is
/// In, or Auto while it is armed or selected (the owner's L A); never when
/// Off. A track sent to a MIDI output that is muted or silenced by a solo
/// sends nothing, so it does not listen (`silent`).
pub fn listening(track: &model::Track, inputs: &Inputs, silent: bool) -> Listen {
    let on = match track.monitor {
        Monitor::In => true,
        Monitor::Auto => track.armed || inputs.selected.contains(&track.id),
        Monitor::Off => false,
    };
    if !on || silent {
        return Listen::OFF;
    }
    Listen {
        sources: from_sources(track, inputs),
        channel: track.midi_from.channel,
    }
}

/// What an armed MIDI track keeps of the live input while the transport
/// records (M5 slice 6): what its MIDI From takes, whether or not it listens
/// (a track with its monitor Off still records). Nothing when not armed.
pub fn recording(track: &model::Track, inputs: &Inputs) -> Listen {
    if !track.armed {
        return Listen::OFF;
    }
    Listen {
        sources: from_sources(track, inputs),
        channel: track.midi_from.channel,
    }
}

/// The sources a track's MIDI From takes, whether or not it listens now
/// (the header light counts their traffic, so its memory does not jump
/// when the track starts to listen).
pub fn from_sources(track: &model::Track, inputs: &Inputs) -> u64 {
    match &track.midi_from.source {
        FromSource::All => {
            inputs
                .ports
                .iter()
                .fold(0, |bits, &(_, slot)| bits | slot_bit(slot))
                | 1 << KEYBOARD
        }
        FromSource::Keyboard => 1 << KEYBOARD,
        FromSource::Port(name) => inputs.bit(name),
    }
}

/// The keys the track's own clips hold at arrangement time `at`, as the
/// engine plays them: see `midi_runs::sounding_keys`.
pub fn sounding_keys(track: &model::Track, at: Ticks) -> Vec<u8> {
    crate::midi_runs::sounding_keys(track, at)
}

/// The keys a track's own clips sound on its MIDI output now, while the
/// song plays: the keys a piano-roll preview on a hardware track must not
/// strike (`audition.rs`). Empty for an instrument track (its voices are
/// keyed by channel, and the preview plays on its own), while stopped, and
/// while the track's output is silenced (a mute, or another track's solo),
/// since then no clip note goes out.
pub fn busy_keys(project: &Project, id: Id, at: Ticks, playing: bool) -> Vec<u8> {
    if !playing {
        return Vec::new();
    }
    let Some(track) = project.tracks.iter().find(|t| t.id == id) else {
        return Vec::new();
    };
    if track.kind != TrackKind::Midi || track.midi_to.is_none() {
        return Vec::new();
    }
    if !lumen_project::solo::hearing(project).of(id).output {
        return Vec::new();
    }
    sounding_keys(track, at)
}

/// What the engine plays: every MIDI track's unmuted clips, in start
/// order, to the track's instrument or its MIDI output. A track sent to an
/// output that is muted (or silenced by another track's solo) plays
/// nothing, so it sends nothing and lets go of what it sounded; one sent to
/// the instrument leaves muting to the mixer, which silences the audio.
#[cfg(test)]
pub fn engine_midi(
    project: &Project,
    inputs: &Inputs,
    ports: &mut PortTable,
    cache: &mut Cache,
) -> Vec<MidiTrack> {
    engine_midi_with(project, inputs, ports, cache, &mut NoHosts)
}

/// Gives each of `tracks` the lanes that drive the parameters of its plugin
/// instrument, baked for a device at `rate` (M7 slice A3). Built-in
/// instruments have no parameters to automate yet (M8).
pub fn with_lanes(
    tracks: &mut [MidiTrack],
    project: &Project,
    over: &crate::document::Overrides,
    rate: u32,
) {
    let Ok(tempo) = lumen_core::time::TempoMap::new(project.tempo.clone()) else {
        return;
    };
    for track in tracks {
        let Some(held) = project.tracks.iter().find(|t| t.id.0 == track.id) else {
            continue;
        };
        let lanes = crate::lane_bake::device_lanes(held, over, &tempo, rate, |d| {
            d.is_vst3() && d.is_instrument()
        });
        track.set_automation(lanes.into_iter().map(|(_, lane)| lane).collect());
    }
}

/// The instrument's other outputs that are on and the track that plays each:
/// `(output pair, track)`. The pair counts the buses that are on in bus order
/// from 1, as the host's link numbers them. Every output the plugin has is on,
/// so what the plugin routes to an output is where it plays and never lands on
/// another by the plugin's own choice; an output with no track of its own
/// plays on the instrument's own track.
fn plugin_outputs(project: &Project, track: &model::Track, hosts: &dyn Hosts) -> Vec<(u32, u64)> {
    let Some(device) = track.sound_source().filter(|d| d.is_vst3()) else {
        return Vec::new();
    };
    let on = hosts.outputs_on(device.id);
    (1..32u32)
        .filter(|bus| on & (1 << bus) != 0)
        .map(|bus| {
            let pair = 1 + (on & ((1 << bus) - 1)).count_ones();
            let own = project
                .tracks
                .iter()
                .find(|t| {
                    t.plugin_output
                        .is_some_and(|o| o.device == device.id && o.bus == bus)
                })
                .map_or(track.id, |t| t.id);
            (pair, own.0)
        })
        .collect()
}

/// [`engine_midi`] with the hosts that play the plugin instruments.
pub fn engine_midi_with(
    project: &Project,
    inputs: &Inputs,
    ports: &mut PortTable,
    cache: &mut Cache,
    hosts: &mut dyn Hosts,
) -> Vec<MidiTrack> {
    cache.retain(project);
    let heard = lumen_project::solo::hearing(project);
    let mut tracks = Vec::new();
    for track in &project.tracks {
        if track.kind != TrackKind::Midi {
            continue;
        }
        let (target, silent) = match &track.midi_to {
            Some(out) => (
                Target::Port {
                    port: ports.index(&out.port),
                    channel: out.channel,
                },
                !heard.of(track.id).output,
            ),
            None => (Target::Instrument, false),
        };
        let members = track
            .clips
            .iter()
            .filter(|c| !silent && !c.muted && matches!(c.content, ClipContent::Midi { .. }))
            .filter_map(|clip| {
                Some(crate::midi_runs::Member {
                    clip,
                    length: inputs.playing_length(track.id, clip)?,
                })
            });
        let suppression = inputs.suppression(track.id);
        let mut clips = Vec::new();
        for run in crate::midi_runs::runs(members).iter() {
            clips.extend(
                cache
                    .clip_with_suppression(run, suppression)
                    .iter()
                    .cloned(),
            );
        }
        tracks.push(
            MidiTrack::new(track.id.0, target, instrument(track, hosts), clips)
                .with_outputs(plugin_outputs(project, track, hosts))
                .listening(listening(track, inputs, silent))
                .recording(recording(track, inputs))
                .recording_within(inputs.record_window)
                .recording_timing(inputs.record_timing),
        );
    }
    tracks
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lumen_core::midi::Kind;
    use lumen_project::model::{Clip, ClipEvent, EventKind, Note};

    use super::*;
    use crate::document::Document;

    fn project_with(clip: Clip) -> (Project, Id) {
        let mut doc = Document::new(crate::empty_project());
        doc.add_track(TrackKind::Midi, None);
        let id = doc.project().tracks[0].id;
        doc.apply("add clip", None, |p| {
            Arc::make_mut(&mut p.tracks[0]).clips.push(Arc::new(clip));
        });
        (doc.project().clone(), id)
    }

    fn midi_clip(notes: Vec<Note>, events: Vec<ClipEvent>) -> Clip {
        Clip {
            id: Id(900),
            name: "Part".into(),
            start: Ticks::quarters(4),
            length: Ticks::quarters(8),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            content: ClipContent::Midi { notes, events },
        }
    }

    #[test]
    fn controller_envelopes_survive_a_move_and_a_loop_extend() {
        // A MIDI clip's controller events are its envelope (M7 slice A7): they
        // move with the clip and repeat when it is loop-extended.
        let q = Ticks::quarters;
        let mut clip = midi_clip(
            vec![Note::new(q(0), q(1), 60, 100)],
            vec![
                ClipEvent {
                    at: q(0),
                    kind: EventKind::Controller {
                        number: 1,
                        value: 20,
                    },
                },
                ClipEvent {
                    at: q(1),
                    kind: EventKind::Controller {
                        number: 1,
                        value: 90,
                    },
                },
            ],
        );
        clip.length = q(2);
        let (project, track) = project_with(clip);
        let mut doc = Document::new(project);
        // Move it to quarter 8, then extend its end to quarter 14 by looping.
        let base = doc.project().clone();
        doc.move_clips(
            &base,
            &[crate::document::ClipMove {
                clip: Id(900),
                track,
                start: q(8),
            }],
            None,
        );
        let base = doc.project().clone();
        doc.trim_clip(
            &base,
            Id(900),
            lumen_project::edit::Edge::End,
            q(14),
            true,
            None,
        );
        let tracks = engine_midi(
            doc.project(),
            &Inputs::default(),
            &mut PortTable::default(),
            &mut Cache::default(),
        );
        let c = &tracks[0].clips[0];
        assert_eq!((c.start, c.length, c.period), (q(8), q(6), Some(q(2))));
        let controls: Vec<_> = c.controls.iter().map(|m| (m.at, m.msg.kind())).collect();
        assert_eq!(
            controls,
            vec![
                (
                    q(0),
                    Kind::Controller {
                        number: 1,
                        value: 20
                    }
                ),
                (
                    q(1),
                    Kind::Controller {
                        number: 1,
                        value: 90
                    }
                ),
            ],
            "one repeat's worth, which the engine plays again every period"
        );
    }

    #[test]
    fn notes_and_events_reach_the_engine_sorted_with_their_loop() {
        let q = Ticks::quarters;
        let mut clip = midi_clip(
            vec![
                Note::new(q(2), q(1), 64, 90),
                Note::new(q(0), q(1), 60, 100),
            ],
            vec![
                ClipEvent {
                    at: q(1),
                    kind: EventKind::Program {
                        program: 4,
                        bank: Some(130),
                    },
                },
                ClipEvent {
                    at: q(0),
                    kind: EventKind::PitchBend { value: -100 },
                },
            ],
        );
        clip.loop_length = Some(q(4));
        let (project, track) = project_with(clip);
        let tracks = engine_midi(
            &project,
            &Inputs::default(),
            &mut PortTable::default(),
            &mut Cache::default(),
        );
        assert_eq!(tracks.len(), 1);
        let t = &tracks[0];
        assert_eq!(t.id, track.0);
        let c = &t.clips[0];
        assert_eq!((c.start, c.length, c.period), (q(4), q(8), Some(q(4))));
        let starts: Vec<_> = c.notes.iter().map(|n| (n.start, n.pitch)).collect();
        assert_eq!(starts, vec![(q(0), 60), (q(2), 64)]);
        let kinds: Vec<_> = c.controls.iter().map(|m| (m.at, m.msg.kind())).collect();
        assert_eq!(
            kinds,
            vec![
                (q(0), Kind::PitchBend { value: -100 }),
                (
                    q(1),
                    Kind::Controller {
                        number: 0,
                        value: 1
                    }
                ),
                (
                    q(1),
                    Kind::Controller {
                        number: 32,
                        value: 2
                    }
                ),
                (q(1), Kind::Program { program: 4 }),
            ]
        );
    }

    #[test]
    fn a_plugin_instruments_parameter_lanes_reach_its_midi_track_baked_for_the_rate() {
        use lumen_project::automation::Target;
        use lumen_project::model::{DeviceKind, PluginRef};
        let mut doc = Document::new(crate::empty_project());
        doc.add_track(TrackKind::Midi, None);
        let track = doc.project().tracks[0].id;
        let synth = doc
            .add_device(
                track,
                PluginRef::Vst3 {
                    class_id: "00112233445566778899AABBCCDDEEFF".into(),
                    name: "Synth".into(),
                    vendor: "Vendor A".into(),
                    kind: DeviceKind::Instrument,
                },
                crate::document::Slot::End,
            )
            .unwrap();
        let lane = lumen_core::automation::Lane::new(vec![
            lumen_core::automation::Point::new(Ticks::ZERO, 0.25),
            lumen_core::automation::Point::new(Ticks::quarters(4), 0.75),
        ])
        .unwrap();
        doc.set_lane(
            track,
            Target::Device {
                device: synth,
                param: 6,
            },
            lane,
            false,
        )
        .unwrap();
        let mut tracks = engine_midi(
            doc.project(),
            &Inputs::default(),
            &mut PortTable::default(),
            &mut Cache::default(),
        );
        assert!(tracks[0].automation.is_empty(), "not until they are baked");
        with_lanes(&mut tracks, doc.project(), &Default::default(), 48_000);
        assert_eq!(tracks[0].automation.len(), 1);
        assert_eq!(tracks[0].automation[0].param, 6);
        // 120 BPM: four quarters are two seconds.
        assert_eq!(tracks[0].automation[0].lane.points()[1].at, 96_000);
        // At another rate, other samples.
        with_lanes(&mut tracks, doc.project(), &Default::default(), 44_100);
        assert_eq!(tracks[0].automation[0].lane.points()[1].at, 88_200);
        // The user moves that knob: the lane lets go of the instrument.
        doc.override_param(synth, 6);
        with_lanes(&mut tracks, doc.project(), doc.overrides(), 48_000);
        assert!(tracks[0].automation.is_empty(), "taken over");
        doc.reenable_all();
        with_lanes(&mut tracks, doc.project(), doc.overrides(), 48_000);
        assert_eq!(tracks[0].automation.len(), 1, "re-enabled");
    }

    #[test]
    fn muted_notes_stay_in_the_clip_but_do_not_reach_the_engine() {
        let q = Ticks::quarters;
        let mut quiet = Note::new(q(1), q(1), 62, 100);
        quiet.4 = true;
        let clip = midi_clip(
            vec![
                Note::new(q(0), q(1), 60, 100),
                quiet,
                Note::new(q(2), q(1), 64, 90),
            ],
            vec![],
        );
        let (project, _) = project_with(clip);
        let tracks = engine_midi(
            &project,
            &Inputs::default(),
            &mut PortTable::default(),
            &mut Cache::default(),
        );
        let pitches: Vec<_> = tracks[0].clips[0].notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, vec![60, 64]);
        let ClipContent::Midi { notes, .. } = &project.tracks[0].clips[0].content else {
            panic!("a MIDI clip");
        };
        assert_eq!(notes.len(), 3, "the model keeps the muted note");
    }

    #[test]
    fn notes_outside_a_clips_window_stay_in_it_but_do_not_play() {
        // Format v15: a clip trimmed or covered keeps its notes, hidden. One
        // before its start (running into it), one past its end.
        let q = Ticks::quarters;
        let clip = midi_clip(
            vec![
                Note::new(q(-1), q(2), 59, 100),
                Note::new(q(0), q(1), 60, 100),
                Note::new(q(9), q(1), 61, 100),
            ],
            vec![ClipEvent {
                at: q(-1),
                kind: EventKind::Pressure { value: 9 },
            }],
        );
        let (project, _) = project_with(clip);
        let tracks = engine_midi(
            &project,
            &Inputs::default(),
            &mut PortTable::default(),
            &mut Cache::default(),
        );
        let pitches: Vec<_> = tracks[0].clips[0].notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, vec![60]);
        assert!(tracks[0].clips[0].controls.is_empty());
        let start = project.tracks[0].clips[0].start;
        assert_eq!(sounding_keys(&project.tracks[0], start), vec![60]);
    }

    #[test]
    fn muted_clips_audio_tracks_and_empty_clips_play_nothing() {
        let mut muted = midi_clip(
            vec![Note::new(Ticks::ZERO, Ticks::quarters(1), 60, 100)],
            vec![],
        );
        muted.muted = true;
        let (project, _) = project_with(muted);
        let tracks = engine_midi(
            &project,
            &Inputs::default(),
            &mut PortTable::default(),
            &mut Cache::default(),
        );
        assert!(
            tracks[0].clips.is_empty(),
            "the track stays, so its notes are released"
        );

        let mut doc = Document::new(crate::empty_project());
        doc.add_track(TrackKind::Audio, None);
        assert!(
            engine_midi(
                doc.project(),
                &Inputs::default(),
                &mut PortTable::default(),
                &mut Cache::default()
            )
            .is_empty()
        );
    }

    fn midi_track(f: impl FnOnce(&mut model::Track)) -> model::Track {
        let mut track = model::Track::new(Id(5), "M", TrackKind::Midi, model::Color(0));
        f(&mut track);
        track
    }

    fn inputs(selected: &[Id]) -> Inputs {
        Inputs {
            selected: selected.to_vec(),
            ports: vec![("Keys".into(), 0), ("Pads".into(), 3), ("Far".into(), 70)],
            replacing: Vec::new(),
            record_window: None,
            record_timing: None,
        }
    }

    #[test]
    fn a_replace_take_silences_the_clips_it_records_over_but_not_its_own() {
        let mut clip = midi_clip(
            vec![Note::new(Ticks::ZERO, Ticks::quarters(1), 60, 100)],
            vec![],
        );
        clip.start = Ticks::quarters(4);
        clip.length = Ticks::quarters(8);
        clip.id = Id(7);
        let mut inputs = Inputs::default();
        // Nothing is being recorded: the clip plays whole.
        assert_eq!(inputs.playing_length(Id(5), &clip), Some(clip.length));
        // A take that begins inside the clip cuts it there.
        inputs.replacing = vec![Replacing {
            track: Id(5),
            from: Ticks::quarters(6),
            until: None,
            take: None,
        }];
        assert_eq!(
            inputs.playing_length(Id(5), &clip),
            Some(Ticks::quarters(2))
        );
        // One that begins at or before the clip's start silences it.
        inputs.replacing[0].from = Ticks::quarters(4);
        assert_eq!(inputs.playing_length(Id(5), &clip), None);
        inputs.replacing[0].from = Ticks::ZERO;
        assert_eq!(inputs.playing_length(Id(5), &clip), None);
        // Another track is not touched, and neither is the take's own clip.
        assert_eq!(inputs.playing_length(Id(6), &clip), Some(clip.length));
        inputs.replacing[0].take = Some(Id(7));
        assert_eq!(inputs.playing_length(Id(5), &clip), Some(clip.length));
    }

    #[test]
    fn bounded_replace_keeps_the_clip_for_playback_on_both_sides() {
        let mut clip = midi_clip(vec![], vec![]);
        clip.start = Ticks::quarters(0);
        clip.length = Ticks::quarters(12);
        clip.id = Id(7);
        let inputs = Inputs {
            replacing: vec![Replacing {
                track: Id(5),
                from: Ticks::quarters(4),
                until: Some(Ticks::quarters(8)),
                take: None,
            }],
            ..Inputs::default()
        };
        assert_eq!(inputs.playing_length(Id(5), &clip), Some(clip.length));
        assert_eq!(
            inputs.suppression(Id(5)),
            Some(crate::midi_runs::Suppression {
                from: Ticks::quarters(4),
                until: Ticks::quarters(8),
                exempt: None,
            })
        );
        assert_eq!(inputs.playing_length(Id(6), &clip), Some(clip.length));
    }

    #[test]
    fn a_track_listens_when_in_or_auto_and_armed_or_selected_never_when_off() {
        let none = inputs(&[]);
        let sel = inputs(&[Id(5)]);
        let auto = midi_track(|_| {});
        assert_eq!(listening(&auto, &none, false), Listen::OFF);
        assert_ne!(listening(&auto, &sel, false), Listen::OFF);
        let armed = midi_track(|t| t.armed = true);
        assert_ne!(listening(&armed, &none, false), Listen::OFF);
        let on = midi_track(|t| t.monitor = Monitor::In);
        assert_ne!(listening(&on, &none, false), Listen::OFF);
        let off = midi_track(|t| {
            t.monitor = Monitor::Off;
            t.armed = true;
        });
        assert_eq!(listening(&off, &sel, false), Listen::OFF);
        // A track sent to a muted or silenced output sends nothing.
        assert_eq!(listening(&on, &sel, true), Listen::OFF);
    }

    #[test]
    fn an_armed_track_records_what_its_midi_from_takes_even_with_monitor_off() {
        let none = inputs(&[]);
        let idle = midi_track(|_| {});
        assert_eq!(recording(&idle, &none), Listen::OFF);
        let armed = midi_track(|t| {
            t.armed = true;
            t.monitor = Monitor::Off;
            t.midi_from.channel = Some(4);
        });
        assert_eq!(listening(&armed, &none, false), Listen::OFF);
        let record = recording(&armed, &none);
        assert_eq!(record.channel, Some(4));
        assert_eq!(record.sources, from_sources(&armed, &none));
        assert_ne!(record.sources, 0);
    }

    #[test]
    fn the_recording_window_reaches_the_engine_tracks() {
        let (project, _) = project_with(midi_clip(Vec::new(), Vec::new()));
        let mut inputs = Inputs::default();
        let window = (Ticks::quarters(4), Ticks::quarters(8));
        inputs.record_window = Some(window);
        let tracks = engine_midi(
            &project,
            &inputs,
            &mut PortTable::default(),
            &mut Cache::default(),
        );
        assert_eq!(tracks[0].record_window, window);
        let tracks = engine_midi(
            &project,
            &Inputs::default(),
            &mut PortTable::default(),
            &mut Cache::default(),
        );
        assert_eq!(
            tracks[0].record_window,
            (Ticks(i64::MIN), Ticks(i64::MAX)),
            "unbounded when nothing records"
        );
    }

    #[test]
    fn sources_are_the_inputs_the_track_names() {
        let sel = inputs(&[Id(5)]);
        let all = listening(&midi_track(|_| {}), &sel, false);
        assert_eq!(
            all.sources,
            1 | 1 << 3 | 1 << KEYBOARD,
            "slot 70 does not fit"
        );
        let keys = midi_track(|t| t.midi_from.source = FromSource::Keyboard);
        assert_eq!(listening(&keys, &sel, false).sources, 1 << KEYBOARD);
        let port = |name: &str, channel| {
            midi_track(|t| {
                t.midi_from = model::MidiFrom {
                    source: FromSource::Port(name.into()),
                    channel,
                }
            })
        };
        let pads = listening(&port("Pads", Some(2)), &sel, false);
        assert_eq!((pads.sources, pads.channel), (1 << 3, Some(2)));
        // Unplugged, or not switched on for tracks: nothing, until it is.
        assert_eq!(listening(&port("Gone", None), &sel, false).sources, 0);
    }

    #[test]
    fn an_unchanged_clip_keeps_its_lists() {
        let clip = midi_clip(
            vec![Note::new(Ticks::ZERO, Ticks::quarters(1), 60, 100)],
            vec![],
        );
        let (project, _) = project_with(clip);
        let mut cache = Cache::default();
        let a = engine_midi(
            &project,
            &Inputs::default(),
            &mut PortTable::default(),
            &mut cache,
        );
        let b = engine_midi(
            &project,
            &Inputs::default(),
            &mut PortTable::default(),
            &mut cache,
        );
        assert!(Arc::ptr_eq(&a[0].clips[0].notes, &b[0].clips[0].notes));
    }

    // `midi_clip` starts at beat 4 and lasts eight beats; the notes below
    // are given in clip time.

    /// A track sent to the MIDI output "Synth" (a hardware track).
    fn hardware(clip: Clip) -> (Project, Id) {
        let (mut project, id) = project_with(clip);
        Arc::make_mut(&mut project.tracks[0]).midi_to = Some(model::MidiTo {
            port: "Synth".into(),
            channel: 0,
        });
        (project, id)
    }

    #[test]
    fn a_note_sounds_from_its_start_up_to_its_end_in_the_clip() {
        let q = Ticks::quarters;
        let clip = midi_clip(vec![Note::new(q(1), q(2), 60, 100)], vec![]);
        let (project, _) = project_with(clip);
        let track = &project.tracks[0];
        assert!(sounding_keys(track, q(4)).is_empty(), "before the note");
        assert_eq!(sounding_keys(track, q(5)), vec![60], "at its start");
        assert_eq!(sounding_keys(track, Ticks(q(7).0 - 1)), vec![60]);
        assert!(
            sounding_keys(track, q(7)).is_empty(),
            "its end is not in it"
        );
        assert!(sounding_keys(track, q(3)).is_empty(), "before the clip");
        assert!(sounding_keys(track, q(12)).is_empty(), "the clip's end");
    }

    #[test]
    fn a_chord_holds_each_key_once_and_a_muted_note_holds_nothing() {
        let q = Ticks::quarters;
        let mut quiet = Note::new(q(0), q(4), 62, 100);
        quiet.4 = true;
        let clip = midi_clip(
            vec![
                Note::new(q(0), q(4), 60, 100),
                Note::new(q(0), q(4), 60, 90),
                Note::new(q(0), q(4), 64, 90),
                quiet,
            ],
            vec![],
        );
        let (project, _) = project_with(clip);
        assert_eq!(sounding_keys(&project.tracks[0], q(5)), vec![60, 64]);
    }

    #[test]
    fn a_muted_clip_holds_nothing() {
        let q = Ticks::quarters;
        let mut clip = midi_clip(vec![Note::new(q(0), q(8), 60, 100)], vec![]);
        clip.muted = true;
        let (project, _) = project_with(clip);
        assert!(sounding_keys(&project.tracks[0], q(5)).is_empty());
    }

    #[test]
    fn a_looping_clip_sounds_its_notes_in_each_repeat_and_never_past_one() {
        let q = Ticks::quarters;
        // Content of two beats from beat 4; the clip lasts to beat 12.
        let mut clip = midi_clip(
            vec![
                Note::new(q(0), q(1), 60, 100),
                Note::new(q(1), q(2), 64, 100),
            ],
            vec![],
        );
        clip.loop_length = Some(q(2));
        let (project, _) = project_with(clip);
        let track = &project.tracks[0];
        // Repeat 0 (beats 4 to 6): 60, then 64, which the repeat cuts short.
        assert_eq!(sounding_keys(track, q(4)), vec![60]);
        assert_eq!(sounding_keys(track, q(5)), vec![64]);
        // Repeat 1 (beats 6 to 8): 64 does not carry over into the repeat.
        assert_eq!(sounding_keys(track, q(6)), vec![60]);
        assert_eq!(sounding_keys(track, q(7)), vec![64]);
        assert!(sounding_keys(track, q(12)).is_empty(), "the clip ended");
    }

    #[test]
    fn busy_keys_are_the_keys_a_hardware_track_sounds_only_while_playing() {
        let q = Ticks::quarters;
        let (project, id) = hardware(midi_clip(vec![Note::new(q(0), q(8), 60, 100)], vec![]));
        assert_eq!(busy_keys(&project, id, q(5), true), vec![60]);
        assert!(
            busy_keys(&project, id, q(5), false).is_empty(),
            "stopped: nothing sounds"
        );
        assert!(busy_keys(&project, Id(999), q(5), true).is_empty());
    }

    #[test]
    fn an_instrument_track_has_no_busy_keys() {
        let q = Ticks::quarters;
        let (project, id) = project_with(midi_clip(vec![Note::new(q(0), q(8), 60, 100)], vec![]));
        assert!(busy_keys(&project, id, q(5), true).is_empty());
    }

    #[test]
    fn a_silenced_hardware_output_has_no_busy_keys() {
        let q = Ticks::quarters;
        let (mut project, id) = hardware(midi_clip(vec![Note::new(q(0), q(8), 60, 100)], vec![]));
        Arc::make_mut(&mut project.tracks[0]).mute = true;
        assert!(busy_keys(&project, id, q(5), true).is_empty(), "muted");
        Arc::make_mut(&mut project.tracks[0]).mute = false;
        let mut other = model::Track::new(Id(77), "Other", TrackKind::Midi, model::Color(0));
        other.solo = true;
        project.tracks.push(Arc::new(other));
        assert!(
            busy_keys(&project, id, q(5), true).is_empty(),
            "silenced by another track's solo"
        );
    }
}
