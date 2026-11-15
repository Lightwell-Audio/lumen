use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use eframe::egui;
use lumen_project::automation::Target;
use lumen_project::mapping::Mapping;
use lumen_project::model::Id;

use crate::App;
use crate::arrangement::auto_scale as scale;
use crate::live_tap::Tap;

const HOLD: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Last {
    pub channel: u8,
    pub number: u8,
    pub value: u8,
}

#[derive(Default)]
pub(crate) struct Controllers {
    pub(crate) tap: Option<Tap>,
    pub(crate) learning: Option<(Id, Target)>,
    pub(crate) last: Option<Last>,
    pub(crate) window: bool,
    held: BTreeMap<(Id, Target), Instant>,
    taken: Vec<(u8, u8)>,
}

impl Controllers {
    pub(crate) fn holding(&self) -> bool {
        !self.held.is_empty()
    }

    pub(crate) fn forget_project(&mut self) {
        self.learning = None;
        self.last = None;
        self.held.clear();
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.tap.as_ref().map_or(0, Tap::dropped)
    }
}

impl App {
    pub(crate) fn update_controllers(&mut self, ctx: &egui::Context) {
        let mut heard = Vec::new();
        let mut notes = Vec::new();
        if let Some(tap) = &mut self.controllers.tap {
            tap.drain(|h| heard.push(h));
            tap.drain_notes(|n| notes.push(n.key));
        }
        for key in notes {
            self.drum_map_heard(key);
        }
        self.keep_taken_from_engine();
        let mut latest: Vec<(Mapping, u8)> = Vec::new();
        for h in heard {
            self.controllers.last = Some(Last {
                channel: h.channel,
                number: h.number,
                value: h.value,
            });
            if let Some((track, target)) = self.controllers.learning.take() {
                self.map_heard(track, target, h.channel, h.number);
                continue;
            }
            if let Some(m) = self.document.mapping_at(h.channel, h.number) {
                match latest.iter_mut().find(|(l, _)| *l == m) {
                    Some(slot) => slot.1 = h.value,
                    None => latest.push((m, h.value)),
                }
            }
        }
        let now = Instant::now();
        for (m, raw) in latest {
            let value = scale::value(m.target, f32::from(raw) / 127.0);
            self.document.move_control(m.track, m.target, value);
            self.controllers.held.insert((m.track, m.target), now);
            self.gestures_open.insert((m.track, m.target));
        }
        self.release_controllers(ctx, now);
    }

    fn release_controllers(&mut self, ctx: &egui::Context, now: Instant) {
        let quiet: Vec<(Id, Target)> = self
            .controllers
            .held
            .iter()
            .filter(|(_, at)| now.duration_since(**at) > HOLD)
            .map(|(&key, _)| key)
            .collect();
        for key in &quiet {
            self.controllers.held.remove(key);
            self.gestures_open.remove(key);
        }
        if !quiet.is_empty() && !self.controllers.holding() {
            self.document.seal();
        }
        if self.controllers.holding() {
            ctx.request_repaint_after(HOLD);
        }
    }

    fn keep_taken_from_engine(&mut self) {
        let taken: Vec<(u8, u8)> = self
            .document
            .project()
            .mappings
            .iter()
            .map(|m| (m.channel, m.number))
            .collect();
        if taken != self.controllers.taken {
            if let Some(tap) = &self.controllers.tap {
                tap.take(taken.iter().copied());
            }
            self.controllers.taken = taken;
        }
    }

    fn map_heard(&mut self, track: Id, target: Target, channel: u8, number: u8) {
        match self.document.set_mapping(track, target, channel, number) {
            Ok(()) => {
                let what = self.control_name(track, target);
                self.say(format!(
                    "Controller {number} on channel {} moves {what}",
                    channel + 1
                ));
            }
            Err(refusal) => self.say(refusal.reason().to_owned()),
        }
    }

    pub(crate) fn control_name(&self, track: Id, target: Target) -> String {
        let project = self.document.project();
        let owner = lumen_project::routing::all_tracks(project)
            .find(|t| t.id == track)
            .map_or_else(|| "A track that is gone".to_owned(), |t| t.name.clone());
        format!(
            "{owner} \u{00B7} {}",
            self.arrangement.auto.name_of(project, target)
        )
    }

    pub(crate) fn start_learning(&mut self, track: Id, target: Target) {
        self.controllers.learning = Some((track, target));
    }

    pub(crate) fn stop_learning(&mut self) -> bool {
        self.controllers.learning.take().is_some()
    }
}
