use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;
use lumen_project::model::{Id, PluginRef};

use super::Lumen;
use super::lane_view::{run_command, tone};
use lumen_app::testing::{face_frame, face_repaint_ask, forget_face_repaint_asks};

fn with_horizon(name: &str) -> (Lumen, Id) {
    let (mut lumen, track) = tone(name);
    run_command(&mut lumen, "Add Horizon");
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

fn asked_in_one_frame(lumen: &mut Lumen) -> Option<Duration> {
    let ctx = lumen.harness.ctx.clone();
    forget_face_repaint_asks(&ctx);
    lumen.frames(1);
    face_repaint_ask(&ctx)
}

fn silent(lumen: &Lumen, track: Id) -> bool {
    lumen.app().meter(track).peak_db < -60.0
}

#[test]
fn a_face_at_rest_asks_for_no_frame_and_a_face_with_signal_asks_for_the_face_frame() {
    let (mut lumen, track) = with_horizon("face-cadence");
    lumen.run_until("the meters at rest", |l| silent(l, track));
    lumen.frames(60);
    for frame in 0..5 {
        assert_eq!(
            asked_in_one_frame(&mut lumen),
            None,
            "a stopped face asks for nothing, frame {frame}"
        );
    }

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    lumen.run_until("signal on the track", |l| !silent(l, track));
    for frame in 0..5 {
        assert_eq!(
            asked_in_one_frame(&mut lumen),
            Some(face_frame()),
            "a live face asks for the face frame, frame {frame}"
        );
    }
    assert_eq!(face_frame(), Duration::from_millis(16));

    lumen.key(Modifiers::NONE, Key::Space);
    lumen.run_until("the meters at rest", |l| silent(l, track));
    let ctx = lumen.harness.ctx.clone();
    lumen.run_until("the face to go to sleep", |_| {
        let asked = face_repaint_ask(&ctx);
        forget_face_repaint_asks(&ctx);
        asked.is_none()
    });
    for frame in 0..5 {
        assert_eq!(
            asked_in_one_frame(&mut lumen),
            None,
            "a face that has gone to sleep stays quiet, frame {frame}"
        );
    }
}
