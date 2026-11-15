use std::time::Instant;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;
use lumen_project::model::{Id, PluginRef};

use super::Lumen;
use super::lane_view::{run_command, tone};

fn measure(lumen: &mut Lumen, frames: usize) -> (f64, f64, f64) {
    lumen.frames(20);
    let mut times = Vec::with_capacity(frames);
    for _ in 0..frames {
        let start = Instant::now();
        lumen.frames(1);
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    times.sort_by(f64::total_cmp);
    let p95 = times[((times.len() - 1) as f64 * 0.95).round() as usize];
    (mean, p95, times[times.len() - 1])
}

fn silent(lumen: &Lumen, track: Id) -> bool {
    lumen.app().meter(track).peak_db < -60.0
}

fn with_face(name: &str, command: &str) -> (Lumen, Id) {
    let (mut lumen, track) = tone(name);
    run_command(&mut lumen, command);
    lumen.wait_for("the card", |app| {
        app.project().tracks[0]
            .devices
            .iter()
            .any(|d| matches!(d.plugin, PluginRef::Builtin { .. }))
    });
    lumen.show_whole_cards();
    lumen.frames(20);
    (lumen, track)
}

#[test]
#[ignore = "a bench: prints frame times with a face at rest and with signal"]
fn the_cost_of_a_frame_with_a_face_at_rest_and_with_signal() {
    for (command, label) in [
        ("Add Horizon", "Horizon"),
        ("Add Gravity", "Gravity"),
        ("Add Afterglow", "Afterglow"),
        ("Add Parallax", "Parallax"),
    ] {
        let (mut lumen, track) = with_face(&format!("face-bench-{label}"), command);
        lumen.run_until("the meters at rest", |l| silent(l, track));
        lumen.frames(60);
        let rest = measure(&mut lumen, 300);
        lumen.key(Modifiers::NONE, Key::Space);
        lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
        lumen.run_until("signal", |l| !silent(l, track));
        let live = measure(&mut lumen, 300);
        println!(
            "FACE {label}: rest mean {:.3} p95 {:.3} worst {:.3} ms; live mean {:.3} p95 {:.3} worst {:.3} ms",
            rest.0, rest.1, rest.2, live.0, live.1, live.2
        );
    }
}
