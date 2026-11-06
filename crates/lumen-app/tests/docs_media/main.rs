//! Renders the user documentation's screenshots, clips and walkthroughs
//! from the real app (docs/USER_DOCS.md §6). Not a test of behaviour: it is
//! ignored, needs a GPU (software Vulkan is enough) and `ffmpeg`, and runs
//! one scene at a time because the theme is process-wide:
//!
//! ```text
//! DOCS_MEDIA=site/docs/static/media cargo test -p lumen-app --test docs_media -- --ignored --test-threads=1
//! ```
//!
//! Without `DOCS_MEDIA` the scenes only check that they still reach their
//! states, in one theme, writing nothing (§8.3; CI's docs job).
//!
//! Each scene is a test named as in `tests/shared/scenes.rs`, and checks
//! that it wrote exactly what that table says: a page can only show what a
//! scene makes (`tests/docs_coverage.rs`). A scene drives the app the way
//! its page tells the reader to, so a page whose steps stop working fails
//! here.

#[path = "../shared/scenes.rs"]
mod scenes;

mod pointer;
mod recorder;
mod song;

use std::time::Instant;

use eframe::egui::{Key, Modifiers, Rect, pos2};
use egui_kittest::Harness;
use lumen_app::App;
use lumen_app::testing::docs;
use lumen_project::testing::TempDir;

use recorder::{Take, pace, still};
use song::song;

/// Guides › Editing › Split a clip: the lead clip, and the before and after
/// screenshots with the two halves marked.
#[test]
#[ignore = "renders the docs' media; needs a GPU, and ffmpeg with DOCS_MEDIA"]
fn split_a_clip() {
    let started = Instant::now();
    for &theme in recorder::themes() {
        let dir = TempDir::new(&format!("docs-split-{theme}"));
        let (mut h, device) = song(&dir, theme);
        let drums = clip(&h, 0, 0);
        let crop = arrangement(&h);
        let cut = pos2(drums.left() + drums.width() * 0.5, drums.bottom() - 8.0);

        // Before: the clip, the pointer resting where the cut will go.
        let rest = pos2(crop.right() - 60.0, crop.bottom() - 24.0);
        let mut take = Take::clip(&mut h, device, "split-lead", theme, crop, 0.62, rest);
        take.hold(&mut h, pace::RESULT / 2);
        take.move_to(&mut h, cut);
        take.click(&mut h);
        take.settle(&mut h);
        still(&mut h, crop, "split-before", theme, Some(cut), &[]);
        take.keys(&mut h, &caps("midi.split_notes"), |h| {
            h.key_press_modifiers(Modifiers::COMMAND, Key::E);
        });
        assert_eq!(
            h.state().project().tracks[0].clips.len(),
            2,
            "Ctrl+E split the clip"
        );
        take.hold(&mut h, pace::KEYS);
        take.move_to(&mut h, pos2(cut.x + 170.0, drums.bottom() - 10.0));
        take.result(&mut h);
        take.finish();
        h.hover_at(pos2(crop.left() + 8.0, crop.bottom() + 40.0));
        let halves = [
            ("First half", clip(&h, 0, 0)),
            ("Second half", clip(&h, 0, 1)),
        ];
        still(&mut h, crop, "split-after", theme, None, &halves);
    }
    check("split_a_clip", started);
}

/// The Split tool's gesture: a click cuts the clip there, on the grid.
#[test]
#[ignore = "renders the docs' media; needs a GPU, and ffmpeg with DOCS_MEDIA"]
fn split_tool() {
    let started = Instant::now();
    for &theme in recorder::themes() {
        let dir = TempDir::new(&format!("docs-split-tool-{theme}"));
        let (mut h, device) = song(&dir, theme);
        let bass = clip(&h, 1, 0);
        let crop = arrangement(&h);
        let rest = pos2(crop.right() - 60.0, crop.top() + 30.0);
        let mut take = Take::clip(&mut h, device, "split-tool", theme, crop, 0.4, rest);
        take.hold(&mut h, pace::SETTLE);
        take.keys(&mut h, &caps("view.tool_split"), |h| h.key_press(Key::C));
        take.settle(&mut h);
        // Aim: the cut line follows the pointer, snapped to the grid.
        let aim = pos2(bass.left() + bass.width() * 0.27, bass.center().y);
        take.move_to(&mut h, aim);
        take.move_to(&mut h, aim + eframe::egui::vec2(16.0, 0.0));
        take.settle(&mut h);
        take.click(&mut h);
        assert_eq!(
            h.state().project().tracks[1].clips.len(),
            2,
            "the click cut the clip"
        );
        take.settle(&mut h);
        take.keys(&mut h, "Esc", |h| h.key_press(Key::Escape));
        take.move_to(&mut h, pos2(aim.x + 120.0, crop.bottom() - 16.0));
        take.result(&mut h);
        take.finish();
    }
    check("split_tool", started);
}

/// A walkthrough with sound: the song plays, and Ctrl+E on bar 3 cuts the
/// selected clip without stopping it.
#[test]
#[ignore = "renders the docs' media; needs a GPU, and ffmpeg with DOCS_MEDIA"]
fn split_while_playing() {
    let started = Instant::now();
    let dir = TempDir::new("docs-split-playing");
    let (mut h, device) = song(&dir, "dark");
    let drums = clip(&h, 0, 0);
    let crop = arrangement(&h);
    // Near the clip's start, where the click also puts the playhead, and
    // clear of its trim handle.
    let start = pos2(drums.left() + drums.width() * 0.05, drums.bottom() - 8.0);
    let rest = pos2(crop.right() - 60.0, crop.bottom() - 24.0);
    let mut take = Take::walk(&mut h, device, "split-playing", crop, rest);
    take.chapter("with-the-keyboard", "Split while it plays");
    take.caption("Click the Drums clip to select it.");
    take.settle(&mut h);
    take.move_to(&mut h, start);
    take.click(&mut h);
    take.settle(&mut h);
    take.move_to(&mut h, rest);
    take.caption("Press Space to play the song.");
    take.settle(&mut h);
    take.keys(&mut h, &caps("transport.play_stop"), |h| {
        h.key_press(Key::Space)
    });
    // Time to read the caption before the next one.
    take.result(&mut h);
    assert!(h.state().is_playing(), "Space plays the song");
    take.caption("On bar 3, press Ctrl+E. The clip is cut and the song plays on.");
    // The keys go down a beat before bar 3 shows, so they land on it.
    let lead = pace::KEYS_LEAD as f64 / pace::FPS as f64 * 2.0;
    hold_until(&mut take, &mut h, 8.0 - lead);
    take.keys(&mut h, &caps("midi.split_notes"), |h| {
        h.key_press_modifiers(Modifiers::COMMAND, Key::E);
    });
    assert_eq!(
        h.state().project().tracks[0].clips.len(),
        2,
        "Ctrl+E split the clip"
    );
    assert!(h.state().is_playing(), "the song plays on");
    hold_until(&mut take, &mut h, 13.0);
    take.caption("Space stops it.");
    take.keys(&mut h, &caps("transport.play_stop"), |h| {
        h.key_press(Key::Space)
    });
    take.result(&mut h);
    take.finish();
    check("split_while_playing", started);
}

/// Films frames until the playhead reaches `quarters`, within ten seconds
/// of video: a song that stopped or stalled fails the scene instead of
/// filming forever.
fn hold_until(take: &mut Take, h: &mut Harness<'static, App>, quarters: f64) {
    for _ in 0..10 * pace::FPS {
        if h.state().playhead().as_quarters() >= quarters {
            return;
        }
        take.hold(h, 1);
    }
    panic!(
        "the playhead stayed at {} quarters, short of {quarters}",
        h.state().playhead().as_quarters()
    );
}

/// A command's first default shortcut on Windows, as keycaps show it:
/// "Ctrl+E". Read from the keymap, never written by hand.
fn caps(id: &str) -> String {
    docs::commands()
        .into_iter()
        .find(|c| c.id == id)
        .and_then(|c| c.keys.first().map(|k| k.join("+")))
        .unwrap_or_else(|| panic!("{id} has a shortcut"))
}

/// Clip `index` of track `track`, on screen.
fn clip(h: &Harness<'static, App>, track: usize, index: usize) -> Rect {
    let id = h.state().project().tracks[track].clips[index].id;
    h.state().clip_rect(&h.ctx, id).expect("in view")
}

/// The arrangement around the song's clips: the ruler, the track headers
/// and the lanes, without the transport bar (whose live CPU reading means
/// nothing in a picture).
fn arrangement(h: &Harness<'static, App>) -> Rect {
    let first = clip(h, 0, 0);
    let last = clip(h, h.state().project().tracks.len() - 1, 0);
    let width = h.ctx.content_rect().width();
    Rect::from_min_max(
        pos2(0.0, first.top() - 25.0),
        pos2(width, last.bottom() + 44.0),
    )
}

/// Checks that scene `test` wrote exactly what `scenes.rs` lists, and
/// marked the controls it lists (both also when the scene only checks), and
/// says how long it took and how big its files are (§6.4).
fn check(test: &str, started: Instant) {
    let scene = scenes::SCENES
        .iter()
        .find(|s| s.test == test)
        .unwrap_or_else(|| panic!("{test} is listed in tests/shared/scenes.rs"));
    let expected = scene.files(recorder::themes());
    let written = recorder::written();
    let missing: Vec<_> = expected.difference(&written).collect();
    let unlisted: Vec<_> = written.difference(&expected).collect();
    assert!(
        missing.is_empty() && unlisted.is_empty(),
        "{test}: scenes.rs lists {missing:?} but the scene did not write them; \
         it wrote {unlisted:?}, which scenes.rs does not list"
    );
    for (shot, names) in recorder::marked() {
        let listed: Vec<String> = scenes::callouts(&shot)
            .iter()
            .map(|n| (*n).to_owned())
            .collect();
        assert_eq!(
            names, listed,
            "{test}: the controls marked on {shot}, against scenes.rs"
        );
    }
    if !recorder::rendering() {
        eprintln!(
            "{test}: checked in {:.1} s",
            started.elapsed().as_secs_f64()
        );
        return;
    }
    let dir = recorder::media_dir();
    let mut total = 0;
    for name in &written {
        let size = std::fs::metadata(dir.join(name)).map_or(0, |m| m.len());
        total += size;
        eprintln!("  {name}: {} KB", size.div_ceil(1024));
    }
    eprintln!(
        "{test}: {} files, {} KB, in {:.1} s",
        written.len(),
        total.div_ceil(1024),
        started.elapsed().as_secs_f64()
    );
}
