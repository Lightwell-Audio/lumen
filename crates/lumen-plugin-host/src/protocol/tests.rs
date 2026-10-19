//! The protocol's tests.

use super::codec::*;
use super::frame::write_frame;
use super::*;
use crate::shm::{Layout, Waiting, Wake};

fn note(offset: u32, pitch: i16) -> Event {
    Event {
        offset,
        kind: EventKind::NoteOn {
            channel: 0,
            pitch,
            velocity: 0.75,
        },
    }
}

fn round_trip(request: &Request) -> Request {
    let mut bytes = Vec::new();
    write_request(&mut bytes, request).unwrap();
    read_request(&mut bytes.as_slice()).unwrap()
}

#[test]
fn requests_round_trip() {
    let process = Request::Process {
        generation: 3,
        seq: 99,
        frames: 4,
        input: vec![vec![0.0, 0.5, -0.5, 1.0]; 2],
        events: vec![
            note(1, 60),
            Event {
                offset: 3,
                kind: EventKind::NoteOff {
                    channel: 2,
                    pitch: -1,
                    velocity: 0.0,
                },
            },
            Event {
                offset: 2,
                kind: EventKind::Controller {
                    channel: 1,
                    number: 64,
                    value: 1.0,
                },
            },
            Event {
                offset: 2,
                kind: EventKind::PitchBend {
                    channel: 0,
                    value: 0.5,
                },
            },
            Event {
                offset: 3,
                kind: EventKind::Pressure {
                    channel: 15,
                    value: 0.25,
                },
            },
        ],
        params: vec![ParamQueue {
            id: 7,
            points: vec![(0, 0.25), (2, 1.0)],
        }],
    };
    for request in [
        Request::Load {
            path: "/x/y.vst3".into(),
            class: Some([9; 16]),
        },
        Request::Load {
            path: "z".into(),
            class: None,
        },
        Request::Configure {
            generation: 1,
            sample_rate: 48_000.0,
            max_frames: 512,
            inputs: 2,
            outputs: 2,
            aux_inputs: 0,
            aux_outputs: 0,
        },
        Request::Configure {
            generation: 2,
            sample_rate: 44_100.0,
            max_frames: 256,
            inputs: 2,
            outputs: 2,
            aux_inputs: 0b10,
            aux_outputs: 0b1110,
        },
        Request::Buses,
        process,
        Request::GetState,
        Request::SetState(vec![1, 2, 3]),
        Request::Parameters,
        Request::Units,
        Request::Programs,
        Request::GetControllerState,
        Request::SetControllerState(vec![3; 12]),
        Request::ParamText { id: 3, value: 0.5 },
        Request::ParamValue {
            id: 3,
            text: "2.4 kHz".into(),
        },
        Request::OpenEditor { bounds: None },
        Request::OpenEditor {
            bounds: Some(EditorBounds {
                x: -1200,
                y: 80,
                width: 640,
                height: 480,
                dpi: 144,
            }),
        },
        Request::CloseEditor,
        Request::ShowEditor(true),
        Request::ShowEditor(false),
        Request::EditorStatus,
        Request::SetParam { id: 3, value: 1.0 },
        Request::TakeEdits,
        Request::Attach {
            base: "1f2-3".into(),
            layout: Layout::new(512, 2, 2),
            wake: Wake {
                host: Waiting::Hybrid(std::time::Duration::from_micros(150)),
                lumen: Waiting::Event,
            },
        },
        Request::Attach {
            base: "a".into(),
            layout: Layout::new(8192, 0, 16),
            wake: Wake::SPIN,
        },
        Request::CaptureKeys(vec![b'A', b'W', b'Z']),
        Request::CaptureKeys(Vec::new()),
        Request::Quit,
    ] {
        assert_eq!(round_trip(&request), request);
    }
}

#[test]
fn responses_round_trip() {
    for response in [
        Response::Loaded(ClassInfo {
            id: [1; 16],
            name: "n".into(),
            category: "c".into(),
            vendor: "v".into(),
            subcategories: "s".into(),
        }),
        Response::Configured {
            generation: 2,
            latency: 24,
            tail: 24,
            inputs: 4,
            outputs: 8,
        },
        Response::Buses {
            inputs: vec![
                BusInfo {
                    name: "Input".into(),
                    channels: 2,
                    main: true,
                    default_active: true,
                },
                BusInfo {
                    name: "Sidechain".into(),
                    channels: 2,
                    main: false,
                    default_active: false,
                },
            ],
            outputs: vec![],
        },
        Response::Processed {
            generation: 2,
            seq: 5,
            status: 0,
            latency: 24,
            restart: 0,
            repaired: 3,
            output: vec![vec![0.25; 8]; 2],
        },
        Response::State(vec![5; 40]),
        Response::Parameters {
            total: 99,
            list: vec![ParamInfo {
                id: 4,
                title: "Level".into(),
                units: "dB".into(),
                steps: 0,
                default: 0.5,
                flags: FLAG_CAN_AUTOMATE | FLAG_HIDDEN,
                unit: 2,
                value: 0.25,
            }],
        },
        Response::Units(vec![
            UnitInfo {
                id: 0,
                parent: -1,
                name: "Root".into(),
            },
            UnitInfo {
                id: 2,
                parent: 0,
                name: "Filter".into(),
            },
        ]),
        Response::Programs(vec![
            ProgramList {
                id: 3,
                name: "Factory".into(),
                unit: 0,
                parameter: Some(91),
                programs: vec!["Warm pad".into(), "Glass bells".into()],
            },
            ProgramList {
                id: 4,
                name: "Empty".into(),
                unit: 1,
                parameter: None,
                programs: Vec::new(),
            },
        ]),
        Response::Text("2.4 kHz".into()),
        Response::Value(0.25),
        Response::Editor(EditorStatus::default()),
        Response::Editor(EditorStatus {
            open: true,
            visible: false,
            bounds: Some(EditorBounds {
                x: 10,
                y: 20,
                width: 320,
                height: 200,
                dpi: 96,
            }),
        }),
        Response::Edits {
            list: vec![
                Edit {
                    id: 1,
                    value: 0.0,
                    kind: EditKind::Begin,
                },
                Edit {
                    id: 1,
                    value: 0.7,
                    kind: EditKind::Perform,
                },
                Edit {
                    id: 1,
                    value: 0.0,
                    kind: EditKind::End,
                },
                Edit {
                    id: 9,
                    value: 0.1,
                    kind: EditKind::Output,
                },
            ],
            dropped: 2,
            ignored: 7,
        },
        Response::Key {
            code: b'K',
            down: true,
        },
        Response::Done,
        Response::Attached,
        Response::Attached,
        Response::Failed {
            kind: FailureKind::StaleConfiguration,
            message: "old".into(),
        },
    ] {
        let mut bytes = Vec::new();
        write_response(&mut bytes, &response).unwrap();
        assert_eq!(read_response(&mut bytes.as_slice()).unwrap(), response);
    }
}

fn frame(kind: u16, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, kind, payload).unwrap();
    bytes
}

fn rejects(bytes: &[u8]) -> bool {
    matches!(read_request(&mut &*bytes), Err(FrameError::Wire(_)))
}

#[test]
fn editor_bounds_no_window_could_have_are_refused() {
    let at = |width, height, dpi| Request::OpenEditor {
        bounds: Some(EditorBounds {
            x: 0,
            y: 0,
            width,
            height,
            dpi,
        }),
    };
    assert!(!rejects(&frame(
        REQ_OPEN_EDITOR,
        &at(800, 600, 96).encode().1
    )));
    for request in [
        at(MAX_EDITOR_SIDE + 1, 600, 96),
        at(800, MAX_EDITOR_SIDE + 1, 96),
        at(800, 600, 50_000),
    ] {
        assert!(rejects(&frame(REQ_OPEN_EDITOR, &request.encode().1)));
    }
}

#[test]
fn too_many_keys_to_capture_are_refused() {
    let mut w = crate::wire::Writer::new();
    w.bytes(&[b'A'; MAX_KEYS + 1]);
    assert!(rejects(&frame(REQ_CAPTURE_KEYS, &w.into_bytes())));
}

#[test]
fn only_a_reload_a_bus_change_or_a_new_latency_sets_a_plugin_up_again() {
    // The VST3 restart flags.
    let (reload, io, values, latency, titles, cc) = (1, 1 << 1, 1 << 2, 1 << 3, 1 << 4, 1 << 5);
    for flag in [reload, io, latency] {
        assert_ne!(RESTART_RECONFIGURE & flag, 0, "{flag}");
    }
    // What a plugin says when it loads a patch.
    for flag in [values, titles, cc] {
        assert_eq!(RESTART_RECONFIGURE & flag, 0, "{flag}");
    }
}

#[test]
fn only_a_key_is_news_rather_than_an_answer() {
    assert!(
        Response::Key {
            code: 65,
            down: false
        }
        .is_notice()
    );
    assert!(!Response::Done.is_notice());
}

#[test]
fn a_wrong_marker_or_version_is_refused() {
    let mut bytes = frame(REQ_QUIT, &[]);
    bytes[0] ^= 1;
    assert!(rejects(&bytes));
    let mut bytes = frame(REQ_QUIT, &[]);
    bytes[4] = 9;
    assert!(rejects(&bytes));
}

#[test]
fn an_absurd_length_is_refused_without_reading_it() {
    let mut bytes = frame(REQ_QUIT, &[]);
    bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(rejects(&bytes));
}

#[test]
fn a_torn_frame_is_not_a_clean_close() {
    assert!(matches!(
        read_request(&mut &[][..]),
        Err(FrameError::Closed)
    ));
    let bytes = frame(REQ_GET_STATE, &[]);
    assert!(rejects(&bytes[..5]));
    let bytes = frame(REQ_SET_STATE, &[4, 0, 0, 0, 1, 2, 3, 4]);
    assert!(rejects(&bytes[..bytes.len() - 2]));
}

#[test]
fn malformed_counts_in_a_process_request_are_refused() {
    let good = Request::Process {
        generation: 1,
        seq: 1,
        frames: 4,
        input: vec![],
        events: vec![note(1, 60)],
        params: vec![],
    };
    let mut payload = good.encode().1;
    // generation 4 + seq 8 + frames 4 + channels 4 = 20: the event count.
    payload[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(rejects(&frame(REQ_PROCESS, &payload)));
    // An event past the end of the block.
    let late = Request::Process {
        generation: 1,
        seq: 1,
        frames: 4,
        input: vec![],
        events: vec![note(4, 60)],
        params: vec![],
    };
    assert!(rejects(&frame(REQ_PROCESS, &late.encode().1)));
    // Input shorter than the block.
    let short = Request::Process {
        generation: 1,
        seq: 1,
        frames: 4,
        input: vec![vec![0.0; 3]],
        events: vec![],
        params: vec![],
    };
    assert!(rejects(&frame(REQ_PROCESS, &short.encode().1)));
    // Zero frames.
    let none = Request::Process {
        generation: 1,
        seq: 1,
        frames: 0,
        input: vec![],
        events: vec![],
        params: vec![],
    };
    assert!(rejects(&frame(REQ_PROCESS, &none.encode().1)));
}

#[test]
fn too_many_points_across_queues_are_refused() {
    let queues = (0..3)
        .map(|id| ParamQueue {
            id,
            points: vec![(0, 0.0); MAX_POINTS / 2],
        })
        .collect();
    let request = Request::Process {
        generation: 1,
        seq: 1,
        frames: 4,
        input: vec![],
        events: vec![],
        params: queues,
    };
    assert!(rejects(&frame(REQ_PROCESS, &request.encode().1)));
}

#[test]
fn a_bad_format_is_refused() {
    for (rate, frames, inputs) in [
        (f64::NAN, 512, 2),
        (1.0, 512, 2),
        (48_000.0, 0, 2),
        (48_000.0, 100_000, 2),
        (48_000.0, 512, 200),
    ] {
        let request = Request::Configure {
            generation: 1,
            sample_rate: rate,
            max_frames: frames,
            inputs,
            outputs: 2,
            aux_inputs: 0,
            aux_outputs: 0,
        };
        assert!(rejects(&frame(REQ_CONFIGURE, &request.encode().1)));
    }
}

#[test]
fn a_bad_attach_is_refused() {
    let attach = |base: &str, layout: Layout, wake: Wake| Request::Attach {
        base: base.into(),
        layout,
        wake,
    };
    let good = Layout::new(512, 2, 2);
    // Names go into the operating system's namespace: letters, digits
    // and dashes only, so a hostile base cannot reach another object.
    for base in ["", "a/b", "..", "a b", "a\\b", "x\0y"] {
        let request = attach(base, good, Wake::EVENT);
        assert!(rejects(&frame(REQ_ATTACH, &request.encode().1)), "{base:?}");
    }
    let too_long = "a".repeat(MAX_BASE + 1);
    assert!(rejects(&frame(
        REQ_ATTACH,
        &attach(&too_long, good, Wake::EVENT).encode().1
    )));
    for layout in [
        Layout::new(0, 2, 2),
        Layout::new(100_000, 2, 2),
        Layout::new(512, 200, 2),
        Layout {
            max_events: u32::MAX,
            ..good
        },
    ] {
        let request = attach("ok", layout, Wake::EVENT);
        assert!(rejects(&frame(REQ_ATTACH, &request.encode().1)));
    }
    // An unknown wake mode.
    let mut payload = attach("ok", good, Wake::EVENT).encode().1;
    let at = payload.len() - 10; // host tag
    payload[at] = 9;
    assert!(rejects(&frame(REQ_ATTACH, &payload)));
}

#[test]
fn trailing_bytes_and_unknown_kinds_are_refused() {
    let mut payload = Request::GetState.encode().1;
    payload.push(0);
    assert!(rejects(&frame(REQ_GET_STATE, &payload)));
    assert!(rejects(&frame(999, &[])));
}
