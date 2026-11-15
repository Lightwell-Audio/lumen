use eframe::egui;

use crate::App;
use crate::arrangement::audio_in::InputChoices;
use crate::recording::audio_input;

pub(crate) const WATCH_EVERY: std::time::Duration = crate::motion::METER_FRAME;

#[derive(Debug, Default)]
pub(crate) struct InputSync {
    sent: Option<(u64, Vec<usize>)>,
    meters_at: Option<f64>,
}

impl App {
    pub(crate) fn update_inputs(&mut self, ctx: &egui::Context) -> bool {
        let open = self.transport.audio.open_inputs();
        self.document.set_open_inputs(open.len() as u16);
        let key = (self.document.state(), open);
        if self.inputs.sent.as_ref() != Some(&key) {
            let routes = audio_input::routes(self.document.project(), &key.1);
            if self.transport.audio.inputs.set_routes(&routes) {
                self.inputs.sent = Some(key);
            }
        }
        self.transport.audio.inputs.collect_garbage();

        let open = self.transport.audio.open_inputs();
        self.lend_input_choices(ctx, &open);
        let dt = crate::app::mixer::meter_step(&mut self.inputs.meters_at, ctx);
        let running = matches!(
            self.transport.audio.status,
            crate::audio::Status::Running { .. }
        );
        self.arrangement
            .meters
            .update_inputs(&self.transport.audio.inputs, &open, dt, running);
        running && self.watching_inputs(&open)
    }

    fn lend_input_choices(&mut self, ctx: &egui::Context, open: &[usize]) {
        let wanted = std::mem::take(&mut self.arrangement.inputs.wanted);
        let mut choices = InputChoices {
            open: open.to_vec(),
            names: std::mem::take(&mut self.arrangement.inputs.names),
            ..InputChoices::default()
        };
        let key = self
            .transport
            .audio
            .device_key()
            .map(|(driver, id)| (driver.to_owned(), id.to_owned()));
        if let (Some((driver, id)), true) = (key, wanted || !choices.names.is_empty()) {
            match self.preferences.audio.input_names(&driver, &id, ctx) {
                Some(Ok(names)) => choices.names = names,
                Some(Err(why)) => choices.status = Some(why),
                None => choices.status = Some("Looking up the inputs{2026}".into()),
            }
        }
        self.arrangement.inputs = choices;
    }

    fn watching_inputs(&self, open: &[usize]) -> bool {
        self.document.project().tracks.iter().any(|t| {
            t.input
                .is_some_and(|input| audio_input::place(input, open).is_some())
                && (t.armed || t.monitor == lumen_project::model::Monitor::In)
        })
    }
}
