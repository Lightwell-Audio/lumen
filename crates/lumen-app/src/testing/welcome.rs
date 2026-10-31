//! What the E2E suite reads of the first-run window (M9 R7b).

use crate::App;

impl App {
    /// The step the first-run window shows, while it is open.
    pub fn welcome_step(&self) -> Option<&'static str> {
        self.welcome.open.then(|| self.welcome.step.name())
    }

    /// Whether the settings say setup was finished or closed.
    pub fn setup_done(&self) -> bool {
        self.settings.setup_done
    }

    /// Which side of the sound check sounds now: "", "left", "right" or
    /// "both".
    pub fn sound_check_side(&self) -> &'static str {
        use lumen_engine::chime::Sounding;
        match self.transport.audio.sound_check_sounding().0 {
            Sounding::Silent => "",
            Sounding::Left => "left",
            Sounding::Right => "right",
            Sounding::Both => "both",
        }
    }
}
