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

/// Guides › Clips › Trim and fade inside an audio clip: the lead clip fades
/// the opened clip in, trims its end and lowers its gain in the editor; a
/// gesture clip for each handle and for slip; and a shot with the handles
/// marked.
#[test]
#[ignore = "renders the docs' media; needs a GPU, and ffmpeg with DOCS_MEDIA"]
fn audio_editor_handles() {
    use eframe::egui::{Pos2, vec2};
    use egui_kittest::kittest::Queryable;
    use lumen_app::testing::InputDeviceTurn;
    use lumen_core::time::Ticks;

    type H = Harness<'static, App>;
    fn shown(h: &H) -> lumen_app::testing::AudioEditorShown {
        h.state().audio_editor_shown().expect("drawn")
    }
    fn handles(h: &H) -> lumen_app::testing::AudioHandlesShown {
        shown(h).handles.expect("the handles")
    }
    fn x(h: &H, q: f64) -> f32 {
        let s = shown(h);
        s.view.x(s.body.left(), q)
    }
    fn undo(h: &mut H, steps: usize) {
        for _ in 0..steps {
            h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
            h.run_steps(3);
        }
    }
    /// One drag from `from` to `to` as its own clip; gives the device back.
    #[expect(clippy::too_many_arguments, reason = "one take's whole setting")]
    fn gesture(
        h: &mut H,
        device: InputDeviceTurn,
        (name, theme): (&str, &str),
        editor: Rect,
        rest: Pos2,
        from: Pos2,
        to: Pos2,
        wanted: &str,
    ) -> InputDeviceTurn {
        let mut take = Take::clip(h, device, name, theme, editor, 0.5, rest);
        take.hold(h, pace::RESULT / 2);
        take.move_to(h, from);
        take.press(h);
        take.move_to(h, to);
        take.hold(h, 2);
        take.release(h);
        take.settle(h);
        assert_eq!(h.state().undo_label(), Some(wanted), "{name}");
        take.move_to(h, rest);
        take.result(h);
        take.finish()
    }

    let started = Instant::now();
    for &theme in recorder::themes() {
        let dir = TempDir::new(&format!("docs-audio-handles-{theme}"));
        let (mut h, device) = song(&dir, theme);
        h.set_size(vec2(960.0, 640.0));
        h.run_steps(10);
        // The Drums clip plays bars 2 and 3 of its four-bar file, opened.
        let id = h.state().project().tracks[0].clips[0].id;
        h.state_mut().trim_clip_start_to(id, Ticks::quarters(4));
        h.state_mut()
            .trim_clip_end_to(id, Ticks::quarters(12), false);
        h.state_mut().select_clip(id);
        h.run_steps(4);
        h.key_press(Key::Enter);
        h.run_steps(10);
        let s = shown(&h);
        let editor = Rect::from_min_max(
            pos2(s.body.left() - 1.0, s.body.top() - 70.0),
            pos2(h.ctx.content_rect().right(), s.body.bottom() + 1.0),
        );
        let rest = pos2(editor.right() - 40.0, editor.top() + 20.0);
        let mid = s.body.center().y;

        // The lead: a fade in, the end trimmed by a beat, the gain down.
        let mut take = Take::clip(
            &mut h,
            device,
            "audio-editor-handles-lead",
            theme,
            editor,
            0.5,
            rest,
        );
        take.hold(&mut h, pace::RESULT / 2);
        let fade_in = handles(&h).fade_in;
        let faded = pos2(x(&h, 5.0), fade_in.y);
        take.move_to(&mut h, fade_in);
        take.press(&mut h);
        take.move_to(&mut h, faded);
        take.hold(&mut h, 2);
        take.release(&mut h);
        take.settle(&mut h);
        let end = pos2(handles(&h).end - 2.0, mid);
        let trimmed = pos2(x(&h, 11.0), mid);
        take.move_to(&mut h, end);
        take.press(&mut h);
        take.move_to(&mut h, trimmed);
        take.hold(&mut h, 2);
        take.release(&mut h);
        take.settle(&mut h);
        let line = pos2(x(&h, 8.0), handles(&h).gain_y);
        take.move_to(&mut h, line);
        take.press(&mut h);
        take.move_to(&mut h, line + vec2(0.0, 30.0));
        take.hold(&mut h, 2);
        take.release(&mut h);
        take.settle(&mut h);
        let c = h.state().project().tracks[0].clips[0].clone();
        assert!(c.fade_in.length > Ticks::ZERO, "faded in");
        assert_eq!(
            c.start + c.length,
            Ticks::quarters(11),
            "trimmed on the grid"
        );
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let device = take.finish();
        undo(&mut h, 3);

        // The handles, marked.
        let hs = handles(&h);
        let s = shown(&h);
        let gain_line =
            Rect::from_x_y_ranges(x(&h, 7.0)..=x(&h, 9.0), hs.gain_y - 6.0..=hs.gain_y + 6.0);
        let grip = Rect::from_x_y_ranges(hs.end - 8.0..=hs.end + 4.0, s.body.y_range());
        let fade_mark = Rect::from_center_size(hs.fade_in, vec2(18.0, 18.0));
        h.hover_at(rest);
        still(
            &mut h,
            editor,
            "audio-editor-handles",
            theme,
            None,
            &[
                ("Trim grip", grip),
                ("Fade handle", fade_mark),
                ("Gain line", gain_line),
            ],
        );

        // One gesture a clip, each undone before the next.
        let end = pos2(handles(&h).end - 2.0, mid);
        let to = pos2(x(&h, 10.0), mid);
        let device = gesture(
            &mut h,
            device,
            ("audio-editor-trim", theme),
            editor,
            rest,
            end,
            to,
            "Trim clip",
        );
        undo(&mut h, 1);
        let fade_in = handles(&h).fade_in;
        let to = pos2(x(&h, 6.0), fade_in.y);
        let device = gesture(
            &mut h,
            device,
            ("audio-editor-fade", theme),
            editor,
            rest,
            fade_in,
            to,
            "Fade in",
        );
        let bend = handles(&h).bend_in.expect("a fade has its bend dot");
        let device = gesture(
            &mut h,
            device,
            ("audio-editor-fade-shape", theme),
            editor,
            rest,
            bend,
            bend + vec2(0.0, -40.0),
            "Fade curve",
        );
        undo(&mut h, 2);
        let line = pos2(x(&h, 8.0), handles(&h).gain_y);
        let device = gesture(
            &mut h,
            device,
            ("audio-editor-gain", theme),
            editor,
            rest,
            line,
            line + vec2(0.0, 40.0),
            "Clip gain",
        );
        undo(&mut h, 1);

        // Slip: the whole file shows, and its grey end is dragged.
        h.get_by_label("Whole file").click();
        h.run_steps(6);
        let grey = pos2(x(&h, 14.0), mid);
        let to = pos2(x(&h, 12.0), mid);
        let _device = gesture(
            &mut h,
            device,
            ("audio-editor-slip", theme),
            editor,
            rest,
            grey,
            to,
            "Slip clip",
        );
        undo(&mut h, 1);
    }
    check("audio_editor_handles", started);
}

/// The window width at which the audio editor's strip shows every group,
/// Processing with its words and the Warp chip included.
const STRIP_WIDTH: f32 = 1180.0;

/// Opens the Drums clip (bars 2 and 3 of its four-bar file) in the editor on
/// a 960 x 640 window. Returns the editor's crop and where the pointer rests
/// between gestures.
fn open_drums(h: &mut Harness<'static, App>) -> (Rect, eframe::egui::Pos2) {
    open_drums_in(h, 960.0)
}

/// [`open_drums`] in a window `width` points wide. The strip folds its groups
/// into the ⋯ menu as the width shrinks, so a scene that shows every button
/// asks for the room (the Warp chip of S5a took 960 to fold Processing).
fn open_drums_in(h: &mut Harness<'static, App>, width: f32) -> (Rect, eframe::egui::Pos2) {
    use lumen_core::time::Ticks;
    h.set_size(eframe::egui::vec2(width, 640.0));
    h.run_steps(10);
    let id = h.state().project().tracks[0].clips[0].id;
    h.state_mut().trim_clip_start_to(id, Ticks::quarters(4));
    h.state_mut()
        .trim_clip_end_to(id, Ticks::quarters(12), false);
    h.state_mut().select_clip(id);
    h.run_steps(4);
    h.key_press(Key::Enter);
    h.run_steps(10);
    let s = h.state().audio_editor_shown().expect("drawn");
    let editor = Rect::from_min_max(
        pos2(s.body.left() - 1.0, s.body.top() - 70.0),
        pos2(h.ctx.content_rect().right(), s.body.bottom() + 1.0),
    );
    let rest = pos2(editor.right() - 40.0, editor.top() + 20.0);
    (editor, rest)
}

/// The point at beat `q` and `f` of the wave's height, in the open editor.
fn on_wave(h: &Harness<'static, App>, q: f64, f: f32) -> eframe::egui::Pos2 {
    let s = h.state().audio_editor_shown().expect("drawn");
    pos2(
        s.view.x(s.body.left(), q),
        s.body.top() + s.body.height() * f,
    )
}

/// One undo step.
fn undo_one(h: &mut Harness<'static, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(3);
}

/// Guides > Clips > Pick part of a clip in its editor: the lead picks a
/// range on the wave and splits it; a gesture clip each for the range and the
/// cursor; and a shot with the range marked.
#[test]
#[ignore = "renders the docs' media; needs a GPU, and ffmpeg with DOCS_MEDIA"]
fn audio_editor_range() {
    let started = Instant::now();
    for &theme in recorder::themes() {
        let dir = TempDir::new(&format!("docs-audio-range-{theme}"));
        let (mut h, device) = song(&dir, theme);
        let (editor, rest) = open_drums(&mut h);

        // The lead: a range from beat 6 to 9, split at both edges.
        let mut take = Take::clip(
            &mut h,
            device,
            "audio-editor-range-lead",
            theme,
            editor,
            0.5,
            rest,
        );
        take.hold(&mut h, pace::RESULT / 2);
        let (from, to) = (on_wave(&h, 6.0, 0.5), on_wave(&h, 9.0, 0.5));
        take.move_to(&mut h, from);
        take.press(&mut h);
        take.move_to(&mut h, to);
        take.hold(&mut h, 2);
        take.release(&mut h);
        take.settle(&mut h);
        take.keys(&mut h, "Ctrl+E", |h| {
            h.key_press_modifiers(Modifiers::COMMAND, Key::E);
            h.run_steps(3);
        });
        take.settle(&mut h);
        assert_eq!(h.state().project().tracks[0].clips.len(), 3, "split twice");
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let device = take.finish();
        undo_one(&mut h);
        h.key_press(Key::Escape);
        h.run_steps(3);

        // The range alone: dragged on the wave, no edit made.
        let before = h.state().project().clone();
        let mut take = Take::clip(
            &mut h,
            device,
            "audio-editor-range",
            theme,
            editor,
            0.5,
            rest,
        );
        take.hold(&mut h, pace::RESULT / 2);
        take.move_to(&mut h, from);
        take.press(&mut h);
        take.move_to(&mut h, to);
        take.hold(&mut h, 2);
        take.release(&mut h);
        take.settle(&mut h);
        assert_eq!(*h.state().project(), before, "a range is not an edit");
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let device = take.finish();

        // The shot, with the range still picked.
        let s = h.state().audio_editor_shown().expect("drawn");
        let range = Rect::from_x_y_ranges(from.x..=to.x, s.body.y_range());
        h.hover_at(rest);
        still(
            &mut h,
            editor,
            "audio-editor-range",
            theme,
            None,
            &[("Range", range)],
        );
        h.key_press(Key::Escape);
        h.run_steps(3);

        // The cursor: one click on the wave.
        let mut take = Take::clip(
            &mut h,
            device,
            "audio-editor-cursor",
            theme,
            editor,
            0.5,
            rest,
        );
        take.hold(&mut h, pace::RESULT / 2);
        let seven = on_wave(&h, 7.0, 0.5);
        take.move_to(&mut h, seven);
        take.click(&mut h);
        take.settle(&mut h);
        assert!(h.state().audio_editor_cursor().is_some(), "the cursor");
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let _device = take.finish();
    }
    check("audio_editor_range", started);
}

/// Guides > Clips > Set a clip's gain, fades and processing: the lead
/// reverses the clip and inverts its polarity from the strip; a gesture clip
/// each for the gain slider and the File chip; and a shot with the strip's
/// parts marked.
#[test]
#[ignore = "renders the docs' media; needs a GPU, and ffmpeg with DOCS_MEDIA"]
fn audio_clip_properties() {
    use eframe::egui::vec2;
    use egui_kittest::kittest::Queryable;

    let started = Instant::now();
    for &theme in recorder::themes() {
        let dir = TempDir::new(&format!("docs-audio-properties-{theme}"));
        let (mut h, device) = song(&dir, theme);
        let (editor, _) = open_drums_in(&mut h, STRIP_WIDTH);
        // The strip sits above the ruler: take the header's room as well, from
        // its top edge down (130 points started inside the tab row and cut its
        // labels in half).
        let s = h.state().audio_editor_shown().expect("drawn");
        let editor = Rect::from_min_max(
            pos2(editor.left(), s.body.top() - 114.0),
            pos2(editor.right(), editor.bottom()),
        );
        let rest = pos2(editor.right() - 40.0, editor.top() + 20.0);
        let id = h.state().project().tracks[0].clips[0].id;

        // The lead: Reverse, then Polarity.
        let mut take = Take::clip(
            &mut h,
            device,
            "audio-clip-properties-lead",
            theme,
            editor,
            0.5,
            rest,
        );
        take.hold(&mut h, pace::RESULT / 2);
        for name in ["Reverse", "Polarity"] {
            let at = h.get_by_label(name).rect().center();
            take.move_to(&mut h, at);
            take.click(&mut h);
            take.settle(&mut h);
        }
        let c = h.state().project().tracks[0].clips[0].clone();
        assert!(c.reversed && c.inverted, "both are set");
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let device = take.finish();
        undo_one(&mut h);
        undo_one(&mut h);

        // The gain slider, dragged down.
        let mut take = Take::clip(&mut h, device, "audio-clip-gain", theme, editor, 0.5, rest);
        take.hold(&mut h, pace::RESULT / 2);
        let slider = h.get_by_label_contains("Clip gain: ").rect().center();
        take.move_to(&mut h, slider);
        take.press(&mut h);
        take.move_to(&mut h, slider + vec2(-24.0, 0.0));
        take.hold(&mut h, 2);
        take.release(&mut h);
        take.settle(&mut h);
        assert_eq!(h.state().undo_label(), Some("Clip gain"), "one gain step");
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let device = take.finish();
        undo_one(&mut h);

        // The File chip, opened.
        let mut take = Take::clip(&mut h, device, "audio-clip-file", theme, editor, 0.5, rest);
        take.hold(&mut h, pace::RESULT / 2);
        let chip = h.get_by_label_contains("File: ").rect().center();
        take.move_to(&mut h, chip);
        take.click(&mut h);
        take.settle(&mut h);
        assert!(
            h.query_by_label("Show in file manager").is_some(),
            "the file's facts"
        );
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let _device = take.finish();
        h.key_press(Key::Escape);
        h.run_steps(3);
        assert!(h.state().project().tracks[0].clips[0].id == id);

        // The shot, the strip's parts marked.
        let rect = |h: &Harness<'static, App>, label: &str| h.get_by_label_contains(label).rect();
        let gain = rect(&h, "Clip gain: ");
        let fades = rect(&h, "Fade in: ").union(rect(&h, "Fade out: "));
        let processing = rect(&h, "Reverse").union(rect(&h, "Normalize"));
        let file = rect(&h, "File: ");
        h.hover_at(rest);
        still(
            &mut h,
            editor,
            "audio-clip-properties",
            theme,
            None,
            &[
                ("Gain", gain),
                ("Fades", fades),
                ("Processing", processing),
                ("File", file),
            ],
        );
    }
    check("audio_clip_properties", started);
}

/// Guides > Clips > Draw a clip's envelope: the lead lights Gain and adds
/// points; a gesture clip each for a point, a drawn stroke and the loop
/// brace; and a shot with the pills, the envelope and the brace marked.
#[test]
#[ignore = "renders the docs' media; needs a GPU, and ffmpeg with DOCS_MEDIA"]
fn audio_editor_envelopes() {
    use egui_kittest::kittest::Queryable;

    let started = Instant::now();
    for &theme in recorder::themes() {
        let dir = TempDir::new(&format!("docs-audio-envelopes-{theme}"));
        let (mut h, device) = song(&dir, theme);
        let (editor, rest) = open_drums(&mut h);

        // The lead: Gain is lit, then three points make a dip.
        let mut take = Take::clip(
            &mut h,
            device,
            "audio-editor-envelopes-lead",
            theme,
            editor,
            0.5,
            rest,
        );
        take.hold(&mut h, pace::RESULT / 2);
        let gain_pill = h.get_by_label("Gain envelope").rect().center();
        take.move_to(&mut h, gain_pill);
        take.click(&mut h);
        take.settle(&mut h);
        for (q, f) in [(5.0, 0.3), (8.0, 0.7), (11.0, 0.3)] {
            let p = on_wave(&h, q, f);
            take.move_to(&mut h, p);
            take.click(&mut h);
            take.settle(&mut h);
        }
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let device = take.finish();
        for _ in 0..3 {
            undo_one(&mut h);
        }

        // One point.
        let mut take = Take::clip(
            &mut h,
            device,
            "audio-editor-envelope-point",
            theme,
            editor,
            0.5,
            rest,
        );
        take.hold(&mut h, pace::RESULT / 2);
        let p = on_wave(&h, 8.0, 0.6);
        take.move_to(&mut h, p);
        take.click(&mut h);
        take.settle(&mut h);
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let device = take.finish();
        undo_one(&mut h);

        // A stroke with Draw: B, then drag across.
        h.hover_at(on_wave(&h, 5.0, 0.3));
        h.run_steps(2);
        h.key_press(Key::B);
        h.run_steps(3);
        assert!(h.state().audio_editor_draws(), "B takes the pencil");
        let mut take = Take::clip(
            &mut h,
            device,
            "audio-editor-envelope-draw",
            theme,
            editor,
            0.5,
            rest,
        );
        take.hold(&mut h, pace::RESULT / 2);
        let (a, b, c) = (
            on_wave(&h, 5.0, 0.3),
            on_wave(&h, 8.0, 0.7),
            on_wave(&h, 11.0, 0.35),
        );
        take.move_to(&mut h, a);
        take.press(&mut h);
        take.move_to(&mut h, b);
        take.move_to(&mut h, c);
        take.hold(&mut h, 2);
        take.release(&mut h);
        take.settle(&mut h);
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let device = take.finish();
        undo_one(&mut h);
        h.hover_at(on_wave(&h, 5.0, 0.3));
        h.run_steps(2);
        h.key_press(Key::B);
        h.run_steps(3);

        // The loop brace: one click on its band loops the clip.
        let on_band = |h: &Harness<'static, App>, q: f64| {
            let s = h.state().audio_editor_shown().expect("drawn");
            pos2(s.view.x(s.body.left(), q), s.band.center().y)
        };
        let mut take = Take::clip(
            &mut h,
            device,
            "audio-editor-loop-brace",
            theme,
            editor,
            0.5,
            rest,
        );
        take.hold(&mut h, pace::RESULT / 2);
        let p = on_band(&h, 9.0);
        take.move_to(&mut h, p);
        take.click(&mut h);
        take.settle(&mut h);
        assert_eq!(h.state().undo_label(), Some("Loop audio in clip"));
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let _device = take.finish();

        // The shot: the pills, an envelope drawn, the brace showing.
        for (q, f) in [(5.0, 0.3), (8.0, 0.7), (11.0, 0.3)] {
            let p = on_wave(&h, q, f);
            h.hover_at(p);
            h.run_steps(2);
            h.key_press(Key::Escape);
            h.run_steps(1);
            h.drag_at(p);
            h.run_steps(1);
            h.drop_at(p);
            h.run_steps(3);
        }
        let s = h.state().audio_editor_shown().expect("drawn");
        let pills = h
            .get_by_label("Gain envelope")
            .rect()
            .union(h.get_by_label("Pan envelope").rect());
        let curve = Rect::from_x_y_ranges(
            s.view.x(s.body.left(), 5.0)..=s.view.x(s.body.left(), 11.0),
            s.body.y_range(),
        );
        h.hover_at(rest);
        still(
            &mut h,
            editor,
            "audio-editor-envelopes",
            theme,
            None,
            &[
                ("Gain and Pan", pills),
                ("Envelope", curve),
                ("Loop brace", s.band),
            ],
        );
    }
    check("audio_editor_envelopes", started);
}

/// Guides > Clips > Hear a clip while you edit it: the lead turns Loop clip
/// on and the playhead goes round the clip; a gesture clip of a drag on the
/// editor's ruler; and a shot with the toggle and the ruler marked.
#[test]
#[ignore = "renders the docs' media; needs a GPU, and ffmpeg with DOCS_MEDIA"]
fn audio_editor_hearing() {
    use egui_kittest::kittest::Queryable;

    let started = Instant::now();
    for &theme in recorder::themes() {
        let dir = TempDir::new(&format!("docs-audio-hearing-{theme}"));
        let (mut h, device) = song(&dir, theme);
        let (editor, rest) = open_drums(&mut h);
        let on_ruler = |h: &Harness<'static, App>, q: f64| {
            let s = h.state().audio_editor_shown().expect("drawn");
            pos2(s.view.x(s.body.left(), q), s.band.top() - 6.0)
        };

        // The lead: Loop clip on, and the playhead goes round the clip.
        let mut take = Take::clip(
            &mut h,
            device,
            "audio-editor-hearing-lead",
            theme,
            editor,
            0.5,
            rest,
        );
        take.hold(&mut h, pace::RESULT / 2);
        let toggle = h.get_by_label("Loop clip").rect().center();
        take.move_to(&mut h, toggle);
        take.click(&mut h);
        take.settle(&mut h);
        assert!(h.state().clip_loop_played().is_some(), "Loop clip is on");
        // Play: the playhead goes round the clip, past its end and back.
        take.keys(&mut h, "Space", |h| h.key_press(Key::Space));
        hold_until(&mut take, &mut h, 11.0);
        let before = h.state().playhead();
        for _ in 0..10 * pace::FPS {
            if h.state().playhead() < before {
                break;
            }
            take.hold(&mut h, 1);
        }
        assert!(h.state().playhead() < before, "the playhead wrapped");
        take.hold(&mut h, pace::FPS / 2);
        take.keys(&mut h, "Space", |h| h.key_press(Key::Space));
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let device = take.finish();
        h.get_by_label("Loop clip").click();
        h.run_steps(3);
        assert!(h.state().clip_loop_played().is_none(), "and off again");

        // A drag along the ruler listens as it goes.
        let mut take = Take::clip(
            &mut h,
            device,
            "audio-editor-scrub",
            theme,
            editor,
            0.5,
            rest,
        );
        take.hold(&mut h, pace::RESULT / 2);
        let (a, b) = (on_ruler(&h, 5.0), on_ruler(&h, 10.0));
        take.move_to(&mut h, a);
        take.press(&mut h);
        take.move_to(&mut h, b);
        take.hold(&mut h, 2);
        take.release(&mut h);
        take.settle(&mut h);
        take.move_to(&mut h, rest);
        take.result(&mut h);
        let _device = take.finish();

        // The shot: Loop clip lit, the ruler under the playhead.
        h.get_by_label("Loop clip").click();
        h.run_steps(3);
        let s = h.state().audio_editor_shown().expect("drawn");
        let toggle = h.get_by_label("Loop clip").rect();
        let ruler = Rect::from_min_max(
            pos2(s.body.left(), s.band.top() - 18.0),
            pos2(s.body.right(), s.band.top()),
        );
        h.hover_at(rest);
        still(
            &mut h,
            editor,
            "audio-editor-hearing",
            theme,
            None,
            &[("Loop clip", toggle), ("Ruler", ruler)],
        );
    }
    check("audio_editor_hearing", started);
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
    eprintln!(
        "PHASES {test} ({:.1}s): {}",
        started.elapsed().as_secs_f32(),
        recorder::phases()
    );
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
