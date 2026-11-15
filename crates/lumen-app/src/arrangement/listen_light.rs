use eframe::egui::{Id, Rect, Ui, pos2};
use lumen_engine::{INPUT_SLOTS, KEYBOARD};
use lumen_project::model::{Track, TrackKind};

use crate::midi::play::{Inputs, from_sources, listening};
use crate::{light, theme};

const RADIUS: f32 = 2.5;
const GAP: f32 = 4.0;
pub(crate) const ROOM: f32 = GAP + 2.0 * RADIUS;

#[derive(Debug, Clone, Default)]
pub(crate) struct Live {
    pub inputs: Inputs,
    pub counts: Vec<(u16, u32, bool)>,
    pub keyboard: u32,
    pub keyboard_held: bool,
}

impl Live {
    fn traffic(&self, sources: u64) -> (u32, bool) {
        let mut total = 0_u32;
        let mut held = false;
        for &(slot, count, down) in &self.counts {
            if slot < u16::from(INPUT_SLOTS) && sources & (1 << slot) != 0 {
                total = total.wrapping_add(count);
                held |= down;
            }
        }
        if sources & (1 << KEYBOARD) != 0 {
            total = total.wrapping_add(self.keyboard);
            held |= self.keyboard_held;
        }
        (total, held)
    }

    pub(crate) fn listens(&self, track: &Track, heard: bool) -> bool {
        track.kind == TrackKind::Midi
            && listening(track, &self.inputs, track.midi_to.is_some() && !heard).is_on()
    }

    pub(crate) fn paint(&self, ui: &Ui, track: &Track, heard: bool, drawn: Rect) {
        if track.kind != TrackKind::Midi {
            return;
        }
        let (count, held) = self.traffic(from_sources(track, &self.inputs));
        let brightness =
            light::flash_held(ui.ctx(), Id::new(("listen light", track.id)), count, held);
        let shown = if self.listens(track, heard) {
            brightness
        } else {
            0.0
        };
        ui.ctx()
            .data_mut(|d| d.insert_temp(shown_id(track.id), shown));
        if shown <= 0.0 {
            return;
        }
        let x = drawn.right() + GAP + RADIUS;
        light::dot(
            ui.painter(),
            light::BEAM,
            pos2(x, drawn.center().y),
            RADIUS,
            theme::p().accent,
            shown,
        );
    }
}

fn shown_id(track: lumen_project::model::Id) -> Id {
    Id::new(("listen light shown", track))
}

pub(crate) fn shown(ctx: &eframe::egui::Context, track: lumen_project::model::Id) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(shown_id(track)))
        .unwrap_or(0.0)
}
