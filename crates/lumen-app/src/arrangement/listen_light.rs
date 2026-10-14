//! The light after a MIDI track's name (M5 slice 2e, the owner's H B): it
//! flashes at the BEAM level when live input reaches the track, so a
//! producer sees which track hears the keys. It exists only while the track
//! listens (monitor In, or Auto while selected or armed) and is unlit
//! between flashes: it is drawn just past the name's last letter. A track
//! that listens keeps [`ROOM`] for it at the end of its name, lit or not,
//! so a long name is shortened to the same place all the while it listens
//! and never shifts with a flash (it shortens by a letter when the track
//! starts to listen, which is when its ring appears too).
//!
//! The flash follows the traffic of the sources the track's MIDI From takes
//! (input slots and the computer keyboard), not its channel filter: a
//! message on another channel still flashes it. That is a light for "your
//! keys are arriving here", and the channel is one more thing to hold in
//! mind; the sound is right either way.

use eframe::egui::{Id, Rect, Ui, pos2};
use lumen_engine::{INPUT_SLOTS, KEYBOARD};
use lumen_project::model::{Track, TrackKind};

use crate::midi_play::{Inputs, from_sources, listening};
use crate::{light, theme};

/// The light's radius (the chip's lights use the same).
const RADIUS: f32 = 2.5;
/// Space between the name's last letter and the light's edge.
const GAP: f32 = 4.0;
/// What a listening track's name gives up at its end for the light.
pub(crate) const ROOM: f32 = GAP + 2.0 * RADIUS;

/// What live input there is, lent by the app each frame.
#[derive(Debug, Clone, Default)]
pub(crate) struct Live {
    /// Which tracks are selected and which inputs are on for tracks.
    pub inputs: Inputs,
    /// Each present input's slot and its traffic count.
    pub counts: Vec<(u16, u32)>,
    /// The computer keyboard's traffic count.
    pub keyboard: u32,
}

impl Live {
    /// The traffic of the sources `sources` names, as one number: it
    /// changes when any of them has a message.
    fn traffic(&self, sources: u64) -> u32 {
        let mut total = 0_u32;
        for &(slot, count) in &self.counts {
            if slot < u16::from(INPUT_SLOTS) && sources & (1 << slot) != 0 {
                total = total.wrapping_add(count);
            }
        }
        if sources & (1 << KEYBOARD) != 0 {
            total = total.wrapping_add(self.keyboard);
        }
        total
    }

    /// Whether `track` listens now (`heard`: its output is not silenced).
    pub(crate) fn listens(&self, track: &Track, heard: bool) -> bool {
        track.kind == TrackKind::Midi
            && listening(track, &self.inputs, track.midi_to.is_some() && !heard).is_on()
    }

    /// Draws the light after the name just drawn (`drawn`). Always keeps the flash memory current, so a track
    /// that starts to listen does not flash for old traffic.
    pub(crate) fn paint(&self, ui: &Ui, track: &Track, heard: bool, drawn: Rect) {
        if track.kind != TrackKind::Midi {
            return;
        }
        let count = self.traffic(from_sources(track, &self.inputs));
        let brightness = light::flash(ui.ctx(), Id::new(("listen light", track.id)), count);
        let shown = if self.listens(track, heard) {
            brightness
        } else {
            0.0
        };
        // What was drawn, for tests (the accessibility tree has no light).
        ui.ctx()
            .data_mut(|d| d.insert_temp(shown_id(track.id), shown));
        if shown <= 0.0 {
            return;
        }
        // Past the last letter.
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

/// How bright `track`'s listening light was when last drawn.
pub(crate) fn shown(ctx: &eframe::egui::Context, track: lumen_project::model::Id) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(shown_id(track)))
        .unwrap_or(0.0)
}
