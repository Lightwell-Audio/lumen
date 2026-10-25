//! MIDI clock out (M5 slice 7, ADR-0062): Sync on an output in the MIDI
//! settings, then the port gets Start, 24 clocks a quarter through a tempo
//! change, Stop, the song position and Continue at each loop wrap and
//! jump, and Stop when the song stops. Counted between transport messages,
//! so the test device's pace does not matter.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::{TempoPoint, Ticks};
use lumen_midi::Direction;
use lumen_midi::fake::FakeControl;
use lumen_project::testing::{TempDir, project_of};

use super::Lumen;

const CLOCK: u8 = 0xF8;
const START: u8 = 0xFA;
const CONTINUE: u8 = 0xFB;
const STOP: u8 = 0xFC;
const POSITION: u8 = 0xF2;

/// What the drum machine got, in order.
fn sent(fake: &FakeControl) -> Vec<Vec<u8>> {
    fake.sent()
        .into_iter()
        .filter(|(id, _)| id.name == "Drum machine")
        .map(|(_, bytes)| bytes)
        .collect()
}

#[track_caller]
fn until_sent(
    lumen: &mut Lumen,
    fake: &FakeControl,
    what: &str,
    done: impl Fn(&[Vec<u8>]) -> bool,
) {
    for _ in 0..3_000 {
        lumen.frames(1);
        if done(&sent(fake)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("{what}; the port got {} messages", sent(fake).len());
}

/// Clocks between each transport message: `(status, clocks after it)`.
fn runs(sent: &[Vec<u8>]) -> Vec<(Vec<u8>, usize)> {
    let mut out: Vec<(Vec<u8>, usize)> = Vec::new();
    for m in sent {
        if m[0] == CLOCK {
            if let Some(last) = out.last_mut() {
                last.1 += 1;
            }
        } else {
            out.push((m.clone(), 0));
        }
    }
    out
}

#[test]
fn a_synced_port_follows_play_the_tempo_map_loops_jumps_and_stop() {
    let fake = FakeControl::new();
    fake.plug(Direction::Output, "Drum machine");
    // 480 BPM for a bar, then 960; a loop from the second beat to the
    // tenth, across the change.
    let mut project = project_of("Clock", Vec::new());
    project.tempo = vec![
        TempoPoint {
            at: Ticks::ZERO,
            bpm: 480.0,
            ramp: false,
        },
        TempoPoint {
            at: Ticks::quarters(4),
            bpm: 960.0,
            ramp: false,
        },
    ];
    project.transport.loop_start = Ticks::quarters(2);
    project.transport.loop_end = Ticks::quarters(10);
    project.transport.loop_enabled = true;
    let dir = TempDir::new("e2e-clock");
    let path = dir.path().join("song.lumen");
    lumen_project::save(&project, &path, &Default::default()).unwrap();
    let midi = fake.clone();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path).with_midi(midi));
    lumen.wait_for("the project", |app| app.project().tempo.len() == 2);

    // Sync on in the MIDI settings: the delay field appears beside it.
    lumen.open_midi_settings();
    for _ in 0..400 {
        if lumen.shows("Sync Drum machine output") {
            break;
        }
        lumen.frames(1);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!lumen.shows("Sync delay of Drum machine: 0 ms"));
    lumen.click("Sync Drum machine output");
    assert!(lumen.is_on("Sync Drum machine output"));
    assert!(lumen.shows("Sync delay of Drum machine: 0 ms"));
    lumen.key(Modifiers::NONE, Key::Escape);
    assert!(sent(&fake).is_empty(), "nothing while stopped");

    lumen.key(Modifiers::NONE, Key::Space);
    until_sent(&mut lumen, &fake, "two loop wraps", |sent| {
        sent.iter().filter(|m| m[0] == CONTINUE).count() >= 2
    });
    let got = runs(&sent(&fake));
    // From the song's start: Start, then every clock to the loop's end
    // (ten quarters, four at 480 BPM and six at 960).
    assert_eq!(got[0], (vec![START], 240));
    // Each wrap: Stop, the loop start (eight sixteenths), Continue, then
    // the loop's eight quarters.
    assert_eq!(got[1], (vec![STOP], 0));
    assert_eq!(got[2], (vec![POSITION, 8, 0], 0));
    assert_eq!(got[3], (vec![CONTINUE], 192));
    assert_eq!(got[4].0, vec![STOP]);

    // A jump while playing: to the start.
    let before = sent(&fake).len();
    lumen.key(Modifiers::NONE, Key::Home);
    until_sent(&mut lumen, &fake, "the jump", |sent| {
        sent[before..].iter().any(|m| m[0] == POSITION && m[1] == 0)
    });
    let after: Vec<Vec<u8>> = sent(&fake)[before..]
        .iter()
        .filter(|m| m[0] != CLOCK)
        .cloned()
        .collect();
    let jump = after
        .iter()
        .position(|m| *m == vec![POSITION, 0, 0])
        .unwrap();
    assert_eq!(after[jump - 1], vec![STOP]);
    assert_eq!(after[jump + 1], vec![CONTINUE]);

    // Stop: the last message is Stop, and no clock follows.
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("stop", |app| !app.is_playing());
    until_sent(&mut lumen, &fake, "Stop", |sent| {
        sent.last().is_some_and(|m| m[0] == STOP)
    });
    let count = sent(&fake).len();
    lumen.frames(30);
    std::thread::sleep(Duration::from_millis(100));
    lumen.frames(5);
    assert_eq!(sent(&fake).len(), count, "nothing after Stop");
}
