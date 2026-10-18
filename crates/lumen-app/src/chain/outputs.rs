//! The outputs row of a plugin instrument's card (M6 slice 6d; the owner's
//! O1): shown only when the running plugin has outputs besides its main one.
//! The word "Outputs" and one chooser that says how many have a track of their
//! own ("Main only", "Main + 2"); its menu lists every output with a switch
//! and the name of the track it makes, with All on and All off. Turning an
//! output on makes an audio track for it, right under the instrument's track;
//! turning it off takes the track away, and when that track holds clips or
//! devices the menu asks whether to remove it or keep it.
//!
//! Every change is `Document::set_outputs`: one undo step.

use eframe::egui::{self, Align2, FontId, Id as EguiId, Rect, Response, Sense, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_project::model::Id;

use crate::document::{Leave, OutputEdit, Refusal, find_device};
use crate::hosting::State;
use crate::theme::Tone;
use crate::widgets::{self, raised};
use crate::{App, motion, theme};

/// What a card's row shows.
pub(super) struct Look {
    /// Outputs besides the main one that have a track of their own.
    pub extra_on: usize,
}

/// The word before the chooser, as wide as the sidechain row's.
const LABEL: f32 = 62.0;
const HEIGHT: f32 = 22.0;
/// The most rows the list shows before it scrolls.
const LIST_HEIGHT: f32 = 224.0;

/// "Main only", "Main + 2".
fn summary(extra_on: usize) -> String {
    match extra_on {
        0 => "Main only".to_owned(),
        n => format!("Main + {n}"),
    }
}

/// The chooser's id for the card of `device` (the command opens its menu).
pub(super) fn chooser_id(device: Id) -> EguiId {
    chooser_id_of(EguiId::new(("device", device.0)))
}

pub(super) fn chooser_id_of(card: EguiId) -> EguiId {
    card.with("outputs")
}

/// The row in `area`, which is `KEY_ROOM` tall; answers the chooser.
pub(super) fn paint(ui: &mut Ui, area: Rect, id: EguiId, look: &Look) -> Response {
    let palette = *theme::p();
    let pixels = ui.pixels_per_point();
    let row =
        Rect::from_center_size(area.center(), vec2(area.width(), HEIGHT)).round_to_pixels(pixels);
    let chooser = Rect::from_min_max(pos2(row.left() + LABEL, row.top()), row.max);
    let caption = FontId::proportional(theme::size::CAPTION);
    ui.painter().text(
        pos2(row.left() + 2.0, row.center().y),
        Align2::LEFT_CENTER,
        "Outputs",
        caption.clone(),
        palette.text_secondary,
    );
    let response = ui.interact(chooser, id, Sense::click());
    let words = summary(look.extra_on);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::ComboBox,
            true,
            format!("Outputs, {words}"),
        )
    });
    let hover = motion::hover(ui.ctx(), id.with("hover"), response.hovered());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let face = raised::Face::new(theme::controls(false))
        .hover(hover)
        .pressed(response.is_pointer_button_down_on())
        .lit(motion::hover(ui.ctx(), id.with("open"), open), Tone::On);
    let body = raised::paint(
        ui.painter(),
        chooser,
        theme::radius::for_height(chooser.height()),
        &face,
    );
    let painter = ui.painter();
    let ink = if look.extra_on == 0 {
        palette.text_secondary
    } else {
        palette
            .text
            .lerp_to_gamma(theme::lit(Tone::On).text, face.lit)
    };
    painter.text(
        pos2(chooser.left() + 7.0, body.center().y),
        Align2::LEFT_CENTER,
        widgets::fit(painter, &words, caption.clone(), chooser.width() - 28.0),
        caption,
        ink,
    );
    widgets::draw_arrow(
        painter,
        pos2(chooser.right() - 8.0, body.center().y),
        1.0,
        palette.text_hint,
    );
    raised::focus_ring(painter, &response, chooser);
    response.on_hover_text("Which of the instrument's outputs have a track of their own")
}

/// What each track that plays a plugin's output says about it: where the
/// sound comes from, and that it plays nothing while the instrument is not
/// running.
pub(crate) fn notes_of(app: &App) -> std::collections::HashMap<Id, crate::arrangement::OutputNote> {
    use crate::arrangement::OutputNote;
    let project = app.document.project();
    project
        .tracks
        .iter()
        .filter_map(|track| {
            let output = track.plugin_output?;
            let note = |text: String, warn: bool| Some((track.id, OutputNote { text, warn }));
            let Some((_, device)) = find_device(project, output.device) else {
                return note(
                    "its instrument is gone: this track plays nothing".to_owned(),
                    true,
                );
            };
            let instrument = crate::document::device_name(device);
            let which = app
                .hosting
                .output_names(output.device)
                .get(output.bus as usize)
                .cloned()
                .unwrap_or_else(|| format!("output {}", output.bus + 1));
            match app.hosting.state(output.device) {
                Some(State::Ready) => note(format!("plays {which} of {instrument}"), false),
                Some(State::Loading) | None => note(
                    format!("{instrument} is starting: this track plays nothing yet"),
                    true,
                ),
                Some(State::Failed(_)) => note(
                    format!("{instrument} stopped: this track plays nothing until it is reloaded"),
                    true,
                ),
                Some(State::Missing) => note(
                    format!("{instrument} is not installed: this track plays nothing"),
                    true,
                ),
                Some(State::Safe) => note(
                    format!("{instrument} is not loaded in safe mode: this track plays nothing"),
                    true,
                ),
            }
        })
        .collect()
}

/// Outputs asked to be turned off whose tracks hold work: waiting for the
/// answer in the menu.
#[derive(Default)]
pub(crate) struct Asking(Option<(Id, Vec<u32>)>);

impl App {
    /// The row of `device`'s card: none for a card that is not a running plugin
    /// instrument with outputs besides its main one.
    pub(super) fn outputs_look(&self, device: &lumen_project::model::Device) -> Option<Look> {
        if !device.is_vst3()
            || !device.is_instrument()
            || !matches!(self.hosting.state(device.id), Some(State::Ready))
        {
            return None;
        }
        let extra = self.hosting.extra_outputs(device.id);
        if extra.is_empty() {
            return None;
        }
        let on = self.document.output_tracks(device.id);
        Some(Look {
            extra_on: extra
                .iter()
                .filter(|(bus, _)| on.iter().any(|(b, _)| b == bus))
                .count(),
        })
    }

    /// The menu under the chooser of `device`.
    pub(super) fn outputs_menu(&mut self, chooser: &Response, device: Id) {
        widgets::Menu::below(chooser)
            .keeping_open()
            .show(|ui| self.outputs_menu_body(ui, device));
    }

    /// Opens the chosen card's outputs menu (the command).
    pub(crate) fn open_outputs_menu(&mut self, ctx: &egui::Context) {
        if let Some(device) = self.chain.selected {
            widgets::Menu::open_for(ctx, chooser_id(device));
        }
    }

    /// Whether the chosen card has an outputs row.
    pub(crate) fn chosen_has_outputs(&self) -> bool {
        self.chain.selected.is_some_and(|d| {
            find_device(self.document.project(), d)
                .is_some_and(|(_, dev)| self.outputs_look(dev).is_some())
        })
    }

    fn outputs_menu_body(&mut self, ui: &mut Ui, device: Id) {
        let palette = *theme::p();
        ui.set_min_width(250.0);
        let Some((_, held)) = find_device(self.document.project(), device) else {
            return;
        };
        let instrument = crate::document::device_name(held);
        let extra = self.hosting.extra_outputs(device);
        let on = self.document.output_tracks(device);
        let track_of = |bus: u32| on.iter().find(|(b, _)| *b == bus).map(|(_, t)| *t);
        let track_name = |track: Id| {
            self.document
                .project()
                .tracks
                .iter()
                .find(|t| t.id == track)
                .map_or_else(String::new, |t| t.name.clone())
        };
        let on_buses: Vec<u32> = on.iter().map(|(bus, _)| *bus).collect();
        let mut crowded = false;
        let mut turn_on: Vec<(u32, String)> = Vec::new();
        let mut turn_off: Vec<u32> = Vec::new();

        widgets::menu_caption(ui, &format!("Outputs of {instrument}"));
        // The main output is the instrument's own track: always on.
        let mut main = true;
        ui.add_enabled_ui(false, |ui| {
            widgets::switch(ui, &mut main, "Main", false);
        })
        .response
        .on_disabled_hover_text("The instrument's own track plays its main output");
        // A solid bar takes room of its own (when the list scrolls), so it
        // does not lie over the switches.
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        egui::ScrollArea::vertical()
            .max_height(LIST_HEIGHT)
            .auto_shrink([true, true])
            .show(ui, |ui| {
                for (bus, name) in &extra {
                    let track = track_of(*bus);
                    let mut is_on = track.is_some();
                    // A block carries only so many channels: past that an
                    // output cannot be turned on until another is off.
                    let fits = is_on || self.hosting.output_fits(device, &on_buses, *bus);
                    crowded |= !fits;
                    // One widget per row: the track's name is painted inside
                    // it, so nothing laid out after the switch can widen the
                    // menu (a row beside a full-width switch grew it on
                    // every frame).
                    // The name it was given when made says nothing new: only a
                    // track that was renamed is named here.
                    let made = format!("{instrument} \u{b7} {name}");
                    let note = track
                        .map(track_name)
                        .filter(|named| *named != made)
                        .unwrap_or_default();
                    let row = widgets::switch_noted(ui, &mut is_on, name, fits, &note);
                    if !fits {
                        row.on_hover_text(
                            "Not room for it: a block carries 64 channels in all, the main pair included. Turn another output off first.",
                        );
                    } else if row.clicked() {
                        if is_on {
                            turn_on.push((*bus, format!("{instrument} \u{b7} {name}")));
                        } else {
                            turn_off.push(*bus);
                        }
                    }
                }
            });
        if crowded {
            widgets::menu_caption(ui, "64 channels at most, the main pair included");
        }
        ui.separator();
        ui.horizontal(|ui| {
            if raised::button(ui, "All on")
                .on_hover_text("Every output of the plugin (as many as fit in a block)")
                .clicked()
            {
                let mut now = on_buses.clone();
                turn_on.clear();
                for (bus, name) in &extra {
                    if track_of(*bus).is_none() && self.hosting.output_fits(device, &now, *bus) {
                        now.push(*bus);
                        turn_on.push((*bus, format!("{instrument} \u{b7} {name}")));
                    }
                }
            }
            if raised::button(ui, "All off").clicked() {
                turn_off = on.iter().map(|(bus, _)| *bus).collect();
            }
        });

        // A turned-off output whose track holds work waits for an answer.
        let waiting = self
            .chain
            .asking
            .0
            .as_ref()
            .filter(|(d, _)| *d == device)
            .map(|(_, buses)| buses.clone());
        let mut answer: Option<Option<Leave>> = None;
        if let Some(buses) = &waiting {
            let names: Vec<String> = buses
                .iter()
                .filter_map(|b| extra.iter().find(|(x, _)| x == b).map(|(_, n)| n.clone()))
                .collect();
            ui.separator();
            ui.label(
                egui::RichText::new(format!(
                    "{} has a track with clips or devices. Remove the track, or keep it with no input?",
                    names.join(", ")
                ))
                .small()
                .color(palette.warning),
            );
            ui.horizontal(|ui| {
                if raised::primary_button(ui, "Remove track").clicked() {
                    answer = Some(Some(Leave::Remove));
                }
                if raised::button(ui, "Keep track").clicked() {
                    answer = Some(Some(Leave::Keep));
                }
                if raised::button(ui, "Cancel").clicked() {
                    answer = Some(None);
                }
            });
        }

        if let Some(answer) = answer {
            let buses = waiting.unwrap_or_default();
            self.chain.asking = Asking::default();
            if let Some(leave) = answer {
                self.apply_outputs(device, &[], &buses, Some(leave));
            }
        } else if !turn_on.is_empty() || !turn_off.is_empty() {
            let in_use = self.document.outputs_in_use(device, &turn_off);
            if in_use.is_empty() {
                self.chain.asking = Asking::default();
                self.apply_outputs(device, &turn_on, &turn_off, None);
            } else {
                // The ones with nothing in them can go at once; the others wait.
                let idle: Vec<u32> = turn_off
                    .iter()
                    .copied()
                    .filter(|b| !in_use.contains(b))
                    .collect();
                self.apply_outputs(device, &turn_on, &idle, None);
                self.chain.asking = Asking(Some((device, in_use)));
            }
        }
    }

    /// One change of outputs, one undo step; a refusal is said.
    fn apply_outputs(
        &mut self,
        device: Id,
        on: &[(u32, String)],
        off: &[u32],
        leave: Option<Leave>,
    ) {
        if on.is_empty() && off.is_empty() {
            return;
        }
        match self
            .document
            .set_outputs(device, OutputEdit { on, off, leave })
        {
            Ok(_) => {}
            Err(Refusal::OutputHasContent) => {
                self.chain.asking = Asking(Some((device, off.to_vec())));
            }
            Err(why) => self.say(why.to_string()),
        }
    }
}
