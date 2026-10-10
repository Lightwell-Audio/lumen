//! The light layer (ADR-0043): the transport's mark is a meter of the
//! master, its sun rising while sound plays and settling when it stops.

use std::time::Duration;

use eframe::egui::{Key, Modifiers};
use lumen_core::time::Ticks;

use super::{Lumen, wav};

/// Lets audio play for `ms`, drawing frames as the app would.
fn listen(lumen: &mut Lumen, ms: u64) {
    for _ in 0..ms / 50 {
        std::thread::sleep(Duration::from_millis(50));
        lumen.frames(1);
    }
}

#[test]
fn the_mark_rises_with_the_master_and_settles_when_stopped() {
    let mut lumen = Lumen::start("light-mark");
    // Silent and stopped, the sun only peeks over the horizon.
    lumen.frames(3);
    assert_eq!(lumen.app().mark(), (0.0, false));

    let file = wav(&lumen.dir().join("tone.wav"), 20.0);
    lumen.drop_files(&[file]);
    lumen.frames(5);
    lumen.key(Modifiers::NONE, Key::Space);
    lumen.wait_for("playback", |app| app.playhead() > Ticks::quarters(1));
    listen(&mut lumen, 400);
    // The quiet tone (about −21 dBFS) lifts it a good way, not all the way.
    let (playing, recording) = lumen.app().mark();
    assert!((0.3..0.9).contains(&playing), "{playing}");
    assert!(!recording);

    // Stopped, the master's level falls and the sun with it.
    lumen.key(Modifiers::NONE, Key::Space);
    listen(&mut lumen, 3_500);
    let (stopped, _) = lumen.app().mark();
    assert!(stopped < playing - 0.1, "{stopped} after {playing}");
}
