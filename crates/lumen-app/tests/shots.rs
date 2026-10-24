//! Renders states a `--screenshot` cannot reach (hovers, drags, selections)
//! to PNG files, for checking the look by eye. Not a test of behaviour: it
//! is ignored, and runs on demand with a GPU, one test at a time (the
//! theme is process-wide, so tests run side by side render mixed themes):
//!
//! ```text
//! SHOTS=<folder> cargo test -p lumen-app --test shots -- --ignored --test-threads=1
//! ```

/// M7b E0: the chosen tool switch and its menu in both themes and scales.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn arrangement_tools() {
    use egui_kittest::kittest::Queryable;
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new(&format!("shots-tools-{appearance}-{scale}"));
            let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(960.0, 540.0));
            h.run_steps(30);
            save(&mut h, &format!("tools-rest-{appearance}-{scale}"));
            h.key_press_modifiers(Modifiers::COMMAND, Key::K);
            h.run_steps(3);
            h.event(Event::Text("Split tool".into()));
            h.run_steps(3);
            h.key_press(Key::Enter);
            h.run_steps(30);
            save(&mut h, &format!("tools-{appearance}-{scale}"));
            h.get_by_label("Arrangement tool: Split").click();
            h.run_steps(30);
            save(&mut h, &format!("tools-menu-{appearance}-{scale}"));
        }
    }
}

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

fn save<S>(h: &mut Harness<'_, S>, name: &str) {
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

    // Select two by a marquee (Ctrl+Shift in empty lanes, ADR-0049), held
    // mid-drag.
    let b = clip(&h, 1);
    let from = Pos2::new(a.right() + 200.0, a.top() + 2.0);
    h.event(Event::ModifiersChanged(
        Modifiers::COMMAND | Modifiers::SHIFT,
    ));
    h.hover_at(from);
    h.drag_at(from);
    h.run_steps(2);
    h.hover_at(Pos2::new(a.left() + 30.0, b.bottom() - 4.0));
    save(&mut h, "clip-marquee");
    h.drop_at(Pos2::new(a.left() + 30.0, b.bottom() - 4.0));
    h.run_steps(2);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
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

/// M7b E2: an eligible join with a lengthened, crossed fade and the
/// project menu switch at both themes and each supported scale.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn crossfades() {
    use egui_kittest::kittest::Queryable;

    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new(&format!("shots-crossfade-{appearance}-{scale}"));
            let mut h = lumen_looking(&dir, appearance, scale);
            for _ in 0..3 {
                h.key_press(Key::Equals);
                h.run_steps(2);
            }
            h.run_steps(20);
            let left = h.state().project().tracks[0].clips[0].id;
            let before = clip(&h, 0);
            let zoom = h.state().view().zoom as f32;
            let cut = Pos2::new(before.left() + zoom, before.bottom() - 6.0);
            click(&mut h, cut);
            h.key_press_modifiers(Modifiers::COMMAND, Key::E);
            h.run_steps(4);
            let right = h.state().project().tracks[0].clips[1].id;
            h.state_mut().select_clip(right);

            // Keep the clips touching while slipping the right source window
            // away from the split point, leaving tail in the earlier source.
            let r = h.state().clip_rect(&h.ctx, right).expect("right clip");
            let origin = Pos2::new(r.center().x, r.top() + 6.0);
            h.event(Event::ModifiersChanged(Modifiers::COMMAND | Modifiers::ALT));
            drag(&mut h, origin, origin + vec2(0.25 * zoom, 0.0));
            h.event(Event::ModifiersChanged(Modifiers::NONE));
            h.run_steps(5);

            let at = h
                .state()
                .clip_handles(&h.ctx, right)
                .expect("automatic join has an incoming handle");
            drag(
                &mut h,
                at.fade_in,
                at.fade_in + vec2(at.wave.width() * 0.2, 0.0),
            );
            h.run_steps(10);
            let at = h
                .state()
                .clip_handles(&h.ctx, right)
                .expect("crossfade handle");
            h.hover_at(at.fade_in + vec2(at.wave.width() * 0.1, 0.0));
            save(
                &mut h,
                &format!("crossfade-join-{appearance}-{}", (scale * 100.0) as u32),
            );

            h.get_by_label("Lumen").click();
            h.run_steps(4);
            save(
                &mut h,
                &format!("crossfade-menu-{appearance}-{}", (scale * 100.0) as u32),
            );
            // The left clip is kept alive here to ensure the rendered scene
            // came from a two-clip join, not a standalone fade.
            assert_ne!(left, right);
            drop(h);
            let mut minimum = lumen_looking_sized(&dir, appearance, scale, vec2(960.0, 540.0));
            minimum.get_by_label("Lumen").click();
            minimum.run_steps(4);
            save(
                &mut minimum,
                &format!("crossfade-minimum-{appearance}-{}", (scale * 100.0) as u32),
            );
            minimum.key_press(Key::Escape);
            minimum.run_steps(20);
            let r = clip(&minimum, 0);
            right_click(&mut minimum, Pos2::new(r.left() + 12.0, r.top() + 6.0));
            minimum.run_steps(5);
            save(
                &mut minimum,
                &format!(
                    "crossfade-clip-menu-{appearance}-{}",
                    (scale * 100.0) as u32
                ),
            );
        }
    }
}

/// M7b E2: the crossfade of a plain split (the owner's flow): the zone, its
/// shaded wedges and both curves, unhovered and hovered.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn crossfade_split() {
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 2.0] {
            let dir = TempDir::new(&format!("shots-crossfade-split-{appearance}-{scale}"));
            let mut h = lumen_looking(&dir, appearance, scale);
            for _ in 0..3 {
                h.key_press(Key::Equals);
                h.run_steps(2);
            }
            h.run_steps(20);
            let before = clip(&h, 0);
            let zoom = h.state().view().zoom as f32;
            let cut = Pos2::new(before.left() + zoom, before.bottom() - 6.0);
            click(&mut h, cut);
            h.key_press_modifiers(Modifiers::COMMAND, Key::E);
            h.run_steps(4);
            let left = h.state().project().tracks[0].clips[0].id;
            let right = h.state().project().tracks[0].clips[1].id;
            let at = h.state().clip_handles(&h.ctx, left).expect("left handles");
            drag(
                &mut h,
                at.fade_out,
                at.fade_out + vec2(3.0 * zoom / 2.0, 0.0),
            );
            h.run_steps(10);
            let name = (scale * 100.0) as u32;
            h.hover_at(Pos2::new(before.right() + 300.0, before.bottom() + 120.0));
            h.run_steps(5);
            save(&mut h, &format!("crossfade-split-idle-{appearance}-{name}"));
            let at = h
                .state()
                .clip_handles(&h.ctx, right)
                .expect("right handles");
            h.hover_at(at.fade_in + vec2(at.wave.width() * 0.05, 20.0));
            h.run_steps(5);
            save(
                &mut h,
                &format!("crossfade-split-hover-{appearance}-{name}"),
            );
            // The incoming clip selected: its outline begins where the
            // crossing ends.
            h.state_mut().select_clip(right);
            h.hover_at(Pos2::new(before.right() + 300.0, before.bottom() + 120.0));
            h.run_steps(5);
            save(
                &mut h,
                &format!("crossfade-split-right-{appearance}-{name}"),
            );
        }
    }
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
    for appearance in ["dark", "light"] {
        for scale in [1.0, 1.5, 2.0] {
            lanes_menus_looking(appearance, scale);
        }
    }
}

fn lanes_menus_looking(appearance: &str, scale: f32) {
    let dir = TempDir::new("shots-menus");
    let mut h = lumen_looking(&dir, appearance, scale);
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
        save(
            &mut h,
            &format!("{name}-{appearance}-{}", (scale * 100.0) as u32),
        );
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

/// The empty arrangement's prompt in short windows, with the detail panel
/// open under the lanes (the owner's report: the button ran over the time
/// ruler and the hint was cut). The prompt gives up its icon, then its hint.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn empty_prompt_short() {
    for (appearance, scale) in [("dark", 1.0), ("light", 1.5), ("dark", 2.0)] {
        let dir = TempDir::new("shots-empty-short");
        for height in [540.0, 440.0, 380.0] {
            let mut h = bare_scaled(&dir, appearance, scale, vec2(960.0, height));
            let name = format!(
                "empty-short-{appearance}-{}-{height}",
                (scale * 100.0) as u32
            );
            h.run_steps(30);
            save(&mut h, &format!("{name}-panel-as-started"));
            h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
            h.run_steps(30);
            save(&mut h, &format!("{name}-panel-toggled"));
        }
    }
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

/// MIDI files and clock (M5 slice 7): the tracks and the tempo offer a
/// dropped song file makes, and the MIDI page with Sync and its delay.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn midi_files_and_clock() {
    use egui_kittest::kittest::Queryable;
    use lumen_midi::{Direction, fake::FakeControl};
    let groove = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/smf/groove-480.mid");
    for (appearance, scale) in [("dark", 1.0), ("light", 1.5), ("dark", 2.0)] {
        let fake = FakeControl::new();
        fake.plug(Direction::Output, "Drum machine");
        let dir = TempDir::new("shots-smf");
        let launch = Launch::for_test(dir.path())
            .appearing(appearance)
            .with_midi(fake.clone());
        let mut h = Harness::builder()
            .with_size(vec2(1280.0, 760.0))
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
        h.input_mut().dropped_files =
            vec![std::sync::Arc::new(Dropped(groove.clone())) as eframe::egui::DroppedFileHandle];
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.run_steps(1);
        }
        let tag = format!("{appearance}-{scale}");
        save(&mut h, &format!("smf-tempo-offer-{tag}"));
        h.get_by_label("Lumen").click();
        h.run_steps(5);
        h.get_by_label("Audio and MIDI settings\u{2026}").click();
        h.run_steps(5);
        h.get_by_label("MIDI tab").click();
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.run_steps(1);
        }
        h.get_by_label("Sync Drum machine output").click();
        h.run_steps(20);
        save(&mut h, &format!("midi-sync-{tag}"));
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
fn mixer_armed_glow_clears_the_peak_readout() {
    // The owner's M4.5 finding: an armed record button's glow ran into the
    // peak readout above it. The gap above the buttons now clears the glow.
    use egui_kittest::kittest::Queryable;
    for (appearance, scale) in [("dark", 1.0), ("light", 1.0), ("dark", 2.0)] {
        let dir = TempDir::new("shots-mixer-arm");
        let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1200.0, 900.0));
        h.key_press_modifiers(Modifiers::COMMAND, Key::T);
        h.run_steps(3);
        h.get_by_label("Mixer tab").click();
        h.run_steps(5);
        h.get_by_label("Arm kick in the mixer").click();
        h.run_steps(20);
        h.hover_at(Pos2::new(900.0, 650.0));
        h.run_steps(5);
        save(&mut h, &format!("mixer-armed-{appearance}-{scale}"));
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
            // The first Escape leaves the name field; the second closes the card.
            h.key_press(Key::Escape);
            h.run_steps(30);
            h.key_press(Key::Escape);
            h.run_steps(30);
            // Audio settings: choice rows and the test tone's switch.
            let lumen = h.get_by_label("Lumen").rect();
            click(&mut h, lumen.center());
            h.run_steps(30);
            h.get_by_label("Audio and MIDI settings\u{2026}").click();
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

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn monitor_sweep() {
    // The Monitor badge (M5 slice 2 fix): In, Off and Auto beside long
    // names, rows unselected and selected, an audio track with none, at the
    // smallest window too.
    use egui_kittest::kittest::Queryable;
    use lumen_midi::fake::FakeControl;
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
        let dir = TempDir::new("shots-monitor");
        let mut lead = midi_track(31, "Lead", "poly", (0, 8), None, vec![], vec![]);
        lead.monitor = Monitor::In;
        let mut pad = midi_track(51, "Pad", "poly", (0, 8), None, vec![], vec![]);
        pad.monitor = Monitor::Off;
        let mut long = midi_track(
            71,
            "Long Name Pad Strings Choir Layer",
            "poly",
            (0, 8),
            None,
            vec![],
            vec![],
        );
        long.monitor = Monitor::In;
        let mut long_off = midi_track(
            91,
            "Another Very Long Track Name",
            "poly",
            (0, 8),
            None,
            vec![],
            vec![],
        );
        long_off.monitor = Monitor::Off;
        let project = project_of(
            "Monitor",
            vec![
                midi_track(1, "Keys", "poly", (0, 8), None, vec![], vec![]),
                lead,
                pad,
                long,
                long_off,
                Track::new(Id(111), "Vox", TrackKind::Audio, Color(0x4F81BD)),
            ],
        );
        let path = dir.path().join("song.lumen");
        lumen_project::save(&project, &path, &Default::default()).expect("save");
        let launch = Launch::for_test(dir.path())
            .appearing(appearance)
            .with_midi(fake)
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
        h.run_steps(30);
        save(&mut h, &format!("monitor-rest-{tag}"));
        // Select the In track, then the Off track: the badge on a selected row.
        let r = h.get_by_label_contains("Track Lead,").rect();
        left_click(&mut h, pos(r, 0.3, 0.5));
        h.run_steps(40);
        save(&mut h, &format!("monitor-in-selected-{tag}"));
        // At 960 x 540 the mixer leaves room for two rows (the 44 pt
        // transport bar, ADR-0052): Pad is out of view there.
        if let Some(pad) = h.query_by_label_contains("Track Pad,") {
            let r = pad.rect();
            left_click(&mut h, pos(r, 0.3, 0.5));
            h.run_steps(40);
            save(&mut h, &format!("monitor-off-selected-{tag}"));
        }
        // Hover the In badge.
        let badge = h.get_by_label("Monitor Lead: In").rect();
        h.hover_at(badge.center());
        h.run_steps(30);
        save(&mut h, &format!("monitor-hover-{tag}"));
    }
}

/// A left click at `p` (the header's centre can be on a badge).
fn left_click(h: &mut Harness<'static, App>, p: Pos2) {
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

/// The piano roll body and the arrangement's mini notes (M5 slice 3b),
/// in both themes at 100, 150 and 200%.
struct RollShot {
    demo: lumen_app::testing::PianoRollDemo,
    zoom: f64,
    row: f32,
}

impl eframe::App for RollShot {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.demo.show(ui, self.zoom, self.row);
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn piano_roll_body() {
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let mut h = Harness::builder()
                .with_size(vec2(900.0, 420.0))
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
                    lumen_app::testing::PianoRollDemo::prepare(&cc.egui_ctx, appearance == "light");
                    RollShot {
                        demo: lumen_app::testing::PianoRollDemo::new(1),
                        zoom: 40.0,
                        row: 14.0,
                    }
                });
            h.run_steps(5);
            save(
                &mut h,
                &format!("piano-roll-{appearance}-{}", (scale * 100.0) as u32),
            );
        }
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn mini_notes() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/projects/midi-demo.lumen");
    for appearance in ["dark", "light"] {
        let dir = TempDir::new("shots-mini");
        let copy = dir.path().join("midi-demo.lumen");
        std::fs::copy(&fixture, &copy).expect("the MIDI demo project");
        let launch = Launch::for_test(dir.path())
            .appearing(appearance)
            .opening(copy);
        let mut h = Harness::builder()
            .with_size(vec2(1200.0, 500.0))
            .with_step_dt(1.0 / 60.0)
            .wgpu()
            .build_eframe(|cc| {
                lumen_app::testing::install_renderer(cc).expect("the GPU renderer");
                App::new(cc, launch)
            });
        h.run_steps(60);
        save(&mut h, &format!("mini-notes-{appearance}"));
        // A tall Chords track: every pitch a row, neighbours touching, as
        // in the piano roll.
        let dir = TempDir::new("shots-mini-tall");
        let copy = dir.path().join("midi-demo.lumen");
        let text = std::fs::read_to_string(&fixture).expect("the MIDI demo project");
        let tall = text
            .replacen(
                "\"track_heights\": {}",
                "\"track_heights\": { \"0000000000000001\": 260.0 }",
                1,
            )
            .replacen(
                "\"expanded\": []",
                "\"expanded\": [\"0000000000000001\"]",
                1,
            );
        assert_ne!(tall, text, "the fixture names its track heights");
        std::fs::write(&copy, tall).expect("a copy");
        let launch = Launch::for_test(dir.path())
            .appearing(appearance)
            .opening(copy);
        let mut h = Harness::builder()
            .with_size(vec2(1200.0, 640.0))
            .with_step_dt(1.0 / 60.0)
            .wgpu()
            .build_eframe(|cc| {
                lumen_app::testing::install_renderer(cc).expect("the GPU renderer");
                App::new(cc, launch)
            });
        h.run_steps(60);
        save(&mut h, &format!("mini-notes-tall-{appearance}"));
    }
}

/// The clip editor (M5 slice 3c) with the MIDI demo's Chords clip open,
/// one chord picked and another note hovered, in both themes at 100, 150
/// and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn note_editor() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/projects/midi-demo.lumen");
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new("shots-editor");
            let copy = dir.path().join("midi-demo.lumen");
            std::fs::copy(&fixture, &copy).expect("the MIDI demo project");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
                .opening(copy);
            let mut h = Harness::builder()
                .with_size(vec2(1400.0, 860.0))
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
            h.run_steps(60);
            h.state_mut().set_detail_size(560.0);
            h.run_steps(10);
            let id = h.state().project().tracks[0].clips[0].id;
            let r = h.state().clip_rect(&h.ctx, id).expect("in view");
            let at = pos(r, 0.1, 0.6);
            click(&mut h, at);
            click(&mut h, at);
            h.run_steps(20);
            h.state_mut().editor_show_pitch(60);
            h.run_steps(5);
            let point = |h: &Harness<'static, App>, beat: i64, pitch: u8| {
                let p = h
                    .state()
                    .editor_point(lumen_core::time::Ticks::quarters(beat), pitch)
                    .expect("the editor drew");
                pos2(p.x + 12.0, p.y)
            };
            let first = point(&h, 4, 57);
            click(&mut h, first);
            let hover = point(&h, 12, 60);
            h.hover_at(hover);
            h.run_steps(5);
            save(
                &mut h,
                &format!("note-editor-{appearance}-{}", (scale * 100.0) as u32),
            );
        }
    }
}

/// The clip editor showing notes outside the clip's window (ADR-0059): a
/// clip from beat 4 to 8 with notes before its start, past its end and one
/// running over its end, zoomed out so all show; the one past the end
/// picked. Both themes at 100, 150 and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn hidden_notes_editor() {
    use lumen_core::time::Ticks;
    use lumen_project::model::Note;
    let q = Ticks::quarters;
    let notes = vec![
        Note::new(q(-2), q(1), 60, 100),
        Note::new(q(0), q(1), 62, 100),
        Note::new(q(1), q(1), 64, 90),
        Note::new(q(3), q(2), 67, 110),
        Note::new(q(6), q(1), 65, 100),
    ];
    let mut project = lumen_project::testing::project_of(
        "Hidden",
        vec![lumen_project::testing::midi_track(
            1,
            "Keys",
            "poly",
            (4, 4),
            None,
            notes,
            vec![],
        )],
    );
    project.transport.loop_end = q(16);
    project.transport.loop_enabled = true;
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new("shots-hidden");
            let path = dir.path().join("hidden.lumen");
            lumen_project::save(&project, &path, &Default::default()).expect("save");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
                .opening(path);
            let mut h = Harness::builder()
                .with_size(vec2(1400.0, 860.0))
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
            h.run_steps(60);
            h.state_mut().set_detail_size(560.0);
            h.run_steps(10);
            let id = h.state().project().tracks[0].clips[0].id;
            let r = h.state().clip_rect(&h.ctx, id).expect("in view");
            let at = pos(r, 0.5, 0.6);
            click(&mut h, at);
            click(&mut h, at);
            h.run_steps(20);
            let body = h.state().editor_body().expect("the editor drew");
            h.hover_at(body.center());
            h.run_steps(2);
            for _ in 0..3 {
                h.key_press(Key::Minus);
                h.run_steps(3);
            }
            h.state_mut().editor_show_pitch(64);
            h.run_steps(5);
            let past = h.state().editor_point(q(6), 65).expect("drawn");
            click(&mut h, pos2(past.x + 6.0, past.y));
            h.hover_at(body.center());
            h.run_steps(5);
            save(
                &mut h,
                &format!("hidden-notes-{appearance}-{}", (scale * 100.0) as u32),
            );
        }
    }
}

/// The velocity and controller lane under the clip editor (M5 slice 4b):
/// the velocity lane with a stroke drawn across it, and the mod wheel lane
/// with a curve just drawn, its dot showing, plus the solo chip states;
/// both themes at 100, 150 and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn clip_lane() {
    use egui_kittest::kittest::Queryable;
    use lumen_core::time::Ticks;
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/projects/midi-demo.lumen");
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new("shots-lane");
            let copy = dir.path().join("midi-demo.lumen");
            std::fs::copy(&fixture, &copy).expect("the MIDI demo project");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
                .opening(copy);
            let mut h = Harness::builder()
                .with_size(vec2(1400.0, 860.0))
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
            h.run_steps(60);
            h.state_mut().set_detail_size(560.0);
            h.run_steps(10);
            h.get_by_label("Solo Chords").click();
            h.run_steps(20);
            save(
                &mut h,
                &format!("lane-solo-one-{appearance}-{}", (scale * 100.0) as u32),
            );
            h.event(Event::ModifiersChanged(Modifiers::COMMAND));
            h.get_by_label("Solo Pad").click();
            h.event(Event::ModifiersChanged(Modifiers::NONE));
            h.run_steps(20);
            save(
                &mut h,
                &format!("lane-solo-many-{appearance}-{}", (scale * 100.0) as u32),
            );
            h.get_by_label("Clear all solos").click();
            h.run_steps(20);
            save(
                &mut h,
                &format!("lane-solo-none-{appearance}-{}", (scale * 100.0) as u32),
            );
            let id = h.state().project().tracks[0].clips[0].id;
            h.hover_at(pos2(1000.0, 400.0));
            h.run_steps(10);
            let r = h.state().clip_rect(&h.ctx, id).expect("in view");
            // Open the MIDI editor from the clip's title header; its body is
            // reserved for range gestures.
            let at = pos(r, 0.1, 0.1);
            click(&mut h, at);
            click(&mut h, at);
            h.run_steps(20);
            let lane = |h: &Harness<'static, App>, beat: f64, value: i32| {
                h.state()
                    .lane_point(Ticks((beat * lumen_core::time::PPQ as f64) as i64), value)
                    .expect("the lane drew")
            };
            // A stroke across the velocity stems.
            let (a, b) = (lane(&h, 4.0, 40), lane(&h, 12.0, 110));
            drag(&mut h, a, b);
            h.hover_at(lane(&h, 9.0, 60));
            h.run_steps(5);
            save(
                &mut h,
                &format!("lane-velocity-{appearance}-{}", (scale * 100.0) as u32),
            );
            // A curve in the mod wheel lane.
            h.get_by_label("Mod wheel tab").click();
            h.run_steps(5);
            h.get_by_label("Lane curve tool").click();
            h.run_steps(5);
            let (a, b) = (lane(&h, 3.0, 10), lane(&h, 13.0, 120));
            drag(&mut h, a, b);
            h.hover_at(lane(&h, 8.0, 100));
            h.run_steps(5);
            save(
                &mut h,
                &format!("lane-curve-{appearance}-{}", (scale * 100.0) as u32),
            );
        }
    }
}

/// The quantize options open over the piano roll (M5 slice 5): the comet
/// sliders and the ghost trails, both themes at 100, 150 and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn quantize_popover() {
    use egui_kittest::kittest::Queryable;
    use lumen_core::time::{PPQ, Ticks};
    use lumen_project::model::Note;
    use lumen_project::testing::{midi_track, project_of};
    // A part played loose: the notes sit a little off the beat.
    let beats = [
        (0.04, 60),
        (0.55, 64),
        (1.03, 67),
        (1.47, 64),
        (2.06, 60),
        (2.55, 62),
        (3.0, 65),
        (3.53, 69),
        (4.07, 60),
        (4.46, 64),
        (5.02, 67),
        (5.58, 72),
    ];
    let notes: Vec<Note> = beats
        .iter()
        .map(|&(b, p)| Note::new(Ticks((b * PPQ as f64) as i64), Ticks(PPQ / 2), p, 100))
        .collect();
    let project = project_of(
        "Loose",
        vec![midi_track(1, "Keys", "poly", (0, 8), None, notes, vec![])],
    );
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new("shots-quantize");
            let copy = dir.path().join("loose.lumen");
            lumen_project::save(&project, &copy, &Default::default()).expect("save");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
                .opening(copy);
            let mut h = Harness::builder()
                .with_size(vec2(1400.0, 860.0))
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
            h.run_steps(60);
            h.state_mut().set_detail_size(560.0);
            h.run_steps(10);
            let id = h.state().project().tracks[0].clips[0].id;
            h.hover_at(pos2(1000.0, 400.0));
            h.run_steps(10);
            let r = h.state().clip_rect(&h.ctx, id).expect("in view");
            let at = pos(r, 0.1, 0.1);
            click(&mut h, at);
            click(&mut h, at);
            h.run_steps(20);
            h.get_by_label("Quantize").click();
            h.run_steps(20);
            save(
                &mut h,
                &format!("quantize-{appearance}-{}", (scale * 100.0) as u32),
            );
        }
    }
}

/// The owner's MT-MIDI-7 run 3 finding: clips on one track an octave apart,
/// and two different melodies, must look different. Each track's clips
/// share one layout, so pitch is height across the whole track.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn mini_notes_octaves() {
    use lumen_core::time::Ticks;
    use lumen_project::model::{Clip, Note};
    use lumen_project::testing::{midi_track, project_of};
    use std::sync::Arc;
    let q = Ticks::quarters;
    let clip_of = |track: &lumen_project::model::Track, id, start, notes| {
        let mut clip: Clip = (*track.clips[0]).clone();
        clip.id = lumen_project::model::Id(id);
        clip.start = q(start);
        clip.length = q(6);
        clip.content = lumen_project::model::ClipContent::Midi {
            notes,
            events: Vec::new(),
        };
        Arc::new(clip)
    };
    // Track 1: one note an octave apart in two clips.
    let mut octave = midi_track(1, "Octave", "poly", (0, 6), None, Vec::new(), vec![]);
    octave.clips = vec![
        clip_of(&octave, 10, 0, vec![Note::new(q(0), q(5), 60, 100)]),
        clip_of(&octave, 11, 8, vec![Note::new(q(0), q(5), 72, 100)]),
    ];
    // Track 2: two melodies that differ.
    let melody = |pitches: &[u8]| {
        pitches
            .iter()
            .enumerate()
            .map(|(i, &p)| Note::new(q(i as i64), q(1), p, 100))
            .collect::<Vec<_>>()
    };
    let mut tunes = midi_track(4, "Tunes", "poly", (0, 6), None, Vec::new(), vec![]);
    tunes.clips = vec![
        clip_of(&tunes, 20, 0, melody(&[60, 62, 64, 65, 67, 69])),
        clip_of(&tunes, 21, 8, melody(&[67, 64, 60, 55, 52, 48])),
    ];
    let mut project = project_of("Octaves", vec![octave, tunes]);
    for id in [1, 4] {
        project
            .view
            .track_heights
            .insert(lumen_project::model::Id(id), 220.0);
        project.view.expanded.insert(lumen_project::model::Id(id));
    }
    project.view.detail_open = false;
    for appearance in ["dark", "light"] {
        let dir = TempDir::new("shots-octaves");
        let path = dir.path().join("octaves.lumen");
        lumen_project::save(&project, &path, &Default::default()).expect("save");
        let launch = Launch::for_test(dir.path())
            .appearing(appearance)
            .opening(path);
        let mut h = Harness::builder()
            .with_size(vec2(1200.0, 640.0))
            .with_step_dt(1.0 / 60.0)
            .wgpu()
            .build_eframe(|cc| {
                lumen_app::testing::install_renderer(cc).expect("the GPU renderer");
                App::new(cc, launch)
            });
        h.run_steps(60);
        save(&mut h, &format!("mini-notes-octaves-{appearance}"));
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn browser_controls() {
    // M6 slice 1: the Devices tab's controls (title, tab, search with its
    // focus ring, type switch, chips) in both themes at 100% and 200%.
    for appearance in ["dark", "light"] {
        for scale in [1.0, 2.0] {
            let dir = TempDir::new("shots-browser");
            let mut h = bare_scaled(&dir, appearance, scale, vec2(1280.0, 760.0));
            h.run_steps(40);
            h.key_press_modifiers(Modifiers::COMMAND, Key::F);
            h.run_steps(30);
            save(
                &mut h,
                &format!("browser-search-{appearance}-{}", (scale * 100.0) as u32),
            );
        }
    }
}

/// The Devices tab (M6 slice 2d): a chain with every card face, stopped and
/// playing, as the owner's C4 mock-up draws it.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn device_chain() {
    use egui_kittest::kittest::Queryable;
    use lumen_core::time::Ticks;
    use lumen_project::model::{Device, DeviceKind, Id, Note};
    use lumen_project::testing::{midi_track, project_of};

    let class = |n: u8| lumen_plugin_catalog::catalog::class_id(&[n; 16]);
    for (appearance, scale, size) in [
        ("dark", 1.0, vec2(1900.0, 900.0)),
        ("light", 1.0, vec2(1900.0, 900.0)),
        ("dark", 1.5, vec2(1500.0, 900.0)),
        ("dark", 2.0, vec2(1600.0, 1000.0)),
        ("light", 1.5, vec2(1500.0, 900.0)),
        ("light", 2.0, vec2(1600.0, 1000.0)),
        // The smallest window: no picture, the track's band on a rounded corner.
        ("dark", 1.0, vec2(960.0, 540.0)),
        ("light", 1.0, vec2(960.0, 540.0)),
    ] {
        let dir = TempDir::new("shots-chain");
        let mut track = midi_track(
            1,
            "Pad",
            "poly",
            (0, 64),
            None,
            vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, 100)],
            vec![],
        );
        track.devices = vec![
            Device::vst3(
                Id(10),
                &class(1),
                "Bass Synth",
                "Vendor C",
                DeviceKind::Instrument,
            ),
            Device::vst3(
                Id(11),
                &class(2),
                "Overheat",
                "Sampleson",
                DeviceKind::Effect,
            ),
            Device::vst3(
                Id(12),
                &class(3),
                "Tape Delay",
                "Vendor D",
                DeviceKind::Effect,
            ),
            Device::vst3(
                Id(13),
                &class(4),
                "Bus Comp",
                "Vendor C",
                DeviceKind::Effect,
            ),
            Device::vst3(
                Id(14),
                &class(5),
                "Soft Clip",
                "Vendor C",
                DeviceKind::Effect,
            ),
            Device::vst3(
                Id(15),
                &class(6),
                "Wide Chorus",
                "Vendor D",
                DeviceKind::Effect,
            ),
        ];
        track.devices[2].bypass = true;
        track.devices[5].state = vec![0; 14 * 1024];
        let project = project_of("Chain", vec![track]);
        let path = dir.path().join("song.lumen");
        lumen_project::save(&project, &path, &Default::default()).expect("save");
        let launch = Launch::for_test(dir.path())
            .appearing(appearance)
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
        h.run_steps(30);
        h.state_mut().select_tracks(&[Id(1)]);
        h.run_steps(4);
        // The browser has a Devices tab too: the detail panel's comes last.
        h.get_all_by_label("Devices tab")
            .last()
            .expect("the tabs")
            .click();
        h.run_steps(30);
        // Two cards with a picture of their own: a window with knobs.
        let ctx = h.ctx.clone();
        for (device, hue) in [(Id(11), [40u8, 90, 70]), (Id(13), [90, 60, 40])] {
            h.state_mut()
                .give_device_picture(&ctx, device, (384, 192), |x, y| {
                    let (dx, dy) = (x as f32 % 96.0 - 48.0, y as f32 % 96.0 - 48.0);
                    let knob = (dx * dx + dy * dy).sqrt() < 30.0 && x > 40 && y > 40;
                    let shade = 1.0 - y as f32 / 300.0;
                    if knob {
                        [220, 220, 210, 255]
                    } else {
                        [
                            (f32::from(hue[0]) * shade) as u8,
                            (f32::from(hue[1]) * shade) as u8,
                            (f32::from(hue[2]) * shade) as u8,
                            255,
                        ]
                    }
                });
        }
        h.state_mut().pretend_device_state(Id(10), "ready");
        h.state_mut().pretend_device_state(Id(11), "ready");
        h.state_mut().pretend_device_state(Id(12), "ready");
        h.state_mut().pretend_device_state(Id(13), "loading");
        h.state_mut().pretend_device_state(Id(14), "failed");
        // The strip: six parameters chosen, two slots empty.
        h.state_mut().give_device_parameters(
            Id(11),
            &[
                ("Cutoff", 0.62, "2.4 kHz"),
                ("Resonance", 0.30, "0.31"),
                ("Attack", 0.80, "12 ms"),
                ("Release", 0.45, "640 ms"),
                ("Wave", 0.5, "Saw"),
                ("Output level", 0.73, "-4.2 dB"),
            ],
        );
        h.state_mut().give_device_parameters(
            Id(12),
            &[("Time", 0.4, "310 ms"), ("Feedback", 0.55, "42 %")],
        );
        // The plugin's own window: open with Learn on; stuck.
        h.state_mut().give_device_editor(Id(11), "open", true);
        h.state_mut().give_device_editor(Id(12), "stuck", false);
        h.run_steps(4);
        h.get_by_label("Overheat on Pad").click();
        h.run_steps(20);
        let small = if size.x < 1000.0 { "-small" } else { "" };
        let tag = format!("{appearance}-{scale}{small}");
        save(&mut h, &format!("chain-stopped-{tag}"));
        h.state_mut().fold_device(Id(12));
        h.state_mut().fold_device(Id(14));
        h.run_steps(30);
        save(&mut h, &format!("chain-folded-{tag}"));
        h.state_mut().fold_device(Id(12));
        h.state_mut().fold_device(Id(14));
        h.run_steps(30);
        if appearance == "dark" && scale == 1.0 && small.is_empty() {
            h.key_press(Key::Space);
            for _ in 0..30 {
                std::thread::sleep(std::time::Duration::from_millis(40));
                h.run_steps(1);
            }
            save(&mut h, &format!("chain-playing-{tag}"));
        }
    }
}

/// The preset field on a card, its menu, and the failed faces (M6 slice 5f).
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn preset_menu() {
    use egui_kittest::kittest::Queryable;
    use lumen_core::time::Ticks;
    use lumen_project::model::{Device, DeviceKind, Id, Note};
    use lumen_project::testing::{midi_track, project_of};

    let class = |n: u8| lumen_plugin_catalog::catalog::class_id(&[n; 16]);
    for (appearance, scale, size) in [
        ("dark", 1.0, vec2(1500.0, 800.0)),
        ("light", 1.0, vec2(1500.0, 800.0)),
        ("dark", 1.5, vec2(1500.0, 900.0)),
        ("dark", 2.0, vec2(1900.0, 1000.0)),
        ("dark", 1.0, vec2(960.0, 540.0)),
        ("light", 2.0, vec2(1900.0, 1000.0)),
    ] {
        for menu in [false, true] {
            let dir = TempDir::new("shots-presets");
            let mut track = midi_track(
                1,
                "Pad",
                "poly",
                (0, 64),
                None,
                vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, 100)],
                vec![],
            );
            track.devices = vec![
                Device::vst3(
                    Id(10),
                    &class(1),
                    "Bass Synth",
                    "Vendor C",
                    DeviceKind::Instrument,
                ),
                Device::vst3(
                    Id(11),
                    &class(2),
                    "Overheat",
                    "Sampleson",
                    DeviceKind::Effect,
                ),
                Device::vst3(
                    Id(12),
                    &class(3),
                    "Tape Delay",
                    "Vendor D",
                    DeviceKind::Effect,
                ),
                Device::vst3(
                    Id(13),
                    &class(4),
                    "Bus Comp",
                    "Vendor C",
                    DeviceKind::Effect,
                ),
                Device::vst3(
                    Id(14),
                    &class(5),
                    "Soft Clip",
                    "Vendor C",
                    DeviceKind::Effect,
                ),
            ];
            let project = project_of("Presets", vec![track]);
            let path = dir.path().join("song.lumen");
            lumen_project::save(&project, &path, &Default::default()).expect("save");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
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
            h.run_steps(30);
            h.state_mut().select_tracks(&[Id(1)]);
            h.run_steps(4);
            h.get_all_by_label("Devices tab")
                .last()
                .expect("the tabs")
                .click();
            h.run_steps(20);
            for id in [10, 11, 12] {
                h.state_mut().pretend_device_state(Id(id), "ready");
            }
            h.state_mut().pretend_device_state(Id(13), "failed");
            h.state_mut().pretend_device_state(Id(14), "failed");
            h.state_mut().pretend_failures(Id(13), 3, Some(125));
            h.state_mut().pretend_failures(Id(14), 1, Some(40));
            let programs = ["Warm Pad", "Glass Keys", "Sub Bass", "Pluck Lead", "Drone"];
            let files = [
                (None, "Dusty Rhodes", true),
                (None, "Night Drive", true),
                (Some("Bass"), "Deep Wobble", false),
                (Some("Bass"), "Round Sub", false),
            ];
            h.state_mut()
                .give_device_presets(Id(10), &programs, "Warm Pad", false, &files);
            h.state_mut()
                .give_device_presets(Id(11), &programs, "Dusty Rhodes", true, &files);
            h.state_mut()
                .give_device_presets(Id(12), &programs, "Glass Keys", false, &files);
            h.run_steps(20);
            h.get_by_label("Overheat on Pad").click();
            h.run_steps(10);
            let small = if size.x < 1000.0 { "-small" } else { "" };
            let what = if menu { "menu" } else { "cards" };
            if menu {
                let ctx = h.ctx.clone();
                h.state_mut().open_preset_menu_of(&ctx, Id(11));
                h.run_steps(30);
            }
            save(
                &mut h,
                &format!("presets-{what}-{appearance}-{scale}{small}"),
            );
        }
    }
}

/// Switches and choosers on a card's strip (M6 slice 5g, the owner's A).
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn stepped_controls() {
    use egui_kittest::kittest::Queryable;
    use lumen_core::time::Ticks;
    use lumen_project::model::{Device, DeviceKind, Id, Note};
    use lumen_project::testing::{midi_track, project_of};

    let class = |n: u8| lumen_plugin_catalog::catalog::class_id(&[n; 16]);
    for (appearance, scale, size) in [
        ("dark", 1.0, vec2(1500.0, 800.0)),
        ("light", 1.0, vec2(1500.0, 800.0)),
        ("dark", 1.5, vec2(1500.0, 900.0)),
        ("dark", 2.0, vec2(1900.0, 1000.0)),
        ("light", 2.0, vec2(1900.0, 1000.0)),
    ] {
        for open in [false, true] {
            let dir = TempDir::new("shots-stepped");
            let mut track = midi_track(
                1,
                "Pad",
                "poly",
                (0, 64),
                None,
                vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, 100)],
                vec![],
            );
            track.devices = vec![
                Device::vst3(
                    Id(10),
                    &class(1),
                    "Bass Synth",
                    "Vendor C",
                    DeviceKind::Instrument,
                ),
                Device::vst3(
                    Id(11),
                    &class(2),
                    "Overheat",
                    "Sampleson",
                    DeviceKind::Effect,
                ),
            ];
            let project = project_of("Stepped", vec![track]);
            let path = dir.path().join("song.lumen");
            lumen_project::save(&project, &path, &Default::default()).expect("save");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
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
            h.run_steps(30);
            h.state_mut().select_tracks(&[Id(1)]);
            h.run_steps(4);
            h.get_all_by_label("Devices tab")
                .last()
                .expect("the tabs")
                .click();
            h.run_steps(20);
            h.state_mut().pretend_device_state(Id(10), "ready");
            h.state_mut().pretend_device_state(Id(11), "ready");
            h.state_mut().give_device_parameters(
                Id(11),
                &[
                    ("Cutoff", 0.62, "2.4 kHz"),
                    ("Resonance", 0.30, "0.31"),
                    ("Attack", 0.80, "12 ms"),
                ],
            );
            h.state_mut().give_device_stepped(
                Id(11),
                &[
                    ("Unison", 1, 1.0, &["Off", "On"]),
                    ("Retrig", 1, 0.0, &["Off", "On"]),
                    ("Wave", 3, 2.0 / 3.0, &["Sine", "Triangle", "Saw", "Square"]),
                    ("Mode", 2, 0.0, &["Poly", "Mono", "Legato"]),
                    (
                        "Filter",
                        5,
                        0.2,
                        &["LP 12", "LP 24", "HP", "BP", "Notch", "Peak"],
                    ),
                ],
            );
            h.run_steps(20);
            h.get_by_label("Overheat on Pad").click();
            h.run_steps(10);
            if open {
                h.get_by_label("Choose Wave").click();
                h.run_steps(20);
            }
            let what = if open { "list" } else { "strip" };
            save(&mut h, &format!("stepped-{what}-{appearance}-{scale}"));
        }
    }
}

/// Safe mode (M6 slice 5e, the owner's I2 and F2): the strip under the
/// transport bar, the card that is not loaded, and the offer.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn safe_mode() {
    use egui_kittest::kittest::Queryable;
    use lumen_core::time::Ticks;
    use lumen_project::model::{Device, DeviceKind, Id, Note};
    use lumen_project::testing::{midi_track, project_of};

    let class = |n: u8| lumen_plugin_catalog::catalog::class_id(&[n; 16]);
    for (appearance, scale, size) in [
        ("dark", 1.0, vec2(1500.0, 800.0)),
        ("light", 1.0, vec2(1500.0, 800.0)),
        ("dark", 1.0, vec2(960.0, 540.0)),
        ("dark", 2.0, vec2(1900.0, 1000.0)),
    ] {
        for offer in [false, true] {
            let dir = TempDir::new("shots-safe");
            let mut track = midi_track(
                1,
                "Pad",
                "poly",
                (0, 64),
                None,
                vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, 100)],
                vec![],
            );
            track.devices = vec![
                Device::vst3(
                    Id(10),
                    &class(1),
                    "Bass Synth",
                    "Vendor C",
                    DeviceKind::Instrument,
                ),
                Device::vst3(
                    Id(11),
                    &class(2),
                    "Overheat",
                    "Sampleson",
                    DeviceKind::Effect,
                ),
            ];
            track.devices[1].state = vec![0; 84 * 1024];
            let project = project_of("Safe", vec![track]);
            let path = dir.path().join("song.lumen");
            lumen_project::save(&project, &path, &Default::default()).expect("save");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
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
            h.run_steps(30);
            h.state_mut().select_tracks(&[Id(1)]);
            h.run_steps(4);
            h.get_all_by_label("Devices tab")
                .last()
                .expect("the tabs")
                .click();
            h.run_steps(20);
            h.state_mut().pretend_device_state(Id(10), "safe");
            h.state_mut().pretend_device_state(Id(11), "safe");
            h.state_mut().pretend_safe_mode(true, offer);
            h.run_steps(30);
            let small = if size.x < 1000.0 { "-small" } else { "" };
            let what = if offer { "offer" } else { "strip" };
            save(&mut h, &format!("safe-{what}-{appearance}-{scale}{small}"));
        }
    }
}

/// The audio panel's block budget (ADR-0075, the owner's B) with a long
/// chain: twelve plugins, two of them missing blocks.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn block_budget() {
    use egui_kittest::kittest::Queryable;
    let plugins = [
        ("Amp", "MIDI 1 · effect 1", 440, 900, 7),
        ("Synth", "MIDI 1 · instrument", 350, 520, 0),
        ("Reverb", "Vocal return · effect 1", 210, 270, 0),
        ("Drum sampler", "Drums · instrument", 140, 190, 2),
        ("Compressor", "Drums · effect 1", 90, 130, 0),
        ("Equalizer", "Bass · effect 1", 50, 80, 0),
        ("Delay", "Vocal return · effect 2", 40, 70, 0),
        ("Limiter", "Master · effect 1", 40, 60, 0),
        ("Saturator", "Bass · effect 2", 30, 50, 0),
        ("Chorus", "Keys · effect 1", 20, 40, 0),
        ("Noise gate", "Guitar · effect 1", 20, 40, 0),
        ("Tuner", "Guitar · effect 2", 10, 30, 0),
    ];
    let sizes = [
        ("dark", 1.0_f32, vec2(1400.0, 900.0)),
        ("dark", 1.5, vec2(1400.0, 900.0)),
        ("dark", 2.0, vec2(1400.0, 900.0)),
        ("light", 1.0, vec2(1400.0, 900.0)),
        ("light", 1.5, vec2(1400.0, 900.0)),
        ("light", 2.0, vec2(1400.0, 900.0)),
        // The smallest window: the panel must fit, its list shorter.
        ("dark", 1.0, vec2(960.0, 540.0)),
    ];
    for (appearance, scale, size) in sizes {
        {
            let dir = TempDir::new("shots-budget");
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
            h.run_steps(30);
            h.state_mut().pretend_budget(2_902, 2_560, &plugins);
            h.get_by_label_contains("Audio: ").click();
            h.run_steps(20);
            save(
                &mut h,
                &format!(
                    "budget-{appearance}-{}-{}",
                    (scale * 100.0) as u32,
                    size.y as u32
                ),
            );
        }
    }
}

/// The UI polish pass: an armed folded track clear of its level hairline,
/// the panel toggles as icons beside the palette, the keycap with the
/// computer keyboard on, the narrowest browser, and the Devices tab at its
/// shortest.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn ui_polish() {
    use egui_kittest::kittest::Queryable;
    use lumen_project::testing::{midi_track, project_of};
    for (appearance, scale) in [("dark", 1.0), ("light", 1.0), ("dark", 2.0)] {
        let dir = TempDir::new("shots-polish");
        let project = project_of(
            "Polish",
            vec![
                midi_track(1, "Harp", "poly", (0, 8), None, vec![], vec![]),
                midi_track(11, "Flute", "poly", (0, 8), None, vec![], vec![]),
            ],
        );
        let path = dir.path().join("song.lumen");
        lumen_project::save(&project, &path, &Default::default()).expect("save the project");
        let launch = Launch::for_test(dir.path())
            .appearing(appearance)
            .opening(path);
        let mut h = Harness::builder()
            .with_size(vec2(1280.0, 800.0))
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
        h.run_steps(60);
        h.get_by_label("Arm Flute").click();
        h.run_steps(20);
        h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::K);
        h.run_steps(20);
        let tag = format!("{appearance}-{scale}");
        save(&mut h, &format!("polish-main-{tag}"));
        h.state_mut().set_browser_size(40.0);
        let harp = h.state().project().tracks[0].id;
        h.state_mut().select_tracks(&[harp]);
        h.run_steps(10);
        h.get_all_by_label("Devices tab").last().unwrap().click();
        h.run_steps(20);
        h.state_mut().set_detail_size(40.0);
        h.run_steps(30);
        save(&mut h, &format!("polish-devices-{tag}"));
    }
}

/// The sidechain row on a plugin effect's card (M6 slice 6c, the owner's B
/// with the light of I1): unset, keyed with the light lit, keyed with the
/// warning ring, and the menu open.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn sidechain_row() {
    use egui_kittest::kittest::Queryable;
    use lumen_core::time::Ticks;
    use lumen_project::model::{Device, DeviceKind, Id, Note, Sidechain, SidechainTap};
    use lumen_project::testing::{midi_track, project_of};

    let class = |n: u8| lumen_plugin_catalog::catalog::class_id(&[n; 16]);
    for (appearance, scale, size) in [
        ("dark", 1.0, vec2(1500.0, 800.0)),
        ("light", 1.0, vec2(1500.0, 800.0)),
        ("dark", 1.5, vec2(1500.0, 900.0)),
        ("dark", 2.0, vec2(1900.0, 1000.0)),
        ("light", 2.0, vec2(1900.0, 1000.0)),
        ("dark", 1.0, vec2(960.0, 540.0)),
    ] {
        for (what, keyed, level, silent, open) in [
            ("none", false, 0.0, 0.0, false),
            ("lit", true, 0.8, 0.0, false),
            ("quiet", true, 0.0, 3.0, false),
            ("menu", true, 0.5, 0.0, true),
        ] {
            let dir = TempDir::new("shots-sidechain");
            let kick = midi_track(
                2,
                "Kick",
                "poly",
                (0, 64),
                None,
                vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 36, 100)],
                vec![],
            );
            let mut pad = midi_track(
                20,
                "Pad",
                "poly",
                (0, 64),
                None,
                vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, 100)],
                vec![],
            );
            let mut effect = Device::vst3(
                Id(31),
                &class(2),
                "Compressor A",
                "Vendor A",
                DeviceKind::Effect,
            );
            if keyed {
                effect.sidechain = Some(Sidechain {
                    from: Id(2),
                    tap: SidechainTap::PostFx,
                });
            }
            pad.devices = vec![
                Device::vst3(
                    Id(30),
                    &class(1),
                    "Bass Synth",
                    "Vendor C",
                    DeviceKind::Instrument,
                ),
                effect,
            ];
            let project = project_of("Sidechain", vec![kick, pad]);
            let path = dir.path().join("song.lumen");
            lumen_project::save(&project, &path, &Default::default()).expect("save");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
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
            h.run_steps(30);
            h.state_mut().select_tracks(&[Id(20)]);
            h.run_steps(4);
            h.get_all_by_label("Devices tab")
                .last()
                .expect("the tabs")
                .click();
            h.run_steps(20);
            h.state_mut().pretend_device_state(Id(30), "ready");
            h.state_mut().pretend_device_state(Id(31), "ready");
            h.state_mut().give_device_parameters(
                Id(31),
                &[
                    ("Threshold", 0.62, "-18 dB"),
                    ("Ratio", 0.40, "4:1"),
                    ("Attack", 0.25, "10 ms"),
                    ("Release", 0.55, "120 ms"),
                    ("Knee", 0.30, "6 dB"),
                    ("Makeup", 0.45, "+3 dB"),
                    ("Mix", 1.0, "100%"),
                    ("Output", 0.70, "0 dB"),
                ],
            );
            h.state_mut()
                .give_device_sidechain(Id(31), Id(20), level, silent);
            h.run_steps(20);
            h.get_by_label("Compressor A on Pad").click();
            h.run_steps(10);
            if keyed {
                // The song plays: the light shows what arrives.
                h.key_press(eframe::egui::Key::Space);
                h.run_steps(20);
            }
            if open {
                h.get_by_label("Sidechain source, Kick, Post FX").click();
                h.run_steps(20);
            }
            save(
                &mut h,
                &format!("sidechain-{what}-{appearance}-{scale}-{}", size.x as u32),
            );
        }
    }
}

/// The card's Outputs row and its menu: the list scrolls with room for its
/// bar beside the switches, and a switched-on output names its track.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn outputs_row() {
    use egui_kittest::kittest::Queryable;
    use lumen_core::time::Ticks;
    use lumen_project::model::{
        Color, Device, DeviceKind, Id, Note, PluginOutput, Track, TrackKind,
    };
    use lumen_project::testing::{midi_track, project_of};

    let class = |n: u8| lumen_plugin_catalog::catalog::class_id(&[n; 16]);
    let names: Vec<String> = (2..=16).map(|n| format!("St.{n}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    for (appearance, scale, size) in [
        ("dark", 1.0, vec2(1500.0, 800.0)),
        ("light", 1.0, vec2(1500.0, 800.0)),
        ("dark", 2.0, vec2(1900.0, 1000.0)),
        ("dark", 1.0, vec2(960.0, 540.0)),
    ] {
        for (what, open, on) in [("closed", false, 2), ("menu", true, 2), ("all", true, 15)] {
            let dir = TempDir::new("shots-outputs");
            let mut strings = midi_track(
                20,
                "Strings",
                "poly",
                (0, 64),
                None,
                vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 60, 100)],
                vec![],
            );
            strings.devices = vec![Device::vst3(
                Id(30),
                &class(1),
                "Orchestra A",
                "Vendor A",
                DeviceKind::Instrument,
            )];
            let mut tracks = vec![strings];
            for bus in 1..=on {
                let id = 40 + u64::from(bus);
                let mut t = Track::new(
                    Id(id),
                    format!("Orchestra A \u{b7} St.{}", bus + 1),
                    TrackKind::Audio,
                    Color(0x5FA89A),
                );
                t.plugin_output = Some(PluginOutput {
                    device: Id(30),
                    bus,
                });
                tracks.push(t);
            }
            let project = project_of("Outputs", tracks);
            let path = dir.path().join("song.lumen");
            lumen_project::save(&project, &path, &Default::default()).expect("save");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
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
            h.run_steps(30);
            h.state_mut().select_tracks(&[Id(20)]);
            h.run_steps(4);
            h.get_all_by_label("Devices tab")
                .last()
                .expect("the tabs")
                .click();
            h.run_steps(20);
            h.state_mut().pretend_device_state(Id(30), "ready");
            h.state_mut().give_device_outputs(Id(30), &names);
            h.run_steps(20);
            if open {
                h.get_by_label_contains(&format!("Outputs, Main + {on}"))
                    .click();
                h.run_steps(20);
            }
            save(
                &mut h,
                &format!("outputs-{what}-{appearance}-{scale}-{}", size.x as u32),
            );
        }
    }
}

/// Automation lanes over the clips (M7 slice A1): a volume lane with a
/// curve and a picked point on the first track, a pan lane that is switched
/// off on the second, the A button lit in the unfolded headers; both themes
/// at 100, 150 and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn automation_lanes() {
    use lumen_core::automation::{Lane, Point, Shape};
    use lumen_core::time::Ticks;
    use lumen_project::automation::Target;
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new(&format!("shots-lanes-{appearance}-{scale}"));
            let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1280.0, 760.0));
            let tracks: Vec<_> = h.state().project().tracks.iter().map(|t| t.id).collect();
            let q = Ticks::quarters;
            let volume = Lane::new(vec![
                Point::new(q(0), 0.0),
                Point::new(q(2), -3.0).with_shape(Shape::Curve(Default::default())),
                Point::new(q(5), -18.0),
                Point::new(q(7), -18.0).with_shape(Shape::Step),
                Point::new(q(9), 0.0),
            ])
            .unwrap();
            let pan = Lane::new(vec![Point::new(q(1), -0.6), Point::new(q(6), 0.5)]).unwrap();
            h.state_mut()
                .give_lane(tracks[0], Target::Volume, volume)
                .unwrap();
            h.state_mut()
                .give_lane(tracks[1], Target::Pan, pan)
                .unwrap();
            h.state_mut()
                .switch_lane(tracks[1], Target::Pan, false)
                .unwrap();
            h.state_mut().select_tracks(&tracks[..2]);
            h.run_steps(4);
            h.key_press(Key::A);
            h.run_steps(60);
            // Pick the third point of the first lane, as a click would.
            let p = h
                .state()
                .automation_point_pos(&h.ctx, tracks[0], 2)
                .expect("in view");
            click(&mut h, p);
            h.run_steps(10);
            save(&mut h, &format!("lanes-{appearance}-{scale}"));
        }
    }
}

/// Editing a lane (M7 slice A2): the tool strip, a picked point's bend dot,
/// two points picked by a box, a sine being drawn with its live value.
/// A clip's gain envelope over the clip (M7 slice A7, the owner's E1 A): the
/// envelope of a looping clip with its repeat dashed, and its pan. Dark and
/// light at 100, 150 and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn clip_envelope() {
    use lumen_core::automation::{Lane, Point};
    use lumen_core::time::Ticks;
    use lumen_project::automation::Target;
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new(&format!("shots-env-{appearance}-{scale}"));
            let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1280.0, 760.0));
            let track = h.state().project().tracks[0].id;
            let clip = h.state().project().tracks[0].clips[0].id;
            let q = Ticks::quarters;
            // The clip repeats every three quarters for eleven.
            h.state_mut().trim_clip_end_to(clip, q(11), true);
            h.state_mut().select_clip(clip);
            h.run_steps(4);
            h.key_press(Key::E);
            h.run_steps(60);
            let start = h.state().project().tracks[0].clips[0].start;
            let gain = Lane::new(vec![
                Point::new(start, 0.0),
                Point::new(start + q(1), -18.0),
                Point::new(start + q(2), -6.0),
                Point::new(start + q(3), -30.0),
            ])
            .unwrap();
            h.state_mut()
                .give_lane(track, Target::ClipGain { clip }, gain)
                .unwrap();
            h.run_steps(10);
            let p = h
                .state()
                .automation_point_pos(&h.ctx, track, 1)
                .expect("in view");
            click(&mut h, p);
            h.run_steps(10);
            save(&mut h, &format!("clip-envelope-gain-{appearance}-{scale}"));
            let pan =
                Lane::new(vec![Point::new(start, -1.0), Point::new(start + q(2), 1.0)]).unwrap();
            h.state_mut()
                .give_lane(track, Target::ClipPan { clip }, pan)
                .unwrap();
            h.run_steps(5);
            h.state_mut()
                .show_clip_envelope(track, Target::ClipPan { clip });
            h.run_steps(30);
            save(&mut h, &format!("clip-envelope-pan-{appearance}-{scale}"));
        }
    }
}

/// Dark and light at 100, 150 and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn automation_editing() {
    use egui_kittest::kittest::Queryable as _;
    use lumen_core::automation::{Lane, Point};
    use lumen_core::time::Ticks;
    use lumen_project::automation::Target;
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new(&format!("shots-edit-{appearance}-{scale}"));
            let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1280.0, 760.0));
            let tracks: Vec<_> = h.state().project().tracks.iter().map(|t| t.id).collect();
            let name = h.state().project().tracks[0].name.clone();
            let q = Ticks::quarters;
            let volume = Lane::new(vec![
                Point::new(q(1), -24.0),
                Point::new(q(3), -6.0),
                Point::new(q(5), -18.0),
                Point::new(q(7), -3.0),
                Point::new(q(9), -30.0),
            ])
            .unwrap();
            h.state_mut()
                .give_lane(tracks[0], Target::Volume, volume)
                .unwrap();
            h.state_mut().select_tracks(&tracks[..1]);
            h.run_steps(4);
            h.key_press(Key::A);
            h.run_steps(60);
            // One point picked: its segment shows the bend dot.
            let p = h
                .state()
                .automation_point_pos(&h.ctx, tracks[0], 1)
                .expect("in view");
            click(&mut h, p);
            h.run_steps(10);
            save(&mut h, &format!("lane-edit-bend-{appearance}-{scale}"));
            // A box around two points, still held.
            let from = h
                .state()
                .automation_spot(&h.ctx, tracks[0], 4.0, 6.0)
                .expect("in view");
            let to = h
                .state()
                .automation_spot(&h.ctx, tracks[0], 8.0, -40.0)
                .expect("in view");
            h.event(Event::ModifiersChanged(Modifiers::SHIFT));
            h.hover_at(from);
            h.drag_at(from);
            h.run_steps(2);
            h.hover_at(to);
            h.run_steps(3);
            save(&mut h, &format!("lane-edit-box-{appearance}-{scale}"));
            h.drop_at(to);
            h.run_steps(3);
            h.event(Event::ModifiersChanged(Modifiers::NONE));
            h.run_steps(2);
            // The shape tool, a sine part drawn.
            h.get_by_label(&format!("Automation shape tool of {name}"))
                .click();
            h.run_steps(5);
            let from = h
                .state()
                .automation_spot(&h.ctx, tracks[0], 10.0, -30.0)
                .expect("in view");
            let to = h
                .state()
                .automation_spot(&h.ctx, tracks[0], 14.0, -6.0)
                .expect("in view");
            h.hover_at(from);
            h.drag_at(from);
            h.run_steps(2);
            h.hover_at(to);
            h.run_steps(4);
            save(&mut h, &format!("lane-edit-shape-{appearance}-{scale}"));
            h.drop_at(to);
            h.run_steps(3);
        }
    }
}

/// A lane taken over (M7 slice A4a, the owner's look A): the first track's
/// fader was moved under its volume lane, so its fader, chip, A button and lane
/// are amber and dashed; the second track's pan lane plays and its pan follows.
/// Dark and light at 100, 150 and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn automation_override() {
    use egui_kittest::kittest::Queryable as _;
    use lumen_core::automation::{Lane, Point, Shape};
    use lumen_core::time::Ticks;
    use lumen_project::automation::Target;
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new(&format!("shots-override-{appearance}-{scale}"));
            let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1280.0, 760.0));
            let tracks: Vec<_> = h.state().project().tracks.iter().map(|t| t.id).collect();
            let names: Vec<_> = h
                .state()
                .project()
                .tracks
                .iter()
                .map(|t| t.name.clone())
                .collect();
            let q = Ticks::quarters;
            let volume = Lane::new(vec![
                Point::new(q(0), -24.0),
                Point::new(q(3), -6.0).with_shape(Shape::Curve(Default::default())),
                Point::new(q(6), -18.0),
                Point::new(q(9), -3.0),
            ])
            .unwrap();
            let pan = Lane::new(vec![Point::new(q(1), -0.6), Point::new(q(8), 0.5)]).unwrap();
            h.state_mut()
                .give_lane(tracks[0], Target::Volume, volume)
                .unwrap();
            h.state_mut()
                .give_lane(tracks[1], Target::Pan, pan)
                .unwrap();
            h.state_mut().select_tracks(&tracks[..2]);
            h.run_steps(4);
            for name in &names[..2] {
                h.get_by_label(&format!("Fold {name}")).click();
                h.run_steps(30);
            }
            h.key_press(Key::A);
            h.run_steps(60);
            // The playhead partway along, the first fader moved by hand.
            h.state_mut().set_track_volume(tracks[0], -2.0);
            h.run_steps(20);
            save(&mut h, &format!("override-{appearance}-{scale}"));
        }
    }
}

/// A track's recording mode (M7 slice A4b, the owner's K1 B): one chip with a
/// record-red dot and the mode's name, the lane tinted red while the mode can
/// write, and the mode menu open on the first track. Dark and light at 100, 150
/// and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn automation_modes() {
    use egui_kittest::kittest::Queryable as _;
    use lumen_core::automation::{Lane, Point};
    use lumen_core::time::Ticks;
    use lumen_project::automation::Target;
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new(&format!("shots-modes-{appearance}-{scale}"));
            let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1280.0, 760.0));
            let tracks: Vec<_> = h.state().project().tracks.iter().map(|t| t.id).collect();
            let names: Vec<_> = h
                .state()
                .project()
                .tracks
                .iter()
                .map(|t| t.name.clone())
                .collect();
            let q = Ticks::quarters;
            let volume = Lane::new(vec![
                Point::new(q(0), -18.0),
                Point::new(q(4), -6.0),
                Point::new(q(9), -12.0),
            ])
            .unwrap();
            let pan = Lane::new(vec![Point::new(q(1), -0.6), Point::new(q(8), 0.5)]).unwrap();
            h.state_mut()
                .give_lane(tracks[0], Target::Volume, volume)
                .unwrap();
            h.state_mut()
                .give_lane(tracks[1], Target::Pan, pan)
                .unwrap();
            h.state_mut().set_automation_mode(tracks[0], "touch");
            h.state_mut().set_automation_mode(tracks[1], "write");
            h.state_mut().select_tracks(&tracks[..2]);
            h.run_steps(4);
            h.key_press(Key::A);
            h.run_steps(60);
            save(&mut h, &format!("modes-{appearance}-{scale}"));
            h.get_by_label(&format!(
                "Automation lane of {}: Volume \u{00B7} Touch",
                names[0]
            ))
            .click();
            h.run_steps(20);
            save(&mut h, &format!("modes-menu-{appearance}-{scale}"));
        }
    }
}

/// A plugin parameter's lane taken over (M7 slice A4a): the card's strip knob
/// and the All parameters window's bar, both with the amber ring, beside a
/// parameter whose lane plays and follows. Dark and light at 100, 150 and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn automation_override_parameters() {
    use egui_kittest::kittest::Queryable as _;
    use lumen_core::automation::{Lane, Point};
    use lumen_core::time::Ticks;
    use lumen_project::automation::Target;
    use lumen_project::model::{Device, DeviceKind, Id, Note};
    use lumen_project::testing::{midi_track, project_of};

    let class = |n: u8| lumen_plugin_catalog::catalog::class_id(&[n; 16]);
    let words = ["Cutoff", "Resonance", "Drive", "Mix", "Attack", "Release"];
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new(&format!("shots-override-params-{appearance}-{scale}"));
            let mut track = midi_track(
                1,
                "Pad",
                "poly",
                (0, 64),
                None,
                vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, 100)],
                vec![],
            );
            track.devices = vec![Device::vst3(
                Id(11),
                &class(2),
                "Bus Comp",
                "Vendor C",
                DeviceKind::Effect,
            )];
            let project = project_of("Override", vec![track]);
            let path = dir.path().join("song.lumen");
            lumen_project::save(&project, &path, &Default::default()).expect("save");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
                .opening(path);
            let mut h = Harness::builder()
                .with_size(vec2(1280.0, 760.0))
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
            h.run_steps(30);
            h.state_mut().pretend_device_state(Id(11), "ready");
            let list: Vec<(&str, f64, &str)> = words.iter().map(|w| (*w, 0.5, "50%")).collect();
            h.state_mut().give_device_parameters(Id(11), &list);
            h.state_mut().select_tracks(&[Id(1)]);
            h.run_steps(4);
            // The first two parameters have lanes: the first is taken over,
            // the second plays and its knob follows.
            let q = Ticks::quarters;
            for (param, from, to) in [(1, 0.2, 0.8), (2, 0.7, 0.3)] {
                let lane = Lane::new(vec![Point::new(q(0), from), Point::new(q(8), to)]).unwrap();
                h.state_mut()
                    .give_lane(
                        Id(1),
                        Target::Device {
                            device: Id(11),
                            param,
                        },
                        lane,
                    )
                    .unwrap();
            }
            h.state_mut().set_device_param(Id(11), 1, 0.5);
            h.get_all_by_label("Devices tab")
                .last()
                .expect("the detail panel's tab")
                .click();
            h.run_steps(30);
            save(&mut h, &format!("override-card-{appearance}-{scale}"));
            h.get_by_label_contains("All parameters (").click();
            h.run_steps(20);
            save(&mut h, &format!("override-panel-{appearance}-{scale}"));
        }
    }
}

/// Choosing a plugin's parameter for a lane (M7 slice A3): the chooser open
/// on a plugin with a dozen parameters, a search narrowing it, and the lane
/// it chose drawn under its name. Dark and light at 100, 150 and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn automation_plugin_chooser() {
    use egui_kittest::kittest::Queryable as _;
    use lumen_core::automation::{Lane, Point};
    use lumen_core::time::Ticks;
    use lumen_project::model::{Device, DeviceKind, Id, Note};
    use lumen_project::testing::{midi_track, project_of};

    let class = |n: u8| lumen_plugin_catalog::catalog::class_id(&[n; 16]);
    let words = [
        "Cutoff",
        "Resonance",
        "Drive",
        "Mix",
        "Attack",
        "Release",
        "Envelope amount",
        "LFO rate",
        "LFO depth",
        "Stereo width",
        "Output level",
        "Filter type",
    ];
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new(&format!("shots-chooser-{appearance}-{scale}"));
            let mut track = midi_track(
                1,
                "Pad",
                "poly",
                (0, 64),
                None,
                vec![Note::new(Ticks::ZERO, Ticks::quarters(60), 69, 100)],
                vec![],
            );
            track.devices = vec![
                Device::vst3(
                    Id(10),
                    &class(1),
                    "Bass Synth",
                    "Vendor C",
                    DeviceKind::Instrument,
                ),
                Device::vst3(
                    Id(11),
                    &class(2),
                    "Bus Comp",
                    "Vendor C",
                    DeviceKind::Effect,
                ),
            ];
            let project = project_of("Chooser", vec![track]);
            let path = dir.path().join("song.lumen");
            lumen_project::save(&project, &path, &Default::default()).expect("save");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
                .opening(path);
            let mut h = Harness::builder()
                .with_size(vec2(1280.0, 760.0))
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
            h.run_steps(30);
            h.state_mut().pretend_device_state(Id(11), "ready");
            let list: Vec<(&str, f64, &str)> = words.iter().map(|w| (*w, 0.5, "50%")).collect();
            h.state_mut().give_device_parameters(Id(11), &list);
            h.state_mut().select_tracks(&[Id(1)]);
            h.run_steps(4);
            h.key_press(Key::A);
            h.run_steps(60);
            h.get_by_label("Automation lane of Pad: Volume").click();
            h.run_steps(5);
            h.get_by_label("Parameters of Bus Comp\u{2026}").click();
            h.run_steps(20);
            save(&mut h, &format!("chooser-open-{appearance}-{scale}"));
            h.event(Event::Text("lfo".into()));
            h.run_steps(10);
            save(&mut h, &format!("chooser-search-{appearance}-{scale}"));
            h.key_press(Key::Enter);
            h.run_steps(10);
            let q = Ticks::quarters;
            let lane = Lane::new(vec![
                Point::new(q(1), 0.2),
                Point::new(q(4), 0.8),
                Point::new(q(8), 0.35),
            ])
            .unwrap();
            let shown = h.state().automation_shown(Id(1)).expect("a lane is shown");
            h.state_mut().give_lane(Id(1), shown, lane).unwrap();
            h.run_steps(20);
            save(&mut h, &format!("chooser-lane-{appearance}-{scale}"));
        }
    }
}

/// Controller mappings (M7 slice A5): the right-click menu of a mapped control
/// and the Mappings window with a control waiting for a controller. Dark and
/// light at 100, 150 and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn controller_mappings() {
    use eframe::egui::{Event, PointerButton};
    use egui_kittest::kittest::Queryable as _;
    use lumen_project::automation::Target;
    use lumen_project::model::{Color, Id, Track, TrackKind};
    use lumen_project::testing::project_of;

    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new(&format!("shots-mappings-{appearance}-{scale}"));
            let project = project_of(
                "Mappings",
                vec![
                    Track::new(Id(1), "Drums", TrackKind::Audio, Color(0x4F81BD)),
                    Track::new(Id(2), "Bass", TrackKind::Audio, Color(0xC0504D)),
                    Track::new(
                        Id(3),
                        "A very long vocal track name",
                        TrackKind::Audio,
                        Color(0x9BBB59),
                    ),
                ],
            );
            let path = dir.path().join("song.lumen");
            lumen_project::save(&project, &path, &Default::default()).expect("save");
            let launch = Launch::for_test(dir.path())
                .appearing(appearance)
                .opening(path);
            let mut h = Harness::builder()
                .with_size(vec2(1280.0, 760.0))
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
            h.run_steps(40);
            {
                let app = h.state_mut();
                app.give_mapping(Id(1), Target::Volume, 0, 7).unwrap();
                app.give_mapping(Id(2), Target::Pan, 0, 10).unwrap();
                app.give_mapping(Id(3), Target::Volume, 9, 74).unwrap();
                app.select_tracks(&[Id(1)]);
            }
            h.run_steps(30);
            h.get_by_label("Fold Drums").click();
            h.run_steps(40);
            // The menu of a mapped control.
            let slider = h.get_by_label_contains("Volume Drums:").rect();
            h.hover_at(slider.center());
            h.run_steps(2);
            for pressed in [true, false] {
                h.event(Event::PointerButton {
                    pos: slider.center(),
                    button: PointerButton::Secondary,
                    pressed,
                    modifiers: Default::default(),
                });
                h.run_steps(2);
            }
            h.run_steps(15);
            save(&mut h, &format!("mappings-menu-{appearance}-{scale}"));
            h.key_press(Key::Escape);
            h.run_steps(15);
            // The window, with the bass pan waiting for another controller.
            h.state_mut().begin_learning(Id(2), Target::Volume);
            h.run_steps(5);
            h.key_press_modifiers(Modifiers::COMMAND, Key::K);
            h.run_steps(5);
            h.event(Event::Text("Controller mappings".into()));
            h.run_steps(5);
            h.key_press(Key::Enter);
            h.run_steps(30);
            save(&mut h, &format!("mappings-window-{appearance}-{scale}"));
        }
    }
}

/// The tempo and meter lanes (M7 slice A6): three tempos with their values,
/// a picked change, time signatures 4/4, 7/8 and 3/4 as chips, the playhead
/// in the second segment so the name column reads that tempo and meter, and
/// the box a tempo is typed in. Dark and light at 100, 150 and 200%.
#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn song_lanes() {
    for appearance in ["dark", "light"] {
        for scale in [1.0_f32, 1.5, 2.0] {
            let dir = TempDir::new(&format!("shots-song-{appearance}-{scale}"));
            let mut h = lumen_looking_sized(&dir, appearance, scale, vec2(1280.0, 760.0));
            h.state_mut()
                .give_song_maps(&[(8, 96.5), (20, 140.0)], &[(8, 7, 8), (15, 3, 4)]);
            // The corner button, with the lanes hidden and then lit.
            h.run_steps(30);
            save(&mut h, &format!("song-lanes-hidden-{appearance}-{scale}"));
            h.state_mut().ramp_song_tempo(1);
            h.state_mut().show_song_lanes(true);
            h.run_steps(60);
            // Pick the second tempo change, as a click would, and put the
            // playhead past it with a click on the bar ruler.
            let p = h.state().song_tempo_spot(8.0, 96.5).expect("drawn");
            click(&mut h, p);
            h.run_steps(10);
            save(&mut h, &format!("song-lanes-{appearance}-{scale}"));
            // Type its tempo.
            h.event(Event::PointerButton {
                pos: p,
                button: eframe::egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            });
            h.event(Event::PointerButton {
                pos: p,
                button: eframe::egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            });
            h.run_steps(30);
            save(&mut h, &format!("song-lanes-typing-{appearance}-{scale}"));
        }
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn draw_clip_aim() {
    for appearance in ["dark", "light"] {
        for scale in [1.0, 1.5, 2.0] {
            draw_clip_aim_looking(appearance, scale);
        }
    }
}

fn draw_clip_aim_looking(appearance: &str, scale: f32) {
    use egui_kittest::kittest::Queryable;
    let dir = TempDir::new("shots-draw");
    let mut h = lumen_looking(&dir, appearance, scale);
    h.run_steps(20);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    h.run_steps(40);
    let last = h.state().project().tracks.len() - 1;
    let name = h.state().project().tracks[last].name.clone();
    let y = h
        .get_by_label_contains(&format!("Track {name},"))
        .rect()
        .center()
        .y;
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.event(Event::Text("Arrangement draw tool".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    let a = clip(&h, 0);
    let from = Pos2::new(a.left() + a.width() * 0.5, y);
    let to = Pos2::new(a.left() + a.width() * 2.0, y);
    h.hover_at(from);
    h.drag_at(from);
    h.run_steps(2);
    h.hover_at(to);
    h.run_steps(3);
    save(
        &mut h,
        &format!("draw-aim-{appearance}-{}", (scale * 100.0) as u32),
    );
    h.key_press(Key::Escape);
    h.drop_at(to);
}
