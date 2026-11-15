use crate::App;

impl App {
    pub fn welcome_step(&self) -> Option<&'static str> {
        self.welcome.open.then(|| self.welcome.step.name())
    }

    pub fn setup_done(&self) -> bool {
        self.settings.setup_done
    }

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
