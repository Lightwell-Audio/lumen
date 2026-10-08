//! Renders states a `--screenshot` cannot reach (hovers, drags, selections)
//! to PNG files, for checking the look by eye. Not a test of behaviour: it
//! is ignored, and runs on demand with a GPU:
//!
//! ```text
//! SHOTS=<folder> cargo test -p lumen-app --test shots -- --ignored
//! ```

use std::path::PathBuf;

use eframe::egui::{Event, Key, Modifiers, Pos2, Rect, vec2};
use egui_kittest::Harness;
use lumen_app::{App, Launch};
use lumen_project::testing::TempDir;

/// A window with two one-second clips on a one-bar grid.
fn lumen(dir: &TempDir) -> Harness<'static, App> {
    lumen_looking(dir, "dark", 1.0)
}

/// [`lumen`] in `appearance`, at `scale`.
fn lumen_looking(dir: &TempDir, appearance: &str, scale: f32) -> Harness<'static, App> {
    let launch = Launch::for_test(dir.path()).appearing(appearance);
    let mut h = Harness::builder()
        .with_size(vec2(1200.0, 420.0))
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
    let files: Vec<PathBuf> = ["kick", "snare", "bass"]
        .iter()
        .map(|name| wav(&dir.path().join(format!("{name}.wav")), 1.5))
        .collect();
    h.input_mut().dropped_files = files
        .into_iter()
        .map(|p| std::sync::Arc::new(Dropped(p)) as eframe::egui::DroppedFileHandle)
        .collect();
    h.run_steps(60);
    h
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
