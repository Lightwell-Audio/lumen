//! The Devices tab: the shown track's chain in a sunk tray (the owner's C4).
//! Cables join the chips at either end and every card; they carry a Beam,
//! the playhead's light, while the song plays, and a dotted line carries a
//! MIDI track's notes into its instrument. `rows` reads what each card shows,
//! `panel` is the tab, `menu` its cards' menus and `paint` the tray's drawings.

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rangef, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};
use lumen_core::product::PRODUCT_NAME;
use lumen_project::model::{Device, DeviceKind, Id, Output, PluginRef, Project, Track, TrackKind};
use lumen_project::routing::all_tracks;

use super::card::{self, Action, Face, Failure, Field, Look};
use super::geometry::{CHIP_H, Layout, PORT};
use super::place::Place;
use super::strip;
use super::{CardDrag, Dragged, actions};
use crate::app::panes::{self, Pane};
use crate::arrangement::color_of;
use crate::commands::Command;
use crate::document::Slot;
use crate::hosting::State;
use crate::widgets::raised;
use crate::{App, light, motion, theme, widgets};

impl App {
    /// Once a frame, before the keys: the chosen card is still on the shown
    /// track, and Delete means "remove it" while the tab has the keyboard.
    pub(crate) fn refresh_chain(&mut self, ctx: &egui::Context) {
        // A card let go on a track's header last frame: follow it there, or
        // say why it was refused.
        if let Some(drop) = self.arrangement.device_drop.take() {
            match drop.result {
                Ok(device) => {
                    self.arrangement.selected = vec![drop.to];
                    self.chain.selected = Some(device);
                }
                Err(refusal) => self.say(refusal.to_string()),
            }
        }
        // A device let go on a track from the browser.
        if let Some((device, track)) = self.arrangement.browser_drop.take() {
            self.add_dropped_device(&device, track);
        }
        self.open_dropped_editor();
        // The subject follows a selection made above (or by a command since
        // last frame) before the chosen card is checked against it.
        self.refresh_detail(ctx);
        let shown = self.chain_track();
        let project = self.document.project();
        let still_there = |chosen: Id| {
            shown.is_some_and(|track| {
                all_tracks(project)
                    .any(|t| t.id == track && t.devices.iter().any(|d| d.id == chosen))
            })
        };
        if self.chain.selected.is_some_and(|c| !still_there(c)) {
            self.chain.selected = None;
        }
        let docked = self.show_detail
            && self.detail_tab == crate::app::detail::DetailTab::Devices
            && (self.chain.hovered || panes::focused(ctx) == Some(Pane::Detail));
        self.chain.live = docked && self.chain.selected.is_some();
        self.chain.hovered = false;
        // The analysers are read by the Devices tab alone (M8 slice B1): with
        // the tab hidden or on another tab, nothing is looked at, so nothing
        // runs.
        if !self.show_detail || self.detail_tab != crate::app::detail::DetailTab::Devices {
            self.silence_spectra();
        }
    }
}

pub(crate) use paint::paint_drag_ghost;

mod menu;
mod paint;
mod panel;
mod rows;

#[cfg(test)]
mod tests;
