//! The M5 exit walkthrough (slice 8): record a MIDI part, quantize it in the
//! piano roll, save and reopen the project, then play the reopened notes.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_app::TimeView;
use lumen_midi::{Direction, fake::FakeControl};
use lumen_project::model::{ClipContent, Color, Device, Id, MidiTo, Track, TrackKind};
use lumen_project::testing::project_of;

use super::{Lumen, midi_play, record};

const GRID: i64 = lumen_core::time::PPQ / 4;

#[test]
fn record_quantize_save_reopen_and_play_the_same_midi_part() {
    let fake = record::keyboard();
    fake.plug(Direction::Output, "Synth");
    let mut track = Track::new(Id(1), "Keys", TrackKind::Midi, Color(0x4F81BD));
    track.devices = vec![Device::synth(Id(2), "poly")];
    track.armed = true;
    track.midi_to = Some(MidiTo {
        port: "Synth".into(),
        channel: 0,
    });
    let project = project_of("MIDI Walkthrough", vec![track]);
    let mut lumen = record::launch("m5-exit", &project, &fake);
    midi_play::wait_open(&mut lumen, &fake);
    let keys = record::Keys::new(&fake);

    record::record(&mut lumen);
    record::reach(&mut lumen, record::beats(1.0));
    keys.on(60, 96);
    record::reach(&mut lumen, record::beats(1.5));
    keys.off(60);
    record::reach(&mut lumen, record::beats(2.0));
    keys.on(64, 108);
    record::reach(&mut lumen, record::beats(2.5));
    keys.off(64);
    record::reach(&mut lumen, record::beats(3.5));
    record::stop(&mut lumen);

    let clip = record::clips_of(&lumen)
        .into_iter()
        .find(|clip| matches!(&clip.content, ClipContent::Midi { .. }))
        .expect("the recording made a MIDI clip");
    let ctx = lumen.harness.ctx.clone();
    let rect = lumen
        .app()
        .clip_rect(&ctx, clip.id)
        .expect("the recorded clip is visible");
    lumen.double_click(rect.center());
    lumen.frames(20);
    assert_eq!(lumen.app().editor_open_clip(), Some(clip.id));

    lumen.app_mut().set_detail_size(560.0);
    lumen.app_mut().set_view(TimeView {
        left: 0.0,
        zoom: 60.0,
    });
    lumen.frames(10);
    for _ in 0..6 {
        if lumen.shows("Grid: 1/16") {
            break;
        }
        lumen.key(Modifiers::COMMAND, Key::Num1);
    }
    assert!(
        lumen.shows("Grid: 1/16"),
        "the piano roll is on a 1/16 grid"
    );
    lumen.key(Modifiers::COMMAND, Key::Num5);
    lumen.key(Modifiers::NONE, Key::Q);
    let recorded_id = clip.id;
    let quantized = record::clips_of(&lumen)
        .into_iter()
        .find(|candidate| candidate.id == recorded_id)
        .expect("the recorded clip remains open");
    let notes = record::placed(&quantized);
    assert_eq!(notes.len(), 2, "both recorded notes remain");
    assert!(
        notes.iter().all(|note| note.start.0 % GRID == 0),
        "Q places both note starts on the 1/16 grid: {notes:?}"
    );

    let path = lumen_app::testing::project_file(lumen.dir(), "MIDI Walkthrough");
    lumen.app_mut().save_to(path.clone());
    lumen.wait_for("the save", |app| !app.is_busy() && !app.is_dirty());
    let saved = lumen.app().project_as_saved();
    let saved_notes = saved.tracks[0]
        .clips
        .iter()
        .flat_map(|clip| record::placed(clip))
        .collect::<Vec<_>>();
    assert_eq!(
        saved_notes, notes,
        "the saved project has the quantized part"
    );

    let dir = lumen.crash();
    let reopened_fake = fake.clone();
    let mut reopened = Lumen::start_with(dir, move |launch| {
        launch.opening(path.clone()).with_midi(reopened_fake)
    });
    reopened.wait_for("the project to reopen", |app| !app.is_busy());
    reopened.frames(5);
    assert_eq!(reopened.app().project().tracks, saved.tracks);
    let reopened_notes = reopened.app().project().tracks[0]
        .clips
        .iter()
        .flat_map(|clip| record::placed(clip))
        .collect::<Vec<_>>();
    assert_eq!(
        reopened_notes, saved_notes,
        "the reopened part is unchanged"
    );

    let sent_before_playback = fake.sent().len();
    reopened.key(Modifiers::NONE, Key::Home);
    reopened.frames(3);
    reopened.key(Modifiers::NONE, Key::Space);
    reopened.wait_for("playback", |app| app.is_playing());
    for _ in 0..1_000 {
        reopened.frames(1);
        if fake
            .sent()
            .iter()
            .skip(sent_before_playback)
            .any(|(_, bytes)| bytes.len() == 3 && bytes[0] & 0xF0 == 0x90 && bytes[2] > 0)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        fake.sent()
            .iter()
            .skip(sent_before_playback)
            .any(|(_, bytes)| { bytes.len() == 3 && bytes[0] & 0xF0 == 0x90 && bytes[2] > 0 }),
        "the reopened MIDI part plays through the fake output"
    );
}
