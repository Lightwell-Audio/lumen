//! The engine's MIDI tracks derived from the project (M5 slice 1, ADR-0045):
//! each MIDI track's clips as notes and controller messages in ticks, with
//! loop repeats, where they go (the track's instrument, or a MIDI output
//! port) and which instrument plays them. Pure, like `lanes::engine_clips`
//! and `routing::routing`, so the translation is tested.
//!
//! Clips that did not change keep their note lists (shared by clip, so the
//! engine's comparison is a pointer check and a 100,000-note clip is not
//! copied on every edit elsewhere).

use std::collections::HashMap;
use std::sync::Arc;

use lumen_core::midi::Msg;
use lumen_devices::{Preset, Synth, synth};
use lumen_engine::{
    InstrumentSpec, KEYBOARD, Listen, MidiClip, MidiControl, MidiNote, MidiTrack, Target,
    control_list, note_list,
};
use lumen_project::model::{
    self, ClipContent, EventKind, FromSource, Id, Monitor, Project, TrackKind,
};

/// A clip's translated lists, kept while the clip itself is unchanged.
struct Cached {
    /// The model clip they were built from (also keeps its address from
    /// being reused while the entry lives).
    source: Arc<model::Clip>,
    notes: Arc<[MidiNote]>,
    controls: Arc<[MidiControl]>,
}

/// Translated clips by clip id.
#[derive(Default)]
pub struct Cache {
    clips: HashMap<Id, Cached>,
}

impl Cache {
    /// The lists for `clip`, rebuilt only if it changed.
    fn lists(&mut self, clip: &Arc<model::Clip>) -> (Arc<[MidiNote]>, Arc<[MidiControl]>) {
        if let Some(c) = self.clips.get(&clip.id)
            && (Arc::ptr_eq(&c.source, clip) || *c.source == **clip)
        {
            return (Arc::clone(&c.notes), Arc::clone(&c.controls));
        }
        let ClipContent::Midi { notes, events } = &clip.content else {
            return (note_list(Vec::new()), control_list(Vec::new()));
        };
        let notes = note_list(
            notes
                .iter()
                .map(|n| MidiNote {
                    start: n.start(),
                    length: n.length(),
                    pitch: n.pitch(),
                    velocity: n.velocity(),
                })
                .collect(),
        );
        let controls = control_list(events.iter().flat_map(control).collect());
        self.clips.insert(
            clip.id,
            Cached {
                source: Arc::clone(clip),
                notes: Arc::clone(&notes),
                controls: Arc::clone(&controls),
            },
        );
        (notes, controls)
    }

    /// Forgets clips that are not in `project` any more.
    fn retain(&mut self, project: &Project) {
        let live: std::collections::HashSet<Id> = project
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        if self.clips.len() > live.len() {
            self.clips.retain(|id, _| live.contains(id));
        }
    }
}

/// A clip event as the messages that carry it (a program change with a
/// bank sends the bank first).
fn control(event: &model::ClipEvent) -> Vec<MidiControl> {
    let at = event.at;
    let msg = |m: Msg| MidiControl { at, msg: m };
    match event.kind {
        EventKind::Controller { number, value } => vec![msg(Msg::controller(0, number, value))],
        EventKind::PitchBend { value } => vec![msg(Msg::pitch_bend(0, value))],
        EventKind::Pressure { value } => vec![msg(Msg::pressure(0, value))],
        EventKind::Program { program, bank } => {
            let mut out = Vec::new();
            if let Some(bank) = bank {
                out.push(msg(Msg::controller(
                    0,
                    lumen_core::midi::cc::BANK_MSB,
                    (bank >> 7) as u8,
                )));
                out.push(msg(Msg::controller(
                    0,
                    lumen_core::midi::cc::BANK_LSB,
                    (bank & 127) as u8,
                )));
            }
            out.push(msg(Msg::program(0, program)));
            out
        }
    }
}

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
fn instrument(track: &model::Track) -> Option<InstrumentSpec> {
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

/// What decides which tracks play what is played live (M5 slice 2): the
/// selected tracks and the inputs switched on for tracks. Compared each
/// frame, so the engine's list is sent again only when one changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inputs {
    /// The selected tracks.
    pub selected: Vec<Id>,
    /// Each input switched on for tracks: its id name and its slot.
    pub ports: Vec<(String, u16)>,
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
    if slot < u16::from(KEYBOARD) {
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
    let sources = match &track.midi_from.source {
        FromSource::All => {
            inputs
                .ports
                .iter()
                .fold(0, |bits, &(_, slot)| bits | slot_bit(slot))
                | 1 << KEYBOARD
        }
        FromSource::Keyboard => 1 << KEYBOARD,
        FromSource::Port(name) => inputs.bit(name),
    };
    Listen {
        sources,
        channel: track.midi_from.channel,
    }
}

/// What the engine plays: every MIDI track's unmuted clips, in start
/// order, to the track's instrument or its MIDI output. A track sent to an
/// output that is muted (or silenced by another track's solo) plays
/// nothing, so it sends nothing and lets go of what it sounded; one sent to
/// the instrument leaves muting to the mixer, which silences the audio.
pub fn engine_midi(
    project: &Project,
    inputs: &Inputs,
    ports: &mut PortTable,
    cache: &mut Cache,
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
        let clips = track
            .clips
            .iter()
            .filter(|c| !silent && !c.muted && matches!(c.content, ClipContent::Midi { .. }))
            .filter(|c| c.length.0 > 0)
            .map(|clip| {
                let (notes, controls) = cache.lists(clip);
                MidiClip {
                    id: clip.id.0,
                    start: clip.start,
                    length: clip.length,
                    period: clip.loop_length.filter(|l| l.0 > 0),
                    notes,
                    controls,
                }
            })
            .collect();
        tracks.push(
            MidiTrack::new(track.id.0, target, instrument(track), clips)
                .listening(listening(track, inputs, silent)),
        );
    }
    tracks
}

#[cfg(test)]
mod tests {
    use lumen_core::midi::Kind;
    use lumen_core::time::Ticks;
    use lumen_project::model::{Clip, ClipEvent, Note};

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
            content: ClipContent::Midi { notes, events },
        }
    }

    #[test]
    fn notes_and_events_reach_the_engine_sorted_with_their_loop() {
        let q = Ticks::quarters;
        let mut clip = midi_clip(
            vec![Note(q(2), q(1), 64, 90), Note(q(0), q(1), 60, 100)],
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
    fn muted_clips_audio_tracks_and_empty_clips_play_nothing() {
        let mut muted = midi_clip(vec![Note(Ticks::ZERO, Ticks::quarters(1), 60, 100)], vec![]);
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
        }
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
        let clip = midi_clip(vec![Note(Ticks::ZERO, Ticks::quarters(1), 60, 100)], vec![]);
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
}
