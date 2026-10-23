//! P-FORMAT pass criteria: round trips, golden fixtures, refusal of newer
//! files, robustness against damaged input, crash-safe saving.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use lumen_core::fade::{Fade, FadeShape};
use lumen_core::grid::{Feel, Grid, Size};
use lumen_core::pan::PanLaw;
use lumen_core::time::{PPQ, TempoPoint, Ticks};
use lumen_project::migrate::CURRENT_VERSION;
use lumen_project::model::{
    Clip, ClipContent, ClipEvent, Color, Device, DeviceKind, EventKind, FromSource, Id, Level,
    Media, MidiFrom, MidiTo, Monitor, Note, Output, PluginRef, Project, Send, TimeSignature, Track,
    TrackKind, constant_meter,
};
use lumen_project::model::{Marker, TransportState, ViewState};
use lumen_project::testing::{TempDir, small_project, synthetic};
use lumen_project::{
    LoadError, MediaLocation, SaveError, SaveOptions, from_bytes, load, locate_media,
    media_path_for, save, to_bytes,
};
use serde_json::Value;

/// The model the v1 golden file must always load as. Never change either:
/// when the format moves on, this test proves old projects still open.
fn golden_v1_expected() -> Project {
    let track = |id, name: &str, kind, color| Track::new(Id(id), name, kind, Color(color));
    let mut kick = track(2, "Kick", TrackKind::Audio, 0xC0504D);
    kick.parent = Some(Id(1));
    kick.gain_db = -3.0;
    kick.devices = vec![Device::new(
        Id(0x10),
        PluginRef::Builtin {
            name: "Utility".into(),
        },
        false,
        vec![0, 1, 2, 3],
    )];
    kick.clips = vec![Arc::new(Clip {
        id: Id(0x20),
        name: "Kick 1".into(),
        start: Ticks::ZERO,
        length: Ticks::quarters(4),
        muted: false,
        color: None,
        locked: false,
        fade_in: Default::default(),
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Default::default(),
        audio_phase_seconds: 0.0,
        envelopes: Default::default(),
        content: ClipContent::Audio {
            media: Id(0x30),
            offset: 0,
            gain_db: 0.0,
        },
    })];
    let mut bass = track(3, "Bass", TrackKind::Midi, 0x4F81BD);
    bass.gain_db = -6.0;
    bass.pan = -0.25;
    bass.devices = vec![Device::new(
        Id(0x11),
        PluginRef::Vst3 {
            class_id: "56535455484E36747972656C6C6E3600".into(),
            name: "TyrellN6".into(),
            vendor: "u-he".into(),
            // Files before v17 hold no kind, which means an effect.
            kind: DeviceKind::Effect,
        },
        true,
        vec![],
    )];
    bass.clips = vec![Arc::new(Clip {
        id: Id(0x21),
        name: "Bassline".into(),
        start: Ticks::quarters(4),
        length: Ticks::quarters(4),
        muted: true,
        color: None,
        locked: false,
        fade_in: Default::default(),
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Default::default(),
        audio_phase_seconds: 0.0,
        envelopes: Default::default(),
        content: ClipContent::Midi {
            notes: vec![
                Note::new(Ticks::ZERO, Ticks(PPQ / 4), 36, 100),
                Note::new(Ticks(PPQ / 3), Ticks(PPQ / 4), 43, 90),
            ],
            events: Vec::new(),
        },
    })];
    Project {
        // The v7 migration moves the master out of the tracks.
        master: Arc::new(track(4, "Master", TrackKind::Master, 0x808080)),
        pan_law: PanLaw::ConstantPower,
        name: "Golden v1".into(),
        sample_rate: 48_000,
        tempo: vec![
            TempoPoint {
                at: Ticks::ZERO,
                bpm: 120.0,
                ramp: false,
            },
            TempoPoint {
                at: Ticks::quarters(16),
                bpm: 90.0,
                ramp: false,
            },
        ],
        meter: constant_meter(TimeSignature {
            numerator: 4,
            denominator: 4,
        }),
        tracks: vec![
            Arc::new(track(1, "Drums", TrackKind::Group, 0xC0504D)),
            Arc::new(kick),
            Arc::new(bass),
        ],
        media: vec![Media {
            id: Id(0x30),
            path: "Samples/kick.wav".into(),
            original_path: None,
            frames: 48_000,
            channels: 2,
            sample_rate: 48_000,
            size_bytes: 384_044,
        }],
        // Not in v1 files: the migrations write the defaults.
        markers: Vec::new(),
        transport: Default::default(),
        view: Default::default(),
        mappings: Vec::new(),
    }
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The model the v2 golden file must always load as: v1 plus a saved loop
/// region and view.
fn golden_v2_expected() -> Project {
    Project {
        name: "Golden v2".into(),
        transport: TransportState {
            loop_start: Ticks::quarters(8),
            loop_end: Ticks::quarters(24),
            loop_enabled: true,
            ..TransportState::default()
        },
        view: ViewState {
            left_beat: 4.0,
            px_per_beat: 32.0,
            browser_open: false,
            detail_open: true,
            browser_size: 240.0,
            detail_size: 180.0,
            ..ViewState::default()
        },
        ..golden_v1_expected()
    }
}

/// The model the v3 golden file must always load as: v2 plus an armed
/// track, a vertical scroll and a resized track.
fn golden_v3_expected() -> Project {
    let mut project = Project {
        name: "Golden v3".into(),
        ..golden_v2_expected()
    };
    Arc::make_mut(&mut project.tracks[1]).armed = true;
    project.view.top = 48.0;
    project.view.track_heights.insert(Id(2), 96.0);
    project.view.expanded.insert(Id(2));
    project
}

/// The model the v4 golden file must always load as: v3 plus two
/// locators, punch in and a fixed triplet grid.
fn golden_v4_expected() -> Project {
    let mut project = Project {
        name: "Golden v4".into(),
        ..golden_v3_expected()
    };
    project.markers = vec![
        Marker {
            id: Id(0x40),
            at: Ticks::ZERO,
            name: "Intro".into(),
        },
        Marker {
            id: Id(0x41),
            at: Ticks::quarters(16),
            name: "Verse".into(),
        },
    ];
    project.transport.punch_in = true;
    project.view.grid = Grid {
        fixed: true,
        size: Size::Note(8),
        feel: Feel::Triplet,
        snap: false,
        ..Grid::default()
    };
    project
}

/// The model the v5 golden file must always load as: v4 with a clip that
/// has its own colour, a lock, fades and a loop.
fn golden_v5_expected() -> Project {
    let mut project = Project {
        name: "Golden v5".into(),
        ..golden_v4_expected()
    };
    let kick = Arc::make_mut(&mut project.tracks[1]);
    let clip = Arc::make_mut(&mut kick.clips[0]);
    clip.color = Some(Color(0x5FB3A1));
    clip.locked = true;
    clip.fade_in = Fade {
        length: Ticks(PPQ / 4),
        curve: FadeShape::SCurve.curve(),
        ..Fade::default()
    };
    clip.fade_out = Fade {
        length: Ticks(PPQ / 2),
        curve: FadeShape::Fast.curve(),
        ..Fade::default()
    };
    clip.loop_length = Some(Ticks::quarters(1));
    project
}

/// The model the v6 golden file must always load as: v5 with fades bent
/// freely by the owner's C3 handle (a dot off centre, some S-shape).
fn golden_v6_expected() -> Project {
    use lumen_core::curve::{Curve, Handle};
    let mut project = Project {
        name: "Golden v6".into(),
        ..golden_v5_expected()
    };
    let kick = Arc::make_mut(&mut project.tracks[1]);
    let clip = Arc::make_mut(&mut kick.clips[0]);
    clip.fade_in.curve = Curve::from_handle(Handle {
        dot: [0.3, 0.75],
        s: 0.4,
    });
    clip.fade_out.curve = Curve::SLOW;
    project
}

/// The model the v7 golden file must always load as: v6 routed. A return
/// track that the kick sends to before its fader and the bass after it,
/// a track straight to the master from inside the group, polarity, trim,
/// the master at −2 dB out of the second pair, and the linear pan law.
fn golden_v7_expected() -> Project {
    let mut project = Project {
        name: "Golden v7".into(),
        pan_law: PanLaw::Linear,
        ..golden_v6_expected()
    };
    let reverb = Id(0x50);
    project.tracks.push(Arc::new(Track {
        gain_db: -4.0,
        ..Track::new(reverb, "Reverb", TrackKind::Return, Color(0x7A6FB0))
    }));
    let kick = Arc::make_mut(&mut project.tracks[1]);
    kick.sends.push(Send {
        to: reverb,
        level: Level(-12.0),
        pre_fader: true,
    });
    kick.invert = true;
    kick.trim_db = 3.5;
    kick.output = Output::Master;
    let bass = Arc::make_mut(&mut project.tracks[2]);
    bass.sends.push(Send {
        to: reverb,
        level: Level::SILENT,
        pre_fader: false,
    });
    let master = Arc::make_mut(&mut project.master);
    master.gain_db = -2.0;
    master.output = Output::Device(1);
    project
}

/// The model the v8 golden file must always load as: v7 with a group
/// nested in the drums (holding a MIDI track), and the drums folded.
fn golden_v8_expected() -> Project {
    let mut project = Project {
        name: "Golden v8".into(),
        ..golden_v7_expected()
    };
    let hats = Id(0x60);
    let group = Track {
        parent: Some(Id(1)),
        ..Track::new(hats, "Hats", TrackKind::Group, Color(0x5A8E8E))
    };
    let open_hat = Track {
        parent: Some(hats),
        gain_db: -1.5,
        ..Track::new(Id(0x61), "Open hat", TrackKind::Midi, Color(0x5A8E8E))
    };
    project.tracks.insert(2, Arc::new(group));
    project.tracks.insert(3, Arc::new(open_hat));
    project.view.collapsed.insert(Id(1));
    project
}

/// The model the v9 golden file must always load as: v8 with a wider
/// header column.
fn golden_v9_expected() -> Project {
    let mut project = Project {
        name: "Golden v9".into(),
        ..golden_v8_expected()
    };
    project.view.header_width = 296.0;
    project
}

/// The model the v10 golden file must always load as: v9 with the area
/// pinned at the bottom dragged to 110 points and its returns folded.
fn golden_v10_expected() -> Project {
    let mut project = Project {
        name: "Golden v10".into(),
        ..golden_v9_expected()
    };
    project.view.returns_height = Some(110.0);
    project.view.returns_folded = true;
    project
}

/// The model the v11 golden file must always load as: v10 with the drum
/// MIDI clip carrying a controller, pitch bend, pressure and a program change.
fn golden_v11_expected() -> Project {
    let mut project = Project {
        name: "Golden v11".into(),
        ..golden_v10_expected()
    };
    let track = project
        .tracks
        .iter_mut()
        .find(|t| {
            t.clips
                .iter()
                .any(|c| matches!(c.content, ClipContent::Midi { .. }))
        })
        .expect("the golden project has a MIDI clip");
    for clip in &mut Arc::make_mut(track).clips {
        if let ClipContent::Midi { events, .. } = &mut Arc::make_mut(clip).content {
            let at = |t| Ticks(PPQ * t / 4);
            *events = vec![
                ClipEvent {
                    at: at(0),
                    kind: EventKind::Program {
                        program: 5,
                        bank: Some(130),
                    },
                },
                ClipEvent {
                    at: at(1),
                    kind: EventKind::Controller {
                        number: 64,
                        value: 127,
                    },
                },
                ClipEvent {
                    at: at(2),
                    kind: EventKind::PitchBend { value: -2048 },
                },
                ClipEvent {
                    at: at(3),
                    kind: EventKind::Pressure { value: 90 },
                },
            ];
        }
    }
    project
}

/// The model the v12 golden file must always load as: v11 with the Bass
/// track given the synth (on its preset Keys) and a MIDI output.
fn golden_v12_expected() -> Project {
    let mut project = Project {
        name: "Golden v12".into(),
        ..golden_v11_expected()
    };
    let bass = project
        .tracks
        .iter_mut()
        .find(|t| t.name == "Bass")
        .expect("the golden project has a Bass track");
    let bass = Arc::make_mut(bass);
    bass.devices.push(Device::synth(Id(0x12), "keys"));
    bass.midi_to = Some(MidiTo {
        port: "Microsoft GS Wavetable Synth".into(),
        channel: 9,
    });
    project
}

#[test]
fn golden_v12_file_loads_forever() {
    let path = fixture("v12/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 12 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v12_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v12 golden file must load");
    assert_eq!(project, golden_v12_expected());
    let bass = project.tracks.iter().find(|t| t.name == "Bass").unwrap();
    assert_eq!(bass.instrument().and_then(Device::preset), Some("keys"));
}

/// The model the v13 golden file must always load as: v12 with the Bass
/// track listening to one port on channel 3, and monitoring In.
fn golden_v13_expected() -> Project {
    let mut project = Project {
        name: "Golden v13".into(),
        ..golden_v12_expected()
    };
    let bass = project
        .tracks
        .iter_mut()
        .find(|t| t.name == "Bass")
        .expect("the golden project has a Bass track");
    let bass = Arc::make_mut(bass);
    bass.midi_from = MidiFrom {
        source: FromSource::Port("Keystep 37".into()),
        channel: Some(2),
    };
    bass.monitor = Monitor::In;
    project
}

#[test]
fn golden_v13_file_loads_forever() {
    let path = fixture("v13/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 13 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v13_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v13 golden file must load");
    assert_eq!(project, golden_v13_expected());
}

/// The model the v14 golden file must always load as: v13 with the first
/// note of the MIDI clip muted.
fn golden_v14_expected() -> Project {
    let mut project = Project {
        name: "Golden v14".into(),
        ..golden_v13_expected()
    };
    let track = project
        .tracks
        .iter_mut()
        .find(|t| {
            t.clips
                .iter()
                .any(|c| matches!(c.content, ClipContent::Midi { .. }))
        })
        .expect("the golden project has a MIDI clip");
    for clip in &mut Arc::make_mut(track).clips {
        if let ClipContent::Midi { notes, .. } = &mut Arc::make_mut(clip).content {
            notes[0].4 = true;
        }
    }
    project
}

#[test]
fn golden_v14_file_loads_forever() {
    let path = fixture("v14/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 14 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v14_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v14 golden file must load");
    assert_eq!(project, golden_v14_expected());
}

/// The model the v15 golden file must always load as: v14 with the MIDI
/// clip keeping a note before its start and one past its end, hidden (a
/// clip is a window onto its notes).
fn golden_v15_expected() -> Project {
    let mut project = Project {
        name: "Golden v15".into(),
        ..golden_v14_expected()
    };
    let track = project
        .tracks
        .iter_mut()
        .find(|t| {
            t.clips
                .iter()
                .any(|c| matches!(c.content, ClipContent::Midi { .. }))
        })
        .expect("the golden project has a MIDI clip");
    for clip in &mut Arc::make_mut(track).clips {
        let clip = Arc::make_mut(clip);
        let past = clip.length + Ticks::quarters(1);
        if let ClipContent::Midi { notes, .. } = &mut clip.content {
            notes.insert(
                0,
                Note::new(Ticks::quarters(-2), Ticks::quarters(1), 40, 80),
            );
            notes.push(Note::new(past, Ticks::quarters(1), 41, 81));
        }
    }
    project
}

#[test]
fn golden_v15_file_loads_forever() {
    let path = fixture("v15/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 15 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v15_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v15 golden file must load");
    assert_eq!(project, golden_v15_expected());
    assert!(
        project
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .flat_map(|clip| match &clip.content {
                ClipContent::Midi { notes, .. } => notes.iter().collect::<Vec<_>>(),
                ClipContent::Audio { .. } => Vec::new(),
            })
            .all(|note| note.carries_on())
    );
    let clip = project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| matches!(c.content, ClipContent::Midi { .. }))
        .unwrap();
    let ClipContent::Midi { notes, .. } = &clip.content else {
        unreachable!()
    };
    let shown = lumen_project::edit::shown_notes(clip);
    assert!(!shown.contains(&0), "the note before the start is hidden");
    assert!(
        !shown.contains(&(notes.len() - 1)),
        "and the one past the end"
    );
}

/// The model the v16 golden file must always load as: v15 with one hidden
/// recorded note explicitly barred from carrying through an adjacent seam.
fn golden_v16_expected() -> Project {
    let mut project = Project {
        name: "Golden v16".into(),
        ..golden_v15_expected()
    };
    let clip = project
        .tracks
        .iter_mut()
        .flat_map(|track| &mut Arc::make_mut(track).clips)
        .find(|clip| matches!(clip.content, ClipContent::Midi { .. }))
        .expect("the golden project has a MIDI clip");
    if let ClipContent::Midi { notes, .. } = &mut Arc::make_mut(clip).content {
        notes[0] = notes[0].with_carries_on(false);
    }
    project
}

#[test]
fn golden_v16_file_loads_forever() {
    let path = fixture("v16/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v16_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v16 golden file must load");
    assert_eq!(project, golden_v16_expected());
}

#[test]
fn legacy_notes_default_to_seam_carry_and_false_is_saved_explicitly() {
    let old = load(&fixture("v15/golden.lumen")).unwrap();
    assert!(
        old.tracks
            .iter()
            .flat_map(|track| &track.clips)
            .flat_map(|clip| match &clip.content {
                ClipContent::Midi { notes, .. } => notes.iter().collect::<Vec<_>>(),
                ClipContent::Audio { .. } => Vec::new(),
            })
            .all(|note| note.carries_on())
    );
    let text = String::from_utf8(to_bytes(&golden_v16_expected()).unwrap()).unwrap();
    let flat: String = text.split_whitespace().collect();
    assert!(flat.contains("[-2822400,1411200,40,80,false,false]"));
}

#[test]
fn a_muted_note_is_written_with_a_fifth_element_and_others_as_before() {
    let text = String::from_utf8(to_bytes(&golden_v14_expected()).unwrap()).unwrap();
    let flat: String = text.split_whitespace().collect();
    assert!(flat.contains("[0,352800,36,100,true]"), "muted note");
    assert!(flat.contains("[470400,352800,43,90]"), "plain note");
    // And the v13 file, which has no fifth element, loads unmuted.
    let old = load(&fixture("v13/golden.lumen")).unwrap();
    assert!(
        old.tracks
            .iter()
            .flat_map(|t| &t.clips)
            .all(|c| match &c.content {
                ClipContent::Midi { notes, .. } => notes.iter().all(|n| !n.muted()),
                _ => true,
            })
    );
}

#[test]
fn midi_inputs_belong_on_midi_tracks_and_are_checked() {
    let good = golden_v13_expected();
    assert!(from_bytes(&to_bytes(&good).unwrap()).is_ok());
    let audio = good
        .tracks
        .iter()
        .position(|t| t.kind == TrackKind::Audio)
        .unwrap();
    let bass = good.tracks.iter().position(|t| t.name == "Bass").unwrap();
    let refused = |edit: &dyn Fn(&mut Project)| {
        let mut project = good.clone();
        edit(&mut project);
        to_bytes(&project).map_or(true, |bytes| from_bytes(&bytes).is_err())
    };
    assert!(refused(&|p| Arc::make_mut(&mut p.tracks[audio])
        .midi_from = MidiFrom {
        source: FromSource::Keyboard,
        channel: None
    }));
    assert!(refused(&|p| Arc::make_mut(&mut p.tracks[bass])
        .midi_from
        .channel = Some(16)));
    assert!(refused(&|p| Arc::make_mut(&mut p.tracks[bass])
        .midi_from
        .source = FromSource::Port(String::new())));
    // Monitor is allowed on any kind of track.
    let mut monitored = good.clone();
    Arc::make_mut(&mut monitored.tracks[audio]).monitor = Monitor::Off;
    assert!(from_bytes(&to_bytes(&monitored).unwrap()).is_ok());
}

#[test]
fn golden_v11_file_loads_forever() {
    let path = fixture("v11/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 11 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v11_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v11 golden file must load");
    assert_eq!(project, golden_v11_expected());
}

#[test]
fn golden_v10_file_loads_forever() {
    let path = fixture("v10/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 10 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v10_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v10 golden file must load");
    assert_eq!(project, golden_v10_expected());
}

#[test]
fn golden_v9_file_loads_forever() {
    let path = fixture("v9/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 9 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v9_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v9 golden file must load");
    assert_eq!(project, golden_v9_expected());
}

#[test]
fn golden_v8_file_loads_forever() {
    let path = fixture("v8/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 8 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v8_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v8 golden file must load");
    assert_eq!(project, golden_v8_expected());
}

#[test]
fn golden_v7_file_loads_forever() {
    let path = fixture("v7/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 7 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v7_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v7 golden file must load");
    assert_eq!(project, golden_v7_expected());
}

#[test]
fn golden_v6_file_loads_forever() {
    let path = fixture("v6/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 6 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v6_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v6 golden file must load");
    assert_eq!(project, golden_v6_expected());
}

#[test]
fn golden_v5_file_loads_forever() {
    let path = fixture("v5/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 5 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v5_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v5 golden file must load");
    assert_eq!(project, golden_v5_expected());
}

#[test]
fn golden_v4_file_loads_forever() {
    let path = fixture("v4/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 4 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v4_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v4 golden file must load");
    assert_eq!(project, golden_v4_expected());
}

#[test]
fn golden_v3_file_loads_forever() {
    let path = fixture("v3/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 3 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v3_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v3 golden file must load");
    assert_eq!(project, golden_v3_expected());
}

#[test]
fn golden_v2_file_loads_forever() {
    let path = fixture("v2/golden.lumen");
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() && !path.exists() {
        // Written once, when version 2 was introduced; never rewritten.
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&golden_v2_expected()).unwrap()).unwrap();
    }
    let project = load(&path).expect("v2 golden file must load");
    assert_eq!(project, golden_v2_expected());
}

#[test]
fn golden_v1_file_loads_forever() {
    let project = load(&fixture("v1/golden.lumen")).expect("v1 golden file must load");
    assert_eq!(project, golden_v1_expected());
}

#[test]
fn round_trips_are_exact() {
    for seed in 0..25 {
        let project = synthetic(
            seed,
            3 + seed as usize % 9,
            1 + seed as usize % 5,
            seed as usize % 13,
            37,
        );
        let bytes = to_bytes(&project).unwrap();
        assert_eq!(from_bytes(&bytes).unwrap(), project, "seed {seed}");
    }
    let dir = TempDir::new("round-trip");
    let path = dir.path().join("Song.lumen");
    save(&golden_v1_expected(), &path, &SaveOptions::default()).unwrap();
    assert_eq!(load(&path).unwrap(), golden_v1_expected());
}

#[test]
fn output_is_readable_json_with_one_note_per_line() {
    let text = String::from_utf8(to_bytes(&golden_v1_expected()).unwrap()).unwrap();
    assert!(
        text.starts_with(&format!(
            "{{\n  \"format\": \"lumen-project\",\n  \"version\": {CURRENT_VERSION},"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("[{}, {}, 43, 90]", PPQ / 3, PPQ / 4)),
        "{text}"
    );
}

#[test]
fn newer_versions_are_refused_with_a_clear_message() {
    let mut document: Value =
        serde_json::from_slice(&to_bytes(&small_project(1)).unwrap()).unwrap();
    document["version"] = Value::from(999);
    let error = from_bytes(&serde_json::to_vec(&document).unwrap()).unwrap_err();
    assert!(
        matches!(
            error,
            LoadError::TooNew {
                version: 999,
                supported: CURRENT_VERSION
            }
        ),
        "{error:?}"
    );
    assert!(error.to_string().contains("newer version of Lumen"));
}

#[test]
fn other_files_are_not_projects() {
    for text in [
        r#"{}"#,
        r#"[]"#,
        r#"{"format":"something-else","version":1}"#,
        r#"{"format":"lumen-project"}"#,
    ] {
        assert!(
            matches!(from_bytes(text.as_bytes()), Err(LoadError::NotAProject)),
            "{text}"
        );
    }
}

#[test]
fn every_truncation_is_an_error_not_a_panic() {
    let bytes = to_bytes(&small_project(7)).unwrap();
    // The final byte is a newline; everything shorter than that is incomplete.
    for cut in 0..bytes.len() - 1 {
        let result = from_bytes(&bytes[..cut]);
        assert!(
            result.is_err(),
            "truncation at {cut} of {} loaded",
            bytes.len()
        );
    }
}

#[test]
fn random_damage_never_panics() {
    let original = to_bytes(&small_project(11)).unwrap();
    let mut state = 0x853C_49E6_748F_EA9B_u64;
    let mut rejected = 0;
    for _ in 0..5_000 {
        let mut damaged = original.clone();
        for _ in 0..1 + state % 4 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let at = (state % damaged.len() as u64) as usize;
            damaged[at] = (state >> 32) as u8;
        }
        if from_bytes(&damaged).is_err() {
            rejected += 1;
        }
    }
    assert!(
        rejected > 4_000,
        "most random damage should be detected, got {rejected}"
    );
}

#[test]
fn inconsistent_projects_are_rejected_precisely() {
    let mut bad_pitch = golden_v1_expected();
    if let ClipContent::Midi { notes, .. } =
        &mut Arc::make_mut(&mut Arc::make_mut(&mut bad_pitch.tracks[2]).clips[0]).content
    {
        notes[0].2 = 200;
    }
    let mut cycle = golden_v1_expected();
    Arc::make_mut(&mut cycle.tracks[0]).parent = Some(Id(2)); // group inside its own child
    Arc::make_mut(&mut cycle.tracks[1]).kind = TrackKind::Group;
    let mut missing_media = golden_v1_expected();
    missing_media.media.clear();
    let mut apart = golden_v1_expected();
    apart.tracks.swap(1, 2); // the bass between the drums and the kick
    let mut grouped_return = golden_v1_expected();
    Arc::make_mut(&mut grouped_return.tracks[1]).kind = TrackKind::Return;

    for (project, expected) in [
        (bad_pitch, "out of range"),
        (cycle, "cycle"),
        (missing_media, "missing media"),
        (apart, "not right under it"),
        (grouped_return, "inside a group"),
    ] {
        // Saving refuses, so such a file is never written...
        let Err(SaveError::Invalid(message)) = to_bytes(&project) else {
            panic!("saved an invalid project")
        };
        assert!(message.contains(expected), "{message}");
        // ...and a hand-edited one is rejected on load.
        let mut document: Value =
            serde_json::from_slice(&to_bytes(&golden_v1_expected()).unwrap()).unwrap();
        document["project"] = serde_json::to_value(&project).unwrap();
        let error = from_bytes(&serde_json::to_vec(&document).unwrap()).unwrap_err();
        assert!(
            matches!(error, LoadError::Invalid(ref m) if m.contains(expected)),
            "{error:?}"
        );
    }
}

#[test]
fn a_project_that_cannot_round_trip_is_never_written() {
    let dir = TempDir::new("nan");
    let path = dir.path().join("Song.lumen");
    save(&golden_v1_expected(), &path, &SaveOptions::default()).unwrap();
    let before = fs::read(&path).unwrap();
    let mut broken = golden_v1_expected();
    Arc::make_mut(&mut broken.tracks[1]).gain_db = f32::NAN; // JSON would turn this into null
    assert!(matches!(
        save(&broken, &path, &SaveOptions::default()),
        Err(SaveError::Invalid(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn media_inside_the_project_moves_with_it() {
    let original = TempDir::new("media-a");
    let samples = original.path().join("Samples");
    fs::create_dir_all(&samples).unwrap();
    fs::write(samples.join("kick.wav"), b"RIFF").unwrap();
    let stored = media_path_for(original.path(), &samples.join("kick.wav"));
    assert_eq!(stored, "Samples/kick.wav");

    let media = Media {
        id: Id(1),
        path: stored,
        original_path: None,
        frames: 0,
        channels: 1,
        sample_rate: 48_000,
        size_bytes: 4,
    };
    let moved = TempDir::new("media-b");
    fs::create_dir_all(moved.path().join("Samples")).unwrap();
    fs::copy(
        samples.join("kick.wav"),
        moved.path().join("Samples").join("kick.wav"),
    )
    .unwrap();
    assert!(matches!(
        locate_media(moved.path(), &media),
        MediaLocation::Found(_)
    ));

    let empty = TempDir::new("media-c");
    assert!(matches!(
        locate_media(empty.path(), &media),
        MediaLocation::Missing { .. }
    ));
}

#[test]
fn midi_outputs_and_synths_belong_on_midi_tracks_and_are_checked() {
    let good = golden_v12_expected();
    assert!(from_bytes(&to_bytes(&good).unwrap()).is_ok());
    let audio = good
        .tracks
        .iter()
        .position(|t| t.kind == TrackKind::Audio)
        .unwrap();
    let bass = good.tracks.iter().position(|t| t.name == "Bass").unwrap();
    let refused = |edit: &dyn Fn(&mut Project)| {
        let mut project = good.clone();
        edit(&mut project);
        to_bytes(&project).map_or(true, |bytes| from_bytes(&bytes).is_err())
    };
    assert!(refused(&|p| Arc::make_mut(&mut p.tracks[audio])
        .devices
        .push(Device::synth(Id(0x99), "poly"))));
    assert!(refused(&|p| Arc::make_mut(&mut p.tracks[audio]).midi_to =
        Some(MidiTo {
            port: "X".into(),
            channel: 0
        })));
    assert!(refused(&|p| Arc::make_mut(&mut p.tracks[bass])
        .midi_to
        .as_mut()
        .unwrap()
        .channel = 16));
    assert!(refused(&|p| Arc::make_mut(&mut p.tracks[bass])
        .midi_to
        .as_mut()
        .unwrap()
        .port
        .clear()));
    assert!(refused(&|p| Arc::make_mut(&mut p.tracks[bass]).devices
        [1]
    .state = vec![0xFF, 0xFE]));
}

#[test]
fn the_midi_demo_project_file_is_current_and_valid() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/projects/midi-demo.lumen");
    let demo = lumen_project::testing::midi_demo();
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&demo).unwrap()).unwrap();
    }
    let loaded =
        load(&path).expect("the demo project must load (regenerate with LUMEN_WRITE_FIXTURES=1)");
    assert_eq!(loaded, demo, "regenerate with LUMEN_WRITE_FIXTURES=1");
    assert_eq!(loaded.tracks.len(), 5);
}

#[test]
fn the_automation_demo_project_file_is_current_and_valid() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/projects/automation-demo.lumen");
    let demo = lumen_project::testing::automation_demo();
    if std::env::var_os("LUMEN_WRITE_FIXTURES").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, to_bytes(&demo).unwrap()).unwrap();
    }
    let loaded =
        load(&path).expect("the demo project must load (regenerate with LUMEN_WRITE_FIXTURES=1)");
    assert_eq!(loaded, demo, "regenerate with LUMEN_WRITE_FIXTURES=1");
    assert_eq!(loaded.tracks.len(), 3);
    assert!(loaded.tracks.iter().all(|t| t.automation.len() == 1));
}

#[test]
fn the_midi_demo_arpeggio_and_bass_start_on_each_chords_root_on_the_downbeat() {
    use lumen_project::model::ClipContent;
    let demo = lumen_project::testing::midi_demo();
    let notes =
        |name: &str| match &demo.tracks.iter().find(|t| t.name == name).unwrap().clips[0].content {
            ClipContent::Midi { notes, .. } => notes.clone(),
            _ => panic!("not MIDI"),
        };
    let chords = notes("Chords");
    let bar = lumen_core::time::Ticks::quarters(4);
    for track in ["Hardware", "Bass"] {
        let line = notes(track);
        for b in 0..4_i64 {
            let start = lumen_core::time::Ticks(bar.0 * b);
            let first = line
                .iter()
                .find(|n| n.0 == start)
                .expect("a note on the downbeat");
            // The chord of bar b: its lowest note is the root (pitch class).
            let root = chords
                .iter()
                .filter(|n| n.0 == start)
                .map(|n| n.2)
                .min()
                .unwrap();
            assert_eq!(
                first.2 % 12,
                root % 12,
                "{track} bar {b} starts on the root"
            );
            let in_bar = line
                .iter()
                .filter(|n| n.0 >= start && n.0.0 < start.0 + bar.0);
            assert_eq!(in_bar.count(), 8, "{track} bar {b}: eight eighth notes");
        }
    }
}
