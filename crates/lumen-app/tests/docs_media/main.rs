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

/// Guides › Clips › Look inside an audio clip: the lead clip opens a trimmed
/// clip in the Clip tab, shows its whole file and zooms back to the clip;
/// the shots show the whole file with the part the clip plays marked, and
/// the wave zoomed in to its samples.
#[test]
#[ignore = "renders the docs' media; needs a GPU, and ffmpeg with DOCS_MEDIA"]
fn audio_clip_editor() {
    use egui_kittest::kittest::Queryable;
    use lumen_core::time::Ticks;

    let started = Instant::now();
    for &theme in recorder::themes() {
        let dir = TempDir::new(&format!("docs-audio-editor-{theme}"));
        let (mut h, device) = song(&dir, theme);
        // Tall enough for the tracks above the detail panel.
        h.set_size(eframe::egui::vec2(960.0, 640.0));
        h.run_steps(10);
        // The Drums clip plays bars 2 and 3 of its four-bar file.
        let id = h.state().project().tracks[0].clips[0].id;
        h.state_mut().trim_clip_start_to(id, Ticks::quarters(4));
        h.state_mut()
            .trim_clip_end_to(id, Ticks::quarters(12), false);
        h.run_steps(10);
        let drums = clip(&h, 0, 0);
        let crop = Rect::from_min_max(
            pos2(0.0, arrangement(&h).top()),
            h.ctx.content_rect().right_bottom(),
        );
        let rest = pos2(crop.right() - 60.0, crop.top() + 90.0);

        let mut take = Take::clip(&mut h, device, "audio-editor-lead", theme, crop, 0.5, rest);
        take.hold(&mut h, pace::RESULT / 2);
        take.move_to(&mut h, pos2(drums.center().x, drums.bottom() - 8.0));
        take.click(&mut h);
        take.settle(&mut h);
        take.keys(&mut h, &caps("midi.open_editor"), |h| {
            h.key_press(Key::Enter)
        });
        take.settle(&mut h);
        let opened = h
            .state()
            .audio_editor_shown()
            .expect("Enter opens the editor");
        assert_eq!(opened.window, (4.0, 12.0), "the clip plays bars 2 and 3");
        take.hold(&mut h, pace::READ);

        let whole = h.get_by_label("Whole file").rect().center();
        take.move_to(&mut h, whole);
        take.click(&mut h);
        take.settle(&mut h);
        let shown = h.state().audio_editor_shown().expect("drawn");
        assert!(shown.view.zoom < opened.view.zoom, "Whole file zooms out");
        take.hold(&mut h, pace::READ);

        let to_clip = h.get_by_label("Zoom to clip").rect().center();
        take.move_to(&mut h, to_clip);
        take.click(&mut h);
        take.settle(&mut h);
        let back = h.state().audio_editor_shown().expect("drawn");
        assert!(
            (back.view.zoom - opened.view.zoom).abs() < 1e-9,
            "back on the clip"
        );
        take.move_to(&mut h, pos2(crop.right() - 60.0, back.body.top() - 60.0));
        take.result(&mut h);
        take.finish();

        // The whole file, with the part the clip plays and the rest marked.
        h.get_by_label("Whole file").click();
        h.run_steps(6);
        let shown = h.state().audio_editor_shown().expect("drawn");
        let x = |q: f64| shown.view.x(shown.body.left(), q);
        let band =
            |from: f64, to: f64| Rect::from_x_y_ranges(x(from)..=x(to), shown.body.y_range());
        // The editor alone: its header, ruler and wave.
        let editor = Rect::from_min_max(
            pos2(shown.body.left() - 1.0, shown.body.top() - 70.0),
            pos2(crop.right(), shown.body.bottom() + 1.0),
        );
        h.hover_at(pos2(crop.left() + 8.0, crop.top() + 8.0));
        still(
            &mut h,
            editor,
            "audio-editor-whole",
            theme,
            None,
            &[
                ("The part the clip plays", band(4.0, 12.0)),
                ("The rest of the file", band(12.0, 16.0)),
            ],
        );

        // Zoomed in on a kick's start until the wave is drawn through its
        // samples, then one step further.
        let on_wave = pos2(x(8.0) + 1.0, shown.body.center().y);
        h.hover_at(on_wave);
        for _ in 0..40 {
            if h.state().audio_editor_shown().is_some_and(|s| s.samples) {
                break;
            }
            h.key_press(Key::Plus);
            h.run_steps(2);
        }
        let deep = h.state().audio_editor_shown().expect("drawn");
        assert!(deep.samples, "the zoom keys reach the samples");
        h.key_press(Key::Plus);
        h.run_steps(2);
        still(&mut h, editor, "audio-editor-samples", theme, None, &[]);
    }
    check("audio_clip_editor", started);
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
