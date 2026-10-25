//! The audio inputs' live side (M9 slice R1, ADR-0100): every frame, the
//! tracks' input routes reach the engine when the project or the device
//! changed, the input meters move on, and the project learns how many inputs
//! the device has open (so arming a track can pick a free one).

use std::time::Instant;

use eframe::egui;

use crate::App;
use crate::arrangement::audio_in::InputChoices;
use crate::audio_input;

/// How often the input meters are redrawn while an input is being watched
/// and nothing else is moving.
pub(crate) const WATCH_EVERY: std::time::Duration = std::time::Duration::from_millis(33);

/// What the engine was last told, so it is told again only on change.
#[derive(Debug, Default)]
pub(crate) struct InputSync {
    /// The project state and open inputs the routes were built from.
    sent: Option<(u64, Vec<usize>)>,
    /// When the input meters were last moved on.
    meters_at: Option<Instant>,
}

impl App {
    /// Sends the engine the tracks' input routes when they may have changed
    /// (a send the queue refused is tried again next frame), and moves the
    /// input meters on. Returns whether an input is being watched, so the
    /// frame loop keeps the meters drawn while nothing else moves.
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
        let now = Instant::now();
        let dt = self
            .inputs
            .meters_at
            .map_or(0.0, |at| now.duration_since(at).as_secs_f32());
        self.inputs.meters_at = Some(now);
        let running = matches!(
            self.transport.audio.status,
            crate::audio::Status::Running { .. }
        );
        self.arrangement
            .meters
            .update_inputs(&self.transport.audio.inputs, &open, dt, running);
        running && self.watching_inputs(&open)
    }

    /// Lends the arrangement the device's inputs for its menus. The channel
    /// names are looked up only once a menu asks for them.
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
            match self
                .transport
                .settings_window
                .input_names(&driver, &id, ctx)
            {
                Some(Ok(names)) => choices.names = names,
                Some(Err(why)) => choices.status = Some(why),
                None => choices.status = Some("Looking up the inputs{2026}".into()),
            }
        }
        self.arrangement.inputs = choices;
    }

    /// Whether an armed or monitored audio track has an input the device
    /// has open: its meter is worth drawing even when nothing else moves.
    fn watching_inputs(&self, open: &[usize]) -> bool {
        self.document.project().tracks.iter().any(|t| {
            t.input
                .is_some_and(|input| audio_input::place(input, open).is_some())
                && (t.armed || t.monitor == lumen_project::model::Monitor::In)
        })
    }
}
