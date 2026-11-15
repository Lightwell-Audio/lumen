use std::path::PathBuf;
use std::time::Duration;

use eframe::egui::{Key, Modifiers, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use lumen_app::{App, Launch};
use lumen_core::midi::Msg;
use lumen_project::testing::TempDir;

fn lumen(dir: &TempDir, appearance: &str, scale: f32) -> Harness<'static, App> {
    let launch = Launch::for_test(dir.path()).appearing(appearance);
    let mut h = Harness::builder()
        .with_size(vec2(1280.0, 560.0))
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

fn save<S>(h: &mut Harness<'_, S>, name: &str) {
    h.run_steps(3);
    let image = h.render().expect("render");
    let dir = PathBuf::from(std::env::var("SHOTS").expect("SHOTS names a folder"));
    image.save(dir.join(format!("{name}.png"))).expect("save");
}

fn play(h: &mut Harness<'static, App>, millis: u64) {
    let steps = millis / 10;
    for i in 0..steps {
        std::thread::sleep(Duration::from_millis(10));
        h.run_steps(1);
        let pitch = 60 + (i / 20 % 12) as u8;
        if i % 20 == 0 {
            h.state_mut().play_live(Msg::note_on(0, pitch, 100));
        } else if i % 20 == 10 {
            h.state_mut().play_live(Msg::note_off(0, pitch));
        }
    }
}

#[test]
#[ignore = "renders PNGs for a visual check; needs a GPU and SHOTS"]
fn recording_take() {
    for (appearance, scale) in [("dark", 1.0), ("light", 1.0), ("light", 1.5)] {
        let dir = TempDir::new("shots-recording");
        let mut h = lumen(&dir, appearance, scale);
        h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
        h.run_steps(5);
        let name = h.state().project().tracks[0].name.clone();
        h.get_by_label(&format!("Arm {name}")).click();
        h.run_steps(5);
        for _ in 0..3 {
            h.key_press(Key::Equals);
            h.run_steps(5);
        }
        h.key_press(Key::F9);
        play(&mut h, 4000);
        save(&mut h, &format!("recording-{appearance}-{scale}"));
        h.key_press(Key::F9);
        h.run_steps(30);
        save(&mut h, &format!("recorded-{appearance}-{scale}"));
    }
}
