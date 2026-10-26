//! M9 slice R4 (ADR-0105): joined, bounced and frozen audio is made at the
//! project's sample rate (Project settings), whatever the audio device runs
//! at. The test device runs at 48 kHz.

use std::sync::atomic::AtomicBool;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::PPQ;
use lumen_project::model::{ClipContent, Media};

use super::Lumen;
use super::clip_commands::{audio, command};

/// An audio track with a clip, in a project set to 44.1 kHz in its settings
/// window, as a person would set it.
fn at_44_1(name: &str) -> Lumen {
    let mut lumen = audio(name);
    command(&mut lumen, "Project settings");
    lumen.run_until("the window", |l| l.shows("Sample rate: 48.0 kHz"));
    lumen.click("Sample rate: 48.0 kHz");
    lumen.run_until("the list", |l| l.shows("44.1 kHz"));
    lumen.click("44.1 kHz");
    lumen.wait_for("the rate", |app| app.project().sample_rate == 44_100);
    lumen.key(Modifiers::NONE, Key::Escape);
    lumen.frames(5);
    lumen
}

/// The file `media` names, decoded.
fn decoded(media: &Media) -> lumen_media::Audio {
    lumen_media::decode(
        std::path::Path::new(&media.path),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("the rendered file reads back")
}

/// What the first clip of the first track plays.
fn clip_media(lumen: &Lumen, track: usize) -> Media {
    let ClipContent::Audio { media, .. } = lumen.app().project().tracks[track].clips[0].content
    else {
        panic!("an audio clip")
    };
    lumen
        .app()
        .project()
        .media
        .iter()
        .find(|m| m.id == media)
        .expect("the clip's media is in the project")
        .clone()
}

/// A clip of `ticks` is this many seconds long at 120 BPM.
fn seconds(ticks: i64) -> f64 {
    ticks as f64 / PPQ as f64 * 0.5
}

#[test]
fn a_join_is_made_at_the_project_rate_and_is_as_long_as_the_clip() {
    let mut lumen = at_44_1("render-rate-join");
    let clip = lumen.app().project().tracks[0].clips[0].clone();
    lumen.app_mut().select_clip(clip.id);
    command(&mut lumen, "Join into new file");
    lumen.wait_for("the join", |app| {
        app.undo_label() == Some("Join into new file")
    });

    let media = clip_media(&lumen, 0);
    assert_eq!(media.sample_rate, 44_100);
    let file = decoded(&media);
    assert_eq!(file.sample_rate, 44_100, "the file itself is at that rate");
    assert_eq!(file.frames() as u64, media.frames);
    let expected = seconds(clip.length.0) * 44_100.0;
    assert!(
        (media.frames as f64 - expected).abs() <= 2.0,
        "{} frames for {expected}",
        media.frames
    );
    assert!(
        file.channel(0).iter().any(|v| v.abs() > 0.01),
        "the file has the sound in it"
    );
    // The clip still starts and ends where it did on the timeline.
    let joined = &lumen.app().project().tracks[0].clips[0];
    assert_eq!((joined.start, joined.length), (clip.start, clip.length));
}

#[test]
fn a_bounce_is_made_at_the_project_rate() {
    let mut lumen = at_44_1("render-rate-bounce");
    let clip = lumen.app().project().tracks[0].clips[0].clone();
    lumen.app_mut().select_clip(clip.id);
    command(&mut lumen, "Bounce");
    lumen.frames(3);
    assert!(lumen.shows("Include effects"), "the popover is open");
    lumen.key(Modifiers::NONE, Key::Enter);
    lumen.wait_for("the bounce", |app| app.undo_label() == Some("Bounce"));

    // The bounce went to a new track below.
    let media = clip_media(&lumen, 1);
    assert_eq!(media.sample_rate, 44_100);
    assert_eq!(decoded(&media).sample_rate, 44_100);
    let bounced = &lumen.app().project().tracks[1].clips[0];
    assert_eq!(bounced.start, clip.start);
    assert!(
        bounced.length >= clip.length,
        "it holds the clip, and a tail"
    );
}

#[test]
fn a_freeze_is_made_at_the_project_rate() {
    let mut lumen = at_44_1("render-rate-freeze");
    let track = lumen.app().project().tracks[0].id;
    lumen.app_mut().select_tracks(&[track]);
    command(&mut lumen, "Freeze track");
    lumen.wait_for("the freeze", |app| app.undo_label() == Some("Freeze"));

    let project = lumen.app().project();
    let frozen = project.tracks[0].frozen.as_ref().expect("frozen");
    let media = project
        .media
        .iter()
        .find(|m| m.id == frozen.media)
        .expect("the frozen file is in the project")
        .clone();
    assert_eq!(media.sample_rate, 44_100);
    assert_eq!(decoded(&media).sample_rate, 44_100);
}

#[test]
fn a_project_at_the_device_rate_still_renders_at_it() {
    let mut lumen = audio("render-rate-same");
    assert_eq!(lumen.app().project().sample_rate, 48_000);
    let id = lumen.app().project().tracks[0].clips[0].id;
    lumen.app_mut().select_clip(id);
    command(&mut lumen, "Join into new file");
    lumen.wait_for("the join", |app| {
        app.undo_label() == Some("Join into new file")
    });
    let media = clip_media(&lumen, 0);
    assert_eq!(media.sample_rate, 48_000);
    assert_eq!(decoded(&media).sample_rate, 48_000);
}
