use std::time::Instant;

use eframe::egui;

use lumen_project::routing as rules;

use crate::App;
use crate::arrangement::OutputChoices;

impl App {
    pub(crate) fn update_meters(&mut self, ctx: &egui::Context) -> bool {
        let dt = meter_step(&mut self.meters_at, ctx);
        let project = self.document.project();
        let ids: Vec<u64> = rules::all_tracks(project).map(|t| t.id.0).collect();
        if std::mem::take(&mut self.transport.played_from_stop) {
            self.arrangement.meters.reset_loudness();
        }
        let running = matches!(
            self.transport.audio.status,
            crate::audio::Status::Running { .. }
        );
        let meters = &mut self.arrangement.meters;
        meters.update(&mut self.transport.audio.mixer, &ids, dt, running);
        self.arrangement.track_colour_meters = self.settings.mixing.track_colour_meters;
        self.arrangement.meters.moving()
    }

    pub(crate) fn take_budget(&mut self) {
        if self.callback_bench.is_some() {
            return;
        }
        let now = Instant::now();
        if !self.transport.budget.due(now) {
            return;
        }
        let Some(meters) = self.transport.audio.meters().cloned() else {
            return;
        };
        let time = meters.take_block_time();
        let costs = self.hosting.take_costs(self.document.project());
        self.transport.budget.update(now, time, &costs);
    }

    pub(crate) fn update_outputs(&mut self, ctx: &egui::Context) {
        let pairs = crate::routing::device_pairs(self.document.project());
        let wanted = self.arrangement.outputs.wanted || !pairs.is_empty();
        let key = self
            .transport
            .audio
            .device_key()
            .map(|(driver, id)| (driver.to_owned(), id.to_owned()));
        let names = match (&key, wanted) {
            (Some((driver, id)), true) => self.preferences.audio.output_names(driver, id, ctx),
            _ => None,
        };
        let layout = self.transport.audio.outputs();
        let mut choices = OutputChoices {
            main: layout.main,
            cue: layout.cue,
            wanted: false,
            ..OutputChoices::default()
        };
        match names {
            Some(Ok(names)) => {
                self.transport.audio.want_pairs(&pairs, names.len());
                choices.pairs = names
                    .chunks(2)
                    .enumerate()
                    .map(|(p, chunk)| (p as u16, chunk.join(" \u{00B7} ")))
                    .collect();
            }
            Some(Err(why)) => choices.status = Some(why),
            None if wanted => choices.status = Some("Looking up the outputs\u{2026}".into()),
            None => {}
        }
        self.arrangement.outputs = choices;
    }
}

pub(crate) fn meter_step(at: &mut Option<f64>, ctx: &egui::Context) -> f32 {
    let now = ctx.input(|i| i.time);
    let dt = at.map_or(0.0, |at| (now - at) as f32);
    *at = Some(now);
    dt
}
