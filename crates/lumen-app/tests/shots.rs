//! Renders states a `--screenshot` cannot reach (hovers, drags, selections)
//! to PNG files, for checking the look by eye. Not a test of behaviour: it
//! is ignored, and runs on demand with a GPU, one test at a time (the
//! theme is process-wide, so tests run side by side render mixed themes):
//!
//! ```text
//! SHOTS=<folder> cargo test -p lumen-app --test shots -- --ignored --test-threads=1
//! ```

use std::path::PathBuf;

use eframe::egui::{Event, Key, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use lumen_app::{App, Launch};
use lumen_project::testing::TempDir;

/// A window with two one-second clips on a one-bar grid.
fn lumen(dir: &TempDir) -> Harness<'static, App> {
    lumen_looking(dir, "dark", 1.0)
}

/// [`lumen`] in `appearance`, at `scale`: tall enough for the tracks with
/// the mixer open (at 420 points the dropped files found no lanes).
fn lumen_looking(dir: &TempDir, appearance: &str, scale: f32) -> Harness<'static, App> {
    lumen_looking_sized(dir, appearance, scale, vec2(1280.0, 760.0))
}

/// [`lumen_looking`] in a window of `size`.
fn lumen_looking_sized(
    dir: &TempDir,
    appearance: &str,
    scale: f32,
    size: eframe::egui::Vec2,
) -> Harness<'static, App> {
    let launch = Launch::for_test(dir.path()).appearing(appearance);
    let mut h = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(scale)
        .with_theme(if appearance == "light" {
            eframe::egui::Theme::Light
        } else {
            eframe::egui::Theme::Dark
        })
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .build_eframe(|cc| {
            lumen_app::testing::install_renderer(cc).expect("the GPU renderer");
            App::new(cc, launch)
        });
    h.run_steps(10);
    drop_files(&mut h, dir, 1.5);
    h
}

/// A window in `appearance` with no files yet.
fn bare(dir: &TempDir, appearance: &str, size: eframe::egui::Vec2) -> Harness<'static, App> {
    bare_scaled(dir, appearance, 1.0, size)
}

/// [`bare`] at `scale`.
fn bare_scaled(
    dir: &TempDir,
    appearance: &str,
    scale: f32,
    size: eframe::egui::Vec2,
) -> Harness<'static, App> {
    let launch = Launch::for_test(dir.path()).appearing(appearance);
    let mut h = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(scale)
        .with_theme(if appearance == "light" {
            eframe::egui::Theme::Light
        } else {
            eframe::egui::Theme::Dark
        })
        .with_step_dt(1.0 / 60.0)
        .wgpu()
        .build_eframe(|cc| {
            lumen_app::testing::install_renderer(cc).expect("the GPU renderer");
            App::new(cc, launch)
        });
    h.run_steps(10);
    h
}

/// Drops three tones `seconds` long on new tracks.
fn drop_files(h: &mut Harness<'static, App>, dir: &TempDir, seconds: f32) {
    let files: Vec<PathBuf> = ["kick", "snare", "bass"]
        .iter()
        .map(|name| wav(&dir.path().join(format!("{name}.wav")), seconds))
        .collect();
    h.input_mut().dropped_files = files
        .into_iter()
        .map(|p| std::sync::Arc::new(Dropped(p)) as eframe::egui::DroppedFileHandle)
        .collect();
    h.run_steps(60);
}

fn clip(h: &Harness<'static, App>, track: usize) -> Rect {
    let id = h.state().project().tracks[track].clips[0].id;
    h.state().clip_rect(&h.ctx, id).expect("in view")
}

fn save(h: &mut Harness<'static, App>, name: &str) {
    h.run_steps(3);
    let image = h.render().expect("render");
    let dir = PathBuf::from(std::env::var("SHOTS").expect("SHOTS names a folder"));
    image.save(dir.join(format!("{name}.png"))).expect("save");
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn clip_selection() {
    let dir = TempDir::new("shots-clips");
    let mut h = lumen(&dir);
    let a = clip(&h, 0);
    h.hover_at(Pos2::new(a.left() + 20.0, a.top() + 5.0));
    save(&mut h, "clip-hover-top");
    h.hover_at(Pos2::new(a.left() + 20.0, a.bottom() - 5.0));
    save(&mut h, "clip-hover-bottom");

    // Select two by a marquee, held mid-drag.
    let b = clip(&h, 1);
    let from = Pos2::new(a.right() + 200.0, a.top() + 2.0);
    h.hover_at(from);
    h.drag_at(from);
    h.run_steps(2);
    h.hover_at(Pos2::new(a.left() + 30.0, b.bottom() - 4.0));
    save(&mut h, "clip-marquee");
    h.drop_at(Pos2::new(a.left() + 30.0, b.bottom() - 4.0));
    h.run_steps(2);
    h.hover_at(Pos2::new(a.right() + 300.0, b.bottom() + 60.0));
    save(&mut h, "clip-selected");

    // A range over the three tracks.
    let c = clip(&h, 2);
    let from = Pos2::new(a.left() + 10.0, a.bottom() - 5.0);
    h.hover_at(from);
    h.drag_at(from);
    h.run_steps(2);
    h.hover_at(Pos2::new(a.right() - 10.0, c.bottom() - 5.0));
    h.run_steps(2);
    h.drop_at(Pos2::new(a.right() - 10.0, c.bottom() - 5.0));
    h.hover_at(Pos2::new(a.right() + 300.0, c.bottom() + 60.0));
    save(&mut h, "clip-range");
    h.key_press(Key::Escape);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn clip_handles() {
    let dir = TempDir::new("shots-handles");
    let mut h = lumen(&dir);
    let a = clip(&h, 0);
    h.hover_at(Pos2::new(a.right() - 2.0, a.center().y));
    save(&mut h, "handle-edge");
    // Shift+drag the end four times as far: a looping clip.
    let from = Pos2::new(a.right() - 2.0, a.center().y);
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    h.hover_at(from);
    h.drag_at(from);
    h.run_steps(2);
    let to = from + vec2(a.width() * 3.0, 0.0);
    h.hover_at(to);
    h.run_steps(2);
    h.drop_at(to);
    h.run_steps(2);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.hover_at(Pos2::new(to.x + 300.0, a.bottom() + 120.0));
    save(&mut h, "handle-looped");
}

/// Drags with the left button from `from` to `to`, holding `modifiers`.
fn drag(h: &mut Harness<'static, App>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.drag_at(from);
    h.run_steps(2);
    for step in 1..=6 {
        h.hover_at(from + (to - from) * (step as f32 / 6.0));
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(2);
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn clip_fades_and_gain() {
    let dir = TempDir::new("shots-fades");
    let mut h = lumen(&dir);
    // Close enough that the first clip is wide.
    for _ in 0..2 {
        h.key_press(Key::Equals);
        h.run_steps(2);
    }
    h.run_steps(20);
    // A taller first track, by its header's resize edge.
    let a = clip(&h, 0);
    drag(
        &mut h,
        Pos2::new(300.0, a.bottom() + 1.5),
        Pos2::new(300.0, a.bottom() + 70.0),
    );
    h.run_steps(20);
    let id = h.state().project().tracks[0].clips[0].id;
    let at = |h: &Harness<'static, App>| h.state().clip_handles(&h.ctx, id).expect("handles");
    let a = clip(&h, 0);
    h.hover_at(pos(a, 0.5, 0.75));
    save(&mut h, "fade-hover");

    let fade_in = at(&h).fade_in;
    drag(&mut h, fade_in, fade_in + vec2(a.width() * 0.3, 0.0));
    let fade_out = at(&h).fade_out;
    drag(&mut h, fade_out, fade_out - vec2(a.width() * 0.25, 0.0));
    // Bend the fade out up (Fast), and hold the fade in's bend mid-drag.
    let bend = at(&h).bend_out.expect("a bend dot");
    drag(&mut h, bend, bend - vec2(0.0, 12.0));
    let gain = at(&h).gain;
    drag(&mut h, gain, gain + vec2(0.0, 6.0));
    h.hover_at(pos(a, 0.5, 0.75));
    save(&mut h, "fade-gain-hover");
    let bend = at(&h).bend_in.expect("a bend dot");
    h.hover_at(bend);
    h.drag_at(bend);
    h.run_steps(2);
    // Freely: left and up (the owner's C3).
    h.hover_at(bend + vec2(-25.0, -20.0));
    save(&mut h, "fade-bending");
    h.drop_at(bend + vec2(-25.0, -20.0));
    h.run_steps(2);
    // Alt adds S-shape to the fade out.
    let bend = at(&h).bend_out.expect("a bend dot");
    h.event(Event::ModifiersChanged(Modifiers::ALT));
    drag(&mut h, bend, bend - vec2(0.0, 30.0));
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    h.hover_at(Pos2::new(a.right() + 300.0, a.bottom() + 120.0));
    save(&mut h, "fade-gain-idle");
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn clip_properties() {
    for (appearance, scale) in [("dark", 1.0), ("light", 1.5)] {
        clip_properties_looking(appearance, scale);
    }
}

fn clip_properties_looking(appearance: &str, scale: f32) {
    use egui_kittest::kittest::Queryable;
    let dir = TempDir::new("shots-props");
    let mut h = lumen_looking(&dir, appearance, scale);
    let save = |h: &mut Harness<'static, App>, name: &str| {
        save(
            h,
            &format!("{name}-{appearance}-{}", (scale * 100.0) as u32),
        );
    };
    for _ in 0..2 {
        h.key_press(Key::Equals);
        h.run_steps(2);
    }
    h.run_steps(20);
    // Mute the first clip (0), lock and recolour the second in its card.
    let a = clip(&h, 0);
    click(&mut h, pos(a, 0.5, 0.3));
    h.run_steps(2);
    h.key_press(Key::Num0);
    h.run_steps(2);
    let b = clip(&h, 1);
    click(&mut h, pos(b, 0.5, 0.3));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::ALT, Key::Enter);
    h.run_steps(4);
    h.get_by_label("Colour 5").click();
    h.run_steps(2);
    h.get_by_label("Lock clip").click();
    h.run_steps(4);
    h.hover_at(Pos2::new(a.right() + 300.0, 400.0));
    save(&mut h, "props-card");
    h.key_press(Key::Escape);
    h.run_steps(4);
    // Rename the third in its title bar.
    let c = clip(&h, 2);
    click(&mut h, pos(c, 0.5, 0.3));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::R);
    h.run_steps(4);
    save(&mut h, "props-rename");
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.key_press(Key::Escape);
    h.run_steps(4);
    h.hover_at(Pos2::new(a.right() + 300.0, 400.0));
    save(&mut h, "props-idle");
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn file_drops() {
    let dir = TempDir::new("shots-drops");
    let mut h = lumen(&dir);
    h.run_steps(20);
    let a = clip(&h, 0);
    h.input_mut().hovered_files = vec![eframe::egui::HoveredFile {
        path: Some(dir.path().join("hat.wav")),
        ..Default::default()
    }];
    h.hover_at(Pos2::new(a.right() + 120.0, a.center().y));
    h.run_steps(2);
    save(&mut h, "drop-onto-track");
    h.input_mut().hovered_files = vec![
        eframe::egui::HoveredFile {
            path: Some(dir.path().join("hat.wav")),
            ..Default::default()
        };
        2
    ];
    h.hover_at(Pos2::new(a.right() + 120.0, a.bottom() + 3.0));
    h.run_steps(2);
    save(&mut h, "drop-between-tracks");
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn lanes_menus() {
    let dir = TempDir::new("shots-menus");
    let mut h = lumen(&dir);
    h.run_steps(20);
    let a = clip(&h, 0);
    for (name, p) in [
        ("menu-clip", pos(a, 0.5, 0.3)),
        ("menu-lanes", Pos2::new(a.right() + 300.0, a.center().y)),
    ] {
        h.hover_at(p);
        h.run_steps(1);
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos: p,
                button: eframe::egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.run_steps(1);
        }
        save(&mut h, name);
        h.key_press(Key::Escape);
        h.run_steps(3);
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn command_palette() {
    for (appearance, scale) in [("dark", 1.0), ("light", 1.5)] {
        command_palette_looking(appearance, scale);
    }
}

fn command_palette_looking(appearance: &str, scale: f32) {
    let dir = TempDir::new("shots-palette");
    let mut h = lumen_looking(&dir, appearance, scale);
    let save = |h: &mut Harness<'static, App>, name: &str| {
        save(
            h,
            &format!("{name}-{appearance}-{}", (scale * 100.0) as u32),
        );
    };
    // Two clips selected, so commands name what they act on.
    let a = clip(&h, 0);
    click(&mut h, pos(a, 0.5, 0.3));
    let b = clip(&h, 1);
    h.event(Event::ModifiersChanged(Modifiers::COMMAND));
    click(&mut h, pos(b, 0.5, 0.3));
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    // Run one from the palette so the empty palette has a recent command.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    h.event(Event::Text("mute".into()));
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(4);
    save(&mut h, "palette-empty");
    h.event(Event::Text("dup".into()));
    h.run_steps(4);
    save(&mut h, "palette-dup");
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("past".into()));
    h.run_steps(4);
    save(&mut h, "palette-unavailable");
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("128".into()));
    h.run_steps(4);
    save(&mut h, "palette-value");
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("sna".into()));
    h.run_steps(4);
    save(&mut h, "palette-track");
    h.key_press(Key::Escape);
    h.run_steps(3);
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn keyboard_shortcuts() {
    for (appearance, scale) in [("dark", 1.0), ("light", 1.5)] {
        keyboard_shortcuts_looking(appearance, scale);
    }
}

fn keyboard_shortcuts_looking(appearance: &str, scale: f32) {
    use egui_kittest::kittest::Queryable;
    let dir = TempDir::new("shots-shortcuts");
    let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1280.0, 800.0));
    let save = |h: &mut Harness<'static, App>, name: &str| {
        save(
            h,
            &format!("{name}-{appearance}-{}", (scale * 100.0) as u32),
        );
    };
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    h.event(Event::Text("keyboard".into()));
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(6);
    save(&mut h, "shortcuts-open");
    h.get_by_label("Ctrl").click();
    h.run_steps(4);
    save(&mut h, "shortcuts-ctrl-layer");
    h.get_by_label("Ctrl").click();
    h.run_steps(2);
    h.get_by_label("Search shortcuts").click();
    h.run_steps(2);
    h.event(Event::Text("fit song".into()));
    h.run_steps(3);
    h.get_by_label("Change Fit song to width shortcut W")
        .click();
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run_steps(4);
    save(&mut h, "shortcuts-clash");
    h.key_press(Key::Escape);
    h.run_steps(3);
    h.key_press(Key::Escape);
    h.run_steps(3);
    // In the palette: Ctrl+Enter on a row records new keys there.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(3);
    h.event(Event::Text("fit song".into()));
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(4);
    save(&mut h, "palette-rekey");
    h.key_press(Key::Escape);
    h.run_steps(3);
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn menus_and_tips() {
    for (appearance, scale) in [("dark", 1.0), ("light", 1.5)] {
        menus_and_tips_looking(appearance, scale);
    }
}

fn menus_and_tips_looking(appearance: &str, scale: f32) {
    use egui_kittest::kittest::Queryable;
    let dir = TempDir::new("shots-menus");
    let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1280.0, 720.0));
    let save = |h: &mut Harness<'static, App>, name: &str| {
        save(
            h,
            &format!("{name}-{appearance}-{}", (scale * 100.0) as u32),
        );
    };
    let right = |h: &mut Harness<'static, App>, p: Pos2| {
        h.hover_at(p);
        h.run_steps(1);
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos: p,
                button: eframe::egui::PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.run_steps(1);
        }
        h.run_steps(3);
    };
    // A clip's menu.
    let a = clip(&h, 0);
    click(&mut h, pos(a, 0.5, 0.3));
    right(&mut h, pos(a, 0.5, 0.3));
    save(&mut h, "menu-clip");
    // A row under the pointer: its keycaps inside the highlight.
    let copy = h.get_by_label_contains("Copy").rect().center();
    h.hover_at(copy);
    save(&mut h, "menu-clip-hover");
    h.key_press(Key::Escape);
    h.run_steps(3);
    // The empty lanes' menu, on "Snap to grid", which is on (the owner's
    // screenshot: a lit toggle's keycaps sat off).
    right(&mut h, pos(a, 4.0, 0.5));
    let snap = h.get_by_label("Snap to grid Ctrl+4").rect().center();
    h.hover_at(snap);
    save(&mut h, "menu-lanes-hover");
    h.key_press(Key::Escape);
    h.run_steps(3);
    // The ruler's view menu.
    let ruler = h.get_by_label("Ruler: click to move the playhead").rect();
    right(&mut h, ruler.center());
    save(&mut h, "menu-ruler");
    h.key_press(Key::Escape);
    h.run_steps(3);
    // A track's menu.
    let header = h.get_by_label("kick: double-click to rename").rect();
    right(&mut h, header.center());
    save(&mut h, "menu-track");
    h.key_press(Key::Escape);
    h.run_steps(3);
    // A transport tooltip.
    let play = h.get_by_label("Play from the start marker").rect();
    h.hover_at(play.center());
    h.run_steps(60);
    save(&mut h, "tip-play");
    h.hover_at(pos2(600.0, 400.0));
    h.run_steps(3);
    // The app menu: switches and segmented choices (M4.5 slice 2).
    let lumen = h.get_by_label("Lumen").rect();
    click(&mut h, lumen.center());
    h.run_steps(30);
    save(&mut h, "menu-app");
    // A change keeps it open; leaving it fades it (the owner's C).
    let additive = h.get_by_label("Additive solo").rect();
    click(&mut h, additive.center());
    h.run_steps(20);
    save(&mut h, "menu-app-changed");
    h.hover_at(pos2(900.0, 600.0));
    h.run_steps(24 + 4);
    save(&mut h, "menu-app-fading");
    h.run_steps(30);
    // The metronome's menu.
    let metronome = h.get_by_label("Metronome").rect();
    right(&mut h, metronome.center());
    h.run_steps(30);
    save(&mut h, "menu-metronome");
    h.key_press(Key::Escape);
    h.run_steps(3);
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn smallest_window_at_200() {
    use egui_kittest::kittest::Queryable;
    let dir = TempDir::new("shots-small");
    let mut h = lumen_looking_sized(&dir, "dark", 2.0, vec2(960.0, 540.0));
    let save = |h: &mut Harness<'static, App>, name: &str| save(h, &format!("small-{name}"));
    save(&mut h, "main");
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run_steps(4);
    save(&mut h, "palette");
    h.event(Event::Text("keyboard".into()));
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(6);
    save(&mut h, "shortcuts");
    h.key_press(Key::Escape);
    h.run_steps(3);
    let a = clip(&h, 0);
    click(&mut h, pos(a, 0.5, 0.3));
    h.key_press_modifiers(Modifiers::ALT, Key::Enter);
    h.run_steps(6);
    save(&mut h, "card");
    let _ = h.query_by_label("Lumen");
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn light_layer() {
    use egui_kittest::kittest::Queryable;
    for appearance in ["dark", "light"] {
        let dir = TempDir::new("shots-light");
        let mut h = bare(&dir, appearance, vec2(1200.0, 640.0));
        save(&mut h, &format!("light-empty-{appearance}"));
        drop_files(&mut h, &dir, 8.0);
        h.key_press(Key::Space);
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            h.run_steps(1);
        }
        save(&mut h, &format!("light-playing-{appearance}"));
        h.get_by_label("Detail").click();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            h.run_steps(1);
        }
        save(&mut h, &format!("light-mixer-{appearance}"));
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn midi_chip() {
    use egui_kittest::kittest::Queryable;
    use lumen_midi::{Direction, fake::FakeControl};
    for (appearance, scale, width) in [
        ("dark", 1.0, 1280.0),
        ("light", 1.5, 1280.0),
        ("dark", 1.0, 960.0),
    ] {
        let fake = FakeControl::new();
        fake.plug(Direction::Input, "Arturia KeyStep 37");
        fake.plug(Direction::Input, "loopMIDI Port");
        fake.plug(Direction::Output, "Microsoft GS Wavetable Synth");
        let dir = TempDir::new("shots-chip");
        let launch = Launch::for_test(dir.path())
            .appearing(appearance)
            .with_midi(fake.clone());
        let mut h = Harness::builder()
            .with_size(vec2(width, 480.0))
            .with_pixels_per_point(scale)
            .with_theme(if appearance == "light" {
                eframe::egui::Theme::Light
            } else {
                eframe::egui::Theme::Dark
            })
            .with_step_dt(1.0 / 60.0)
            .wgpu()
            .build_eframe(|cc| {
                lumen_app::testing::install_renderer(cc).expect("the GPU renderer");
                App::new(cc, launch)
            });
        h.run_steps(10);
        let tag = format!("{appearance}-{scale}-{width}");
        if width < 1000.0 {
            save(&mut h, &format!("chip-{tag}"));
            continue;
        }
        h.get_by_label("MIDI: 0 inputs, 0 outputs").click();
        h.run_steps(30);
        h.get_by_label("Track Arturia KeyStep 37 input in the MIDI panel")
            .click();
        h.run_steps(3);
        h.get_by_label("Remote Arturia KeyStep 37 input in the MIDI panel")
            .click();
        h.get_by_label("Track Microsoft GS Wavetable Synth output in the MIDI panel")
            .click();
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.run_steps(1);
        }
        fake.send_to_input("Arturia KeyStep 37", 1, &[0x90, 60, 100]);
        h.run_steps(8);
        save(&mut h, &format!("chip-{tag}"));
    }
}

/// A left click at `p`.
fn click(h: &mut Harness<'static, App>, p: Pos2) {
    h.hover_at(p);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: p,
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(1);
    }
}

/// The point `fx` across and `fy` down `r`.
fn pos(r: Rect, fx: f32, fy: f32) -> Pos2 {
    Pos2::new(r.left() + r.width() * fx, r.top() + r.height() * fy)
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl eframe::egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// A stereo 16-bit WAV of a soft tone, `seconds` long.
fn wav(path: &std::path::Path, seconds: f32) -> PathBuf {
    let rate = 48_000_u32;
    let frames = (seconds * rate as f32) as usize;
    let data: Vec<u8> = (0..frames)
        .flat_map(|i| {
            let v = ((i as f32 * 0.05).sin() * 9_000.0 * (1.0 - i as f32 / frames as f32)) as i16;
            let [a, b] = v.to_le_bytes();
            [a, b, a, b]
        })
        .collect();
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 4).to_le_bytes());
    out.extend(4_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the WAV");
    path.to_path_buf()
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn info_line() {
    for (appearance, scale) in [("dark", 1.0), ("light", 1.5), ("dark", 2.0)] {
        let dir = TempDir::new("shots-info");
        let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1200.0, 520.0));
        let name = |what: &str| format!("info-{what}-{appearance}-{}", (scale * 100.0) as u32);
        let a = clip(&h, 0);
        h.hover_at(Pos2::new(a.left() + 20.0, a.top() + 8.0));
        save(&mut h, &name("clip"));
        h.event(Event::ModifiersChanged(Modifiers::COMMAND));
        h.hover_at(Pos2::new(a.left() + 21.0, a.top() + 8.0));
        save(&mut h, &name("clip-ctrl"));
        h.event(Event::ModifiersChanged(Modifiers::NONE));
        h.hover_at(Pos2::new(a.right() - 2.0, a.center().y));
        save(&mut h, &name("clip-end"));
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn dropouts() {
    use egui_kittest::kittest::Queryable;
    for (appearance, scale) in [("dark", 1.0), ("light", 1.5), ("dark", 2.0)] {
        let dir = TempDir::new("shot-dropouts");
        let mut h = lumen_looking(&dir, appearance, scale);
        h.state_mut().cause_dropouts(3);
        h.run_steps(30);
        save(&mut h, &format!("dropouts-chip-{appearance}-{scale}"));
        h.get_by_label("Audio: Dropouts ×3").click();
        h.run_steps(10);
        save(&mut h, &format!("dropouts-panel-{appearance}-{scale}"));
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn sends() {
    // MT-MIX-4 run 1: knobs on a selected row (C4), the "+N" list (O1)
    // and a return's senders (R2).
    use egui_kittest::kittest::Queryable;
    for (appearance, scale) in [("dark", 1.0), ("light", 1.0), ("dark", 1.5)] {
        let dir = TempDir::new("shots-sends");
        let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1200.0, 1400.0));
        for _ in 0..10 {
            h.get_by_label("Add track").click();
            h.run_steps(3);
            h.get_by_label_contains("Add return track").click();
            h.run_steps(3);
        }
        h.get_by_label("Fold kick").click();
        h.run_steps(30);
        for (knob, level) in [("A", "-6"), ("B", "-20")] {
            h.get_by_label_contains(&format!("Send kick to {knob} "))
                .click();
            h.run_steps(2);
            h.event(Event::Text(level.into()));
            h.run_steps(2);
            h.key_press(Key::Enter);
            h.run_steps(2);
        }
        // Five more tracks sending into A, so it lists four and "+N more"
        // (returns folded away meanwhile, so the new tracks are in view).
        h.get_by_label("Fold returns").click();
        h.run_steps(10);
        for n in 0..11 {
            h.key_press_modifiers(Modifiers::COMMAND, Key::T);
            h.run_steps(3);
            let name = h
                .state()
                .project()
                .tracks
                .iter()
                .rev()
                .find(|t| t.kind == lumen_project::model::TrackKind::Audio)
                .unwrap()
                .name
                .clone();
            h.get_by_label(&format!("Fold {name}")).click();
            h.run_steps(30);
            h.get_by_label_contains(&format!("Send {name} to A "))
                .click();
            h.run_steps(2);
            h.event(Event::Text(format!("-{}", 3 + n * 2)));
            h.run_steps(2);
            h.key_press(Key::Enter);
            h.run_steps(2);
            h.get_by_label(&format!("Fold {name}")).click();
            h.run_steps(30);
        }
        h.get_by_label("Fold returns").click();
        h.run_steps(10);
        h.get_by_label("Fold Return 1").click();
        h.run_steps(30);
        // Up to the top: kick selected.
        for _ in 0..16 {
            h.key_press(Key::ArrowUp);
            h.run_steps(1);
        }
        h.hover_at(Pos2::new(900.0, 650.0));
        h.run_steps(10);
        save(&mut h, &format!("sends-{appearance}-{scale}"));
        // A header knob mid-drag: its level in a chip above it.
        let knob = h.get_by_label_contains("Send kick to A ").rect();
        h.hover_at(knob.center());
        h.drag_at(knob.center());
        h.run_steps(2);
        h.hover_at(knob.center() - vec2(0.0, 20.0));
        h.run_steps(2);
        save(&mut h, &format!("send-dragging-{appearance}-{scale}"));
        h.drop_at(knob.center() - vec2(0.0, 20.0));
        h.run_steps(3);
        h.get_by_label_contains("more sends from kick").click();
        h.run_steps(5);
        save(&mut h, &format!("sends-list-{appearance}-{scale}"));
        h.key_press(Key::Escape);
        h.run_steps(3);
        h.get_by_label_contains("more tracks sending into A")
            .click();
        h.run_steps(5);
        save(&mut h, &format!("senders-list-{appearance}-{scale}"));
        h.key_press(Key::Escape);
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn mixer_strips_and_header_levels() {
    // The owner's M4 slice 5 report: strips wide enough for a track name,
    // the header volume's value clear of its meter, and peak readouts that
    // fit their box.
    use egui_kittest::kittest::Queryable;
    for (appearance, scale) in [("dark", 1.0), ("light", 1.0), ("dark", 1.5)] {
        let dir = TempDir::new("shots-mixer-fixes");
        let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1200.0, 900.0));
        for _ in 0..2 {
            h.key_press_modifiers(Modifiers::COMMAND, Key::T);
            h.run_steps(3);
        }
        h.hover_at(Pos2::new(600.0, 300.0));
        h.run_steps(10);
        h.get_by_label("Fold kick").click();
        h.run_steps(20);
        h.hover_at(Pos2::new(900.0, 650.0));
        h.run_steps(5);
        save(&mut h, &format!("headers-{appearance}-{scale}"));
        // A volume with three digits, typed under the fader (the owner's
        // finding that values overflow past two digits).
        h.get_by_label_contains("kick volume in the mixer").click();
        h.run_steps(2);
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.run_steps(1);
        h.event(Event::Text("-16.3".into()));
        h.run_steps(2);
        h.key_press(Key::Enter);
        h.run_steps(5);
        h.hover_at(Pos2::new(900.0, 650.0));
        h.run_steps(5);
        save(&mut h, &format!("volume-{appearance}-{scale}"));
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn delay_compensation_readout() {
    // M4 slice 6, the owner's D1: the readout at the end of the device
    // line, its path in the info line, and the audio panel's Latency rows.
    use egui_kittest::kittest::Queryable;
    for (appearance, scale) in [("dark", 1.0), ("light", 1.0), ("dark", 1.5)] {
        let dir = TempDir::new("shots-compensation");
        let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1600.0, 900.0));
        let kick = h.state().project().tracks[0].id;
        h.state_mut().set_test_latency(kick, 595);
        for _ in 0..200 {
            if h.state().compensation() == 595 {
                break;
            }
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.run_steps(5);
        let status = h.get_by_label_contains("Audio: ").rect();
        h.hover_at(Pos2::new(status.right() + 120.0, status.center().y));
        h.run_steps(5);
        save(&mut h, &format!("compensation-{appearance}-{scale}"));
        h.get_by_label_contains("Audio: ").click();
        h.run_steps(5);
        save(&mut h, &format!("compensation-panel-{appearance}-{scale}"));
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn polish_sweep() {
    // M4.5 slice 6: every view the polish pass changed, in both themes at
    // 100%, 150% and 200% (the lumen-design checklist, item 9).
    use egui_kittest::kittest::Queryable;
    for appearance in ["dark", "light"] {
        for scale in [1.0, 1.5, 2.0] {
            let name =
                |view: &str| format!("polish-{view}-{appearance}-{}", (scale * 100.0) as u32);
            // Empty states: the lanes, the browser and the mixer's ghost.
            let dir = TempDir::new("shots-polish-empty");
            let mut h = bare_scaled(&dir, appearance, scale, vec2(1280.0, 760.0));
            h.run_steps(40);
            save(&mut h, &name("empty"));
            drop(h);
            // Headers, strips, tabs and the transport with tracks.
            let dir = TempDir::new("shots-polish");
            let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1280.0, 760.0));
            h.run_steps(40);
            let a = clip(&h, 0);
            click(&mut h, pos(a, 0.5, 0.3));
            h.hover_at(pos2(640.0, 700.0));
            h.run_steps(30);
            save(&mut h, &name("tracks"));
            // The app menu's switches and segmented choices.
            let lumen = h.get_by_label("Lumen").rect();
            click(&mut h, lumen.center());
            h.run_steps(30);
            save(&mut h, &name("menu-app"));
            h.key_press(Key::Escape);
            h.run_steps(30);
            // The clip card.
            h.key_press_modifiers(Modifiers::ALT, Key::Enter);
            h.run_steps(30);
            save(&mut h, &name("card"));
            h.key_press(Key::Escape);
            h.run_steps(30);
            // Audio settings: choice rows and the test tone's switch.
            let lumen = h.get_by_label("Lumen").rect();
            click(&mut h, lumen.center());
            h.run_steps(30);
            h.get_by_label("Audio settings\u{2026}").click();
            h.run_steps(30);
            save(&mut h, &name("audio"));
        }
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn mixer_edges() {
    // M4.5 slice 6, the audit's U2: strips cut by the mixer's edges fade
    // into the ground there.
    for appearance in ["dark", "light"] {
        let dir = TempDir::new("shots-mixer-edges");
        let mut h = lumen_looking_sized(&dir, appearance, 1.0, vec2(1000.0, 760.0));
        for _ in 0..12 {
            h.key_press_modifiers(Modifiers::COMMAND, Key::T);
            h.run_steps(3);
        }
        h.run_steps(40);
        save(&mut h, &format!("edges-start-{appearance}"));
        h.hover_at(pos2(500.0, 520.0));
        h.run_steps(2);
        h.event(Event::MouseWheel {
            unit: eframe::egui::MouseWheelUnit::Point,
            delta: vec2(-150.0, 0.0),
            phase: eframe::egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(40);
        save(&mut h, &format!("edges-middle-{appearance}"));
    }
}

/// A right click at `p`.
fn right_click(h: &mut Harness<'static, App>, p: Pos2) {
    h.hover_at(p);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: p,
            button: eframe::egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(1);
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn live_sweep() {
    // M5 slice 2e: the chip and its popover, the keyboard button with its
    // readout, the hollow listening ring, header lights, the track menu with
    // MIDI From and Monitor, and the mixer strip's From and To rows.
    use egui_kittest::kittest::Queryable;
    use lumen_midi::{Direction, fake::FakeControl};
    use lumen_project::model::{Color, Id, Monitor, Track, TrackKind};
    use lumen_project::testing::{midi_track, project_of};

    for (appearance, scale, size) in [
        ("dark", 1.0, vec2(1280.0, 760.0)),
        ("light", 1.0, vec2(1280.0, 760.0)),
        ("dark", 1.5, vec2(1280.0, 760.0)),
        ("light", 1.5, vec2(1280.0, 760.0)),
        ("dark", 2.0, vec2(1280.0, 760.0)),
        ("light", 2.0, vec2(1280.0, 760.0)),
        ("dark", 1.0, vec2(960.0, 540.0)),
        ("light", 1.0, vec2(960.0, 540.0)),
    ] {
        let tag = format!("{appearance}-{}-{}", (scale * 100.0) as u32, size.x as u32);
        let fake = FakeControl::new();
        fake.plug(Direction::Input, "Arturia KeyStep 37");
        fake.plug(Direction::Output, "Microsoft GS Wavetable Synth");
        let dir = TempDir::new("shots-live");
        let mut lead = midi_track(11, "Lead", "poly", (0, 8), None, vec![], vec![]);
        lead.monitor = Monitor::In;
        let mut long = midi_track(
            31,
            "Long Name Pad Strings Choir Layer",
            "poly",
            (0, 8),
            None,
            vec![],
            vec![],
        );
        long.monitor = Monitor::In;
        let project = project_of(
            "Live",
            vec![
                midi_track(1, "Keys", "poly", (0, 8), None, vec![], vec![]),
                lead,
                long,
                Track::new(Id(21), "Vox", TrackKind::Audio, Color(0x4F81BD)),
            ],
        );
        let path = dir.path().join("song.lumen");
        lumen_project::save(&project, &path, &Default::default()).expect("save");
        let launch = Launch::for_test(dir.path())
            .appearing(appearance)
            .with_midi(fake.clone())
            .opening(path);
        let mut h = Harness::builder()
            .with_size(size)
            .with_pixels_per_point(scale)
            .with_theme(if appearance == "light" {
                eframe::egui::Theme::Light
            } else {
                eframe::egui::Theme::Dark
            })
            .with_step_dt(1.0 / 60.0)
            .wgpu()
            .build_eframe(|cc| {
                lumen_app::testing::install_renderer(cc).expect("the GPU renderer");
                App::new(cc, launch)
            });
        for _ in 0..60 {
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Ports on for tracks, through the chip's popover.
        h.get_by_label("MIDI: 0 inputs, 0 outputs").click();
        h.run_steps(30);
        h.get_by_label("Track Arturia KeyStep 37 input in the MIDI panel")
            .click();
        h.run_steps(3);
        h.get_by_label("Track Microsoft GS Wavetable Synth output in the MIDI panel")
            .click();
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.run_steps(1);
        }
        // The popover with traffic.
        fake.send_to_input("Arturia KeyStep 37", 1, &[0x90, 60, 100]);
        h.run_steps(6);
        save(&mut h, &format!("live-popover-{tag}"));
        h.key_press(Key::Escape);
        h.run_steps(40);
        // Keyboard mode on, Keys selected (the hollow ring), a key held.
        h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
        h.run_steps(30);
        h.get_by_label_contains("Track Keys,").click();
        h.run_steps(40);
        save(&mut h, &format!("live-rest-{tag}"));
        h.event(Event::Key {
            key: Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(4);
        save(&mut h, &format!("live-lit-{tag}"));
        h.event(Event::Key {
            key: Key::A,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(60);
        // The track menu with MIDI From open.
        let keys = h.get_by_label_contains("Track Keys,").rect();
        right_click(&mut h, pos(keys, 0.5, 0.2));
        h.run_steps(30);
        h.get_by_label("MIDI From").click();
        h.run_steps(30);
        save(&mut h, &format!("live-menu-{tag}"));
        h.key_press(Key::Escape);
        h.run_steps(5);
        h.key_press(Key::Escape);
        h.run_steps(30);
        // The mixer strips (the panel is open; tall enough for From and To).
        h.state_mut().set_detail_size(size.y * 0.55);
        h.run_steps(60);
        save(&mut h, &format!("live-mixer-{tag}"));
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn transport_fit() {
    // The transport bar at 960, 1280 and 1600 wide with the computer
    // keyboard's mode on, cropped to the bar.
    for appearance in ["dark", "light"] {
        for scale in [1.0, 2.0] {
            for width in [960.0, 1280.0, 1600.0] {
                let height = if width == 960.0 { 540.0 } else { 760.0 };
                let dir = TempDir::new("shots-bar");
                let mut h = bare_scaled(&dir, appearance, scale, vec2(width, height));
                h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
                h.run_steps(40);
                let image = h.render().expect("render");
                let out = PathBuf::from(std::env::var("SHOTS").expect("SHOTS names a folder"));
                image
                    .save(out.join(format!(
                        "bar-{appearance}-{}-{}.png",
                        (scale * 100.0) as u32,
                        width as u32
                    )))
                    .expect("save");
            }
        }
    }
}
