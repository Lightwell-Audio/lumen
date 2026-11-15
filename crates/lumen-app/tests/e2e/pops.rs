use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;
use lumen_project::model::{Id, Marker, Note, Project};
use lumen_project::testing::{TempDir, midi_track, project_of};

use super::Lumen;

fn q(beats: i64) -> Ticks {
    Ticks::quarters(beats)
}

fn chord(start: i64, length: i64, pitches: [u8; 3]) -> Vec<Note> {
    pitches
        .into_iter()
        .map(|pitch| Note::new(q(start), q(length), pitch, 100))
        .collect()
}

fn song(preset: &str, pitches: [u8; 3]) -> Project {
    let mut notes = chord(0, 2, pitches);
    notes.extend(chord(3, 20, pitches));
    notes.extend(chord(24, 20, pitches));
    notes.extend(chord(46, 18, pitches));
    let mut project = project_of(
        "Pops",
        vec![midi_track(
            1,
            "Chords",
            preset,
            (0, 64),
            None,
            notes,
            vec![],
        )],
    );
    project.tempo[0].bpm = 480.0;
    for (n, beat) in [6, 10, 14, 20, 26, 30, 36, 40].into_iter().enumerate() {
        project.markers.push(Marker {
            id: Id(900 + n as u64),
            at: q(beat),
            name: format!("Locator {}", n + 1),
        });
    }
    project
}

fn launch(name: &str, project: &Project) -> Lumen {
    let dir = TempDir::new(&format!("e2e-{name}"));
    let path = dir.path().join("song.lumen");
    lumen_project::save(project, &path, &Default::default()).expect("save the test project");
    let tracks = project.tracks.len();
    let mut lumen = Lumen::start_with(dir, move |l| l.opening(path));
    lumen.wait_for("the project", move |app| {
        app.project().tracks.len() == tracks
    });
    lumen.frames(30);
    lumen
}

fn play_through(
    lumen: &mut Lumen,
    parts: usize,
    frames: usize,
    mut between: impl FnMut(&mut Lumen),
) -> Vec<f32> {
    lumen.frames(1);
    lumen.app_mut().heard_all();
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.is_playing());
    let mut heard: Vec<f32> = Vec::new();
    for part in 0..parts {
        let until = (part + 1) * frames;
        let started = std::time::Instant::now();
        while heard.len() < until {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(30),
                "heard {} of {until} frames",
                heard.len()
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
            lumen.frames(1);
            heard.extend(lumen.app_mut().heard_all().iter().map(|f| f[0]));
        }
        between(lumen);
    }
    heard
}

fn steepest(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

fn straight(name: &str, song: Project) -> f32 {
    let mut lumen = launch(name, &song);
    let heard = play_through(&mut lumen, 1, 48_000, |_| {});
    let steepest = steepest(&heard[1_000..]);
    assert!(steepest > 0.01, "the song sounds: {steepest}");
    steepest
}

#[test]
fn jumping_between_locators_while_a_chord_sounds_does_not_pop() {
    let straight = straight("pops-straight-jump", song("poly", [60, 64, 67]));
    let mut lumen = launch("pops-jump", &song("poly", [60, 64, 67]));
    let heard = play_through(&mut lumen, 9, 5_000, |lumen| {
        lumen.key(Modifiers::COMMAND, Key::ArrowRight);
    });
    let worst = steepest(&heard[1_000..]);
    assert!(
        worst <= straight * 1.4,
        "a jump popped: steepest step {worst}, straight {straight}"
    );
}

#[test]
fn a_loop_wrap_while_a_chord_sounds_does_not_pop() {
    let bass = || song("bass", [33, 40, 45]);
    let straight = straight("pops-straight-loop", bass());
    let mut project = bass();
    project.transport.loop_start = Ticks::ZERO;
    project.transport.loop_end = q(4);
    project.transport.loop_enabled = true;
    let mut lumen = launch("pops-loop", &project);
    let heard = play_through(&mut lumen, 1, 130_000, |_| {});
    let worst = steepest(&heard[1_000..]);
    assert!(
        worst <= straight * 1.1,
        "a wrap popped: steepest step {worst}, straight {straight}"
    );
}
