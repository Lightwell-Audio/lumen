use std::cell::RefCell;
use std::collections::HashMap;

use eframe::egui::{
    self, Align2, Color32, FontId, Id as EguiId, Rect, Response, Sense, Ui, pos2, vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{Device, Id, PluginRef, Sidechain, SidechainTap, TrackKind};

use crate::arrangement::color_of;
use crate::document::find_device;
use crate::hosting::State;
use crate::theme::Tone;
use crate::widgets::{self, raised};
use crate::{App, light, motion, theme};

pub(super) const ROOM: f32 = super::geometry::KEY_ROOM;
const HEIGHT: f32 = 22.0;
const LABEL: f32 = 62.0;
const QUIET_AFTER: f32 = 2.0;
const SILENT: f32 = 0.003;
const FALL: f32 = 9.0;

pub(super) enum Source {
    None,
    Track {
        name: String,
        colour: Color32,
    },
    Gone,
}

pub(super) struct Look {
    pub source: Source,
    pub tap: SidechainTap,
    pub level: f32,
    pub quiet: bool,
    pub playing: bool,
}

#[derive(Default, Clone, Copy)]
pub(crate) struct Reading {
    level: f32,
    silent: f32,
    frozen: bool,
}

#[derive(Default)]
pub(crate) struct Readings(RefCell<HashMap<Id, Reading>>);

pub(super) fn tap_name(tap: SidechainTap) -> &'static str {
    match tap {
        SidechainTap::PreFx => "Pre FX",
        SidechainTap::PostFx => "Post FX",
        SidechainTap::PostMixer => "Post mixer",
    }
}

pub(super) fn tap_short(tap: SidechainTap) -> &'static str {
    match tap {
        SidechainTap::PreFx => "pre",
        SidechainTap::PostFx => "post",
        SidechainTap::PostMixer => "mixer",
    }
}

fn tap_about(tap: SidechainTap) -> &'static str {
    match tap {
        SidechainTap::PreFx => "Before the track's devices. Mute and fader do not change it.",
        SidechainTap::PostFx => {
            "After the track's devices, before mute and fader. The usual choice."
        }
        SidechainTap::PostMixer => "What the track plays: after mute, fader and pan.",
    }
}

pub(super) fn chooser_id(device: Id) -> EguiId {
    chooser_id_of(EguiId::new(("device", device.0)))
}

pub(super) fn chooser_id_of(card: EguiId) -> EguiId {
    card.with("sidechain")
}

pub(super) fn paint(
    ui: &mut Ui,
    area: Rect,
    id: EguiId,
    look: &Look,
    footer: Option<&str>,
) -> Response {
    let palette = *theme::p();
    let pixels = ui.pixels_per_point();
    let height = if footer.is_some() {
        theme::size::CONTROL_SMALL
    } else {
        HEIGHT
    };
    let row =
        Rect::from_center_size(area.center(), vec2(area.width(), height)).round_to_pixels(pixels);
    let caption = FontId::proportional(theme::size::CAPTION);
    let chooser = if footer.is_some() {
        row
    } else {
        ui.painter().text(
            pos2(row.left() + 2.0, row.center().y),
            Align2::LEFT_CENTER,
            "Sidechain",
            caption.clone(),
            palette.text_secondary,
        );
        Rect::from_min_max(pos2(row.left() + LABEL, row.top()), row.max)
    };

    let response = ui.interact(chooser, id, Sense::click());
    let spoken = spoken(look);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, &spoken));
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
    let lit_text = palette
        .text
        .lerp_to_gamma(theme::lit(Tone::On).text, face.lit);
    let arrow_x = chooser.right() - 8.0;
    let lamp = pos2(arrow_x - 14.0, body.center().y);
    let mut left = chooser.left() + 7.0;
    let mut right = arrow_x - 8.0;
    match &look.source {
        Source::None => {
            painter.text(
                pos2(left, body.center().y),
                Align2::LEFT_CENTER,
                "None",
                caption.clone(),
                palette.text_secondary,
            );
        }
        Source::Gone => {
            painter.text(
                pos2(left, body.center().y),
                Align2::LEFT_CENTER,
                widgets::fit(
                    painter,
                    "\u{25B2} Source removed",
                    caption.clone(),
                    right - left,
                ),
                caption.clone(),
                palette.warning,
            );
        }
        Source::Track { name, colour } => {
            let swatch = Rect::from_center_size(pos2(left + 4.0, body.center().y), vec2(8.0, 8.0))
                .round_to_pixels(pixels);
            painter.rect_filled(swatch, 2.0, *colour);
            left = swatch.right() + 6.0;
            right = lamp.x - 9.0;
            let tap = format!(" \u{00B7} {}", tap_name(look.tap));
            let name_w = painter
                .layout_no_wrap(name.clone(), caption.clone(), palette.text)
                .size()
                .x;
            let tap_w = painter
                .layout_no_wrap(tap.clone(), caption.clone(), palette.text_hint)
                .size()
                .x;
            if name_w + tap_w <= right - left {
                painter.text(
                    pos2(left, body.center().y),
                    Align2::LEFT_CENTER,
                    name,
                    caption.clone(),
                    lit_text,
                );
                painter.text(
                    pos2(left + name_w, body.center().y),
                    Align2::LEFT_CENTER,
                    tap,
                    caption.clone(),
                    palette.text_hint,
                );
            } else {
                painter.text(
                    pos2(left, body.center().y),
                    Align2::LEFT_CENTER,
                    widgets::fit(painter, name, caption.clone(), right - left),
                    caption.clone(),
                    lit_text,
                );
            }
            let ring = if look.quiet {
                palette.warning
            } else {
                palette.border_strong
            };
            painter.circle(
                lamp,
                super::geometry::PORT,
                if look.quiet {
                    Color32::TRANSPARENT
                } else {
                    palette.well
                },
                egui::Stroke::new(theme::stroke::LIT, ring),
            );
            if !look.quiet && look.level > 0.02 {
                light::dot(
                    painter,
                    light::BEAM,
                    lamp,
                    super::geometry::PORT - 1.0,
                    palette.accent,
                    look.level.clamp(0.0, 1.0),
                );
            }
        }
    }
    widgets::draw_arrow(
        painter,
        pos2(arrow_x, body.center().y),
        1.0,
        palette.text_hint,
    );
    raised::focus_ring(painter, &response, chooser);

    let tip = tip(look);
    match footer {
        Some(words) => response.on_hover_text(format!("{tip}\n{words}")),
        None => response.on_hover_text(tip),
    }
}

pub(super) fn spoken(look: &Look) -> String {
    match &look.source {
        Source::None => "Sidechain source, none".to_owned(),
        Source::Track { name, .. } => {
            format!("Sidechain source, {name}, {}", tap_name(look.tap))
        }
        Source::Gone => "Sidechain source, removed".to_owned(),
    }
}

pub(super) fn tip(look: &Look) -> String {
    match &look.source {
        Source::None => "No track keys this effect. Click to choose one".to_owned(),
        Source::Gone => {
            "The track that keyed this effect was deleted. Undo brings it back; or choose another"
                .to_owned()
        }
        Source::Track { name, .. } => {
            let state = if look.level > 0.02 {
                format!("Receiving from {name}")
            } else if look.quiet {
                format!("No signal from {name}")
            } else if look.playing {
                format!("Waiting for sound from {name}")
            } else {
                "The song is stopped".to_owned()
            };
            format!("Keyed from {name}, {}. {state}", tap_name(look.tap))
        }
    }
}

impl Readings {
    pub(crate) fn update(&self, track: Id, peak: f32, playing: bool, dt: f32) -> Reading {
        let mut all = self.0.borrow_mut();
        let reading = all.entry(track).or_default();
        if reading.frozen {
            return *reading;
        }
        let peak = peak.clamp(0.0, 1.0);
        reading.level = if peak > reading.level {
            peak
        } else {
            reading.level * (-FALL * dt).exp()
        };
        if playing && peak < SILENT {
            reading.silent += dt;
        } else {
            reading.silent = 0.0;
        }
        *reading
    }

    pub(crate) fn pretend(&self, track: Id, level: f32, silent: f32) {
        self.0.borrow_mut().insert(
            track,
            Reading {
                level,
                silent,
                frozen: true,
            },
        );
    }

    pub(crate) fn level_of(&self, track: Id) -> f32 {
        self.get(track).level
    }

    fn get(&self, track: Id) -> Reading {
        self.0.borrow().get(&track).copied().unwrap_or_default()
    }
}

impl App {
    pub(super) fn read_sidechain(&mut self, ctx: &egui::Context, track: Id, playing: bool) {
        let Some(level) = self.transport.audio.mixer.take_side_level(track.0) else {
            return;
        };
        let peak = level.peak[0].max(level.peak[1]);
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.1);
        let reading = self.chain.sidechain.update(track, peak, playing, dt);
        if playing || reading.level > 0.02 {
            ctx.request_repaint();
        }
    }

    pub(super) fn sidechain_look(&self, track: Id, device: &Device, playing: bool) -> Option<Look> {
        let takes = match &device.plugin {
            PluginRef::Vst3 { .. } => {
                !device.is_instrument()
                    && matches!(self.hosting.state(device.id), Some(State::Ready))
                    && self.hosting.takes_sidechain(device.id)
            }
            PluginRef::Builtin { name } => crate::builtin::takes_sidechain(name) == Some(true),
        };
        if !takes {
            return None;
        }
        let source = match device.sidechain {
            None => Source::None,
            Some(sidechain) => {
                match lumen_project::routing::all_tracks(self.document.project())
                    .find(|t| t.id == sidechain.from)
                {
                    Some(t) => Source::Track {
                        name: t.name.clone(),
                        colour: color_of(t.color),
                    },
                    None => Source::Gone,
                }
            }
        };
        let reading = self.chain.sidechain.get(track);
        Some(Look {
            source,
            tap: device.sidechain.map_or(SidechainTap::PostFx, |s| s.tap),
            level: reading.level,
            quiet: playing && reading.silent > QUIET_AFTER,
            playing,
        })
    }

    pub(super) fn sidechain_menu(&mut self, chooser: &Response, device: Id) {
        widgets::Menu::below(chooser)
            .keeping_open()
            .show(|ui| self.sidechain_menu_body(ui, device));
    }

    pub(crate) fn open_sidechain_menu(&mut self, ctx: &egui::Context) {
        if let Some(device) = self.chain.selected {
            widgets::Menu::open_for(ctx, chooser_id(device));
        }
    }

    pub(crate) fn chosen_has_sidechain(&self) -> bool {
        self.chain.selected.is_some_and(|d| {
            find_device(self.document.project(), d).is_some_and(|(_, dev)| match &dev.plugin {
                PluginRef::Vst3 { .. } => !dev.is_instrument() && self.hosting.takes_sidechain(d),
                PluginRef::Builtin { name } => crate::builtin::takes_sidechain(name) == Some(true),
            })
        })
    }

    fn sidechain_menu_body(&mut self, ui: &mut Ui, device: Id) {
        let palette = *theme::p();
        ui.set_min_width(210.0);
        let Some((owner, held)) = find_device(self.document.project(), device) else {
            return;
        };
        let current = held.sidechain;
        let tap = current.map_or(SidechainTap::PostFx, |s| s.tap);
        let project = self.document.project().clone();
        let mut picked: Option<Option<Id>> = None;
        let heading = |ui: &mut Ui, text: &str| {
            ui.label(egui::RichText::new(text).small().color(palette.text_hint));
        };

        if widgets::choice_row(ui, "None", current.is_none()).clicked() {
            picked = Some(None);
            ui.close();
        }
        let mut last = None;
        for track in project.tracks.iter().filter(|t| t.id != owner) {
            let group = match track.kind {
                TrackKind::Group => "Groups",
                TrackKind::Return => "Returns",
                _ => "Tracks",
            };
            if last != Some(group) {
                heading(ui, group);
                last = Some(group);
            }
            let refusal = self.document.sidechain_refusal(device, track.id);
            ui.horizontal(|ui| {
                let (swatch, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter().rect_filled(swatch, 2.0, color_of(track.color));
                let on = current.is_some_and(|s| s.from == track.id);
                match refusal {
                    Some(why) => {
                        ui.add_enabled_ui(false, |ui| widgets::choice_row(ui, &track.name, false))
                            .response
                            .on_disabled_hover_text(why.to_string());
                    }
                    None => {
                        if widgets::choice_row(ui, &track.name, on).clicked() {
                            picked = Some(Some(track.id));
                            ui.close();
                        }
                    }
                }
            });
        }
        ui.separator();
        heading(ui, "Taken from the track");
        let mut tap_choices = [
            widgets::Choice::new(SidechainTap::PreFx, "Pre FX"),
            widgets::Choice::new(SidechainTap::PostFx, "Post FX"),
            widgets::Choice::new(SidechainTap::PostMixer, "Post mixer"),
        ];
        for choice in &mut tap_choices {
            choice.enabled = current.is_some();
            if current.is_none() {
                choice.tip = Some("Choose a track first");
            }
        }
        let new_tap = widgets::segmented(ui, ui.id().with("sidechain tap"), tap, &tap_choices);
        widgets::menu_caption(ui, tap_about(tap));

        let change = match (picked, new_tap) {
            (Some(None), _) => Some(None),
            (Some(Some(from)), _) => Some(Some(Sidechain { from, tap })),
            (None, Some(tap)) => current.map(|s| Some(Sidechain { tap, ..s })),
            (None, None) => None,
        };
        if let Some(sidechain) = change
            && let Err(why) = self.document.set_sidechain(device, sidechain)
        {
            self.say(why.to_string());
        }
    }
}
