use std::time::Instant;

use eframe::egui::{self, Align2, FontId, Key, Rect, Sense, Ui, pos2, vec2};
use lumen_hosting::{ParamInfo, UnitInfo};
use lumen_project::automation::Target;
use lumen_project::model::{Device, Id};

use super::strip::{self, SLOTS, Slot};
use crate::widgets::param::{ParamLook, param_bar};
use crate::widgets::{self, Dialog, pill_toggle};
use crate::{App, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chip {
    All,
    Unit(i32),
    Starred,
}

#[derive(Default)]
struct RowOut {
    set: Option<(u32, f64)>,
    typed: Option<(u32, String)>,
    ask: Option<(u32, widgets::Ask)>,
}

#[derive(Debug, Clone)]
pub(crate) struct Panel {
    pub device: Id,
    search: String,
    chip: Chip,
    focus_search: bool,
    cursor: usize,
    fill: bool,
}

impl Panel {
    pub(crate) fn new(device: Id, fill: bool) -> Self {
        Self {
            device,
            search: String::new(),
            chip: Chip::All,
            focus_search: true,
            cursor: 0,
            fill,
        }
    }
}

use crate::motion::{TEXT_AGAIN, TEXT_EVERY};
const ROW: f32 = 28.0;

struct RowLook<'a> {
    device: &'a Device,
    p: &'a ParamInfo,
    units: &'a [UnitInfo],
    starred_now: Option<bool>,
    under_cursor: bool,
}

pub(crate) fn unit_name(units: &[UnitInfo], id: i32) -> String {
    match units.iter().find(|u| u.id == id) {
        Some(u) if !u.name.trim().is_empty() && u.id != 0 => u.name.trim().to_owned(),
        _ if id == 0 => "General".to_owned(),
        _ => format!("Group {id}"),
    }
}

pub(crate) fn filter(
    list: &[ParamInfo],
    units: &[UnitInfo],
    shown: &[u32],
    search: &str,
    chip: Chip,
) -> Vec<usize> {
    let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
    list.iter()
        .enumerate()
        .filter(|(_, p)| strip::offered(p))
        .filter(|(_, p)| match chip {
            Chip::All => true,
            Chip::Unit(u) => p.unit == u,
            Chip::Starred => shown.contains(&p.id),
        })
        .filter(|(_, p)| {
            let hay = format!("{} {}", p.title, unit_name(units, p.unit)).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .map(|(i, _)| i)
        .collect()
}

pub(crate) fn chips(
    list: &[ParamInfo],
    units: &[UnitInfo],
    shown: &[u32],
) -> Vec<(Chip, String, usize)> {
    let offered: Vec<&ParamInfo> = list.iter().filter(|p| strip::offered(p)).collect();
    let mut out = vec![(Chip::All, "All".to_owned(), offered.len())];
    let mut groups: Vec<i32> = Vec::new();
    for p in &offered {
        if !groups.contains(&p.unit) {
            groups.push(p.unit);
        }
    }
    if groups.len() > 1 {
        for u in groups {
            let n = offered.iter().filter(|p| p.unit == u).count();
            out.push((Chip::Unit(u), unit_name(units, u), n));
        }
    }
    let starred = offered.iter().filter(|p| shown.contains(&p.id)).count();
    if starred > 0 {
        out.push((Chip::Starred, "On the strip".to_owned(), starred));
    }
    out
}

impl App {
    pub(crate) fn device_parameters(&self, d: &Device) -> &[ParamInfo] {
        if crate::builtin::has_table(d) {
            self.builtin.parameters(d.id)
        } else {
            self.hosting.parameters(d.id)
        }
    }

    pub(crate) fn device_units(&self, d: &Device) -> Vec<UnitInfo> {
        if crate::builtin::has_table(d) {
            Vec::new()
        } else {
            self.hosting.units(d.id).to_vec()
        }
    }

    pub(crate) fn parameter_choices(&self, d: &Device, id: u32) -> Vec<String> {
        if crate::builtin::has_table(d) {
            crate::builtin::choices(d, id)
        } else {
            self.hosting.choices(d.id, id).to_vec()
        }
    }

    pub(super) fn strip_of(&self, d: &Device) -> (Vec<Option<Slot>>, usize) {
        let list = self.device_parameters(d);
        let count = list.iter().filter(|p| strip::offered(p)).count();
        if count == 0 {
            return (Vec::new(), 0);
        }
        let ids = strip::resolve(list, self.chain.shown.get(&d.id).map(Vec::as_slice));
        let mut slots: Vec<Option<Slot>> = Vec::with_capacity(SLOTS);
        for id in ids {
            let Some(p) = list.iter().find(|p| p.id == id) else {
                continue;
            };
            slots.push(Some(self.slot_of(d, p)));
        }
        slots.resize(SLOTS, None);
        (slots, count)
    }

    pub(super) fn drawing_of(&self, d: &Device) -> Option<super::view::DrawingRow> {
        let lumen_project::model::PluginRef::Builtin { name } = &d.plugin else {
            return None;
        };
        let drawing = super::face::drawing_of(name)?;
        let parts = super::face::parts_of(name);
        let table = crate::builtin::table(name)?;
        let list = self.device_parameters(d);
        let mut ids = drawing.params();
        for id in parts.map_or(&[][..], |p| p.reads) {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        let parted = parts.is_some();
        let mut slots = ids
            .into_iter()
            .map(|id| {
                list.iter().find(|p| p.id == id).map(|p| {
                    if parted {
                        self.bare_slot_of(d, p)
                    } else {
                        self.slot_of(d, p)
                    }
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let mut picked = 0;
        if let Some(parts) = parts {
            let values = super::face::Values {
                slots: &slots,
                table,
            };
            picked = self
                .chain
                .bands
                .get(&d.id)
                .copied()
                .unwrap_or_else(|| (parts.first)(&values))
                .min(parts.count - 1);
            let mut wordy: Vec<u32> = Vec::new();
            for (index, control) in drawing.controls.iter().enumerate() {
                wordy.push(control.param);
                if let Some(aim) = (parts.aim)(&values, picked, index) {
                    wordy.push(aim.param);
                }
            }
            for id in wordy {
                if let (Some(at), Some(p)) = (
                    slots.iter().position(|s| s.id == id),
                    list.iter().find(|p| p.id == id),
                ) {
                    slots[at] = self.slot_of(d, p);
                }
            }
        }
        let spectrum = self
            .chain
            .pretend_spectrum
            .as_ref()
            .filter(|(id, _)| *id == d.id)
            .map(|(_, db)| db.clone())
            .or_else(|| {
                self.chain
                    .analyzers
                    .get(&d.id)
                    .map(|shown| shown.bins().to_vec())
            });
        Some(super::view::DrawingRow {
            drawing,
            parts,
            picked,
            spectrum,
            reading: self.builtin.reading(d.id),
            slots,
            table,
        })
    }

    fn bare_slot_of(&self, d: &Device, p: &ParamInfo) -> Slot {
        Slot {
            id: p.id,
            name: String::new(),
            text: String::new(),
            value: self.parameter_value(d, p),
            default: p.default,
            steps: p.steps,
            read_only: p.read_only(),
            automatable: p.can_automate(),
            choices: Vec::new(),
            over: false,
            mapped: None,
            waiting: false,
        }
    }

    fn slot_of(&self, d: &Device, p: &ParamInfo) -> Slot {
        let value = self.parameter_value(d, p);
        Slot {
            id: p.id,
            name: p.title.clone(),
            text: self.parameter_text(d, p, value),
            value,
            default: p.default,
            steps: p.steps,
            read_only: p.read_only(),
            automatable: p.can_automate(),
            choices: if p.steps > 1 {
                self.parameter_choices(d, p.id)
            } else {
                Vec::new()
            },
            over: self.parameter_taken_over(d, p),
            mapped: self.parameter_mapping(d, p),
            waiting: self.parameter_waiting(d, p),
        }
    }

    fn parameter_value(&self, d: &Device, p: &ParamInfo) -> f64 {
        if let Some(&value) = self.chain.deferred.get(&(d.id, p.id)) {
            return value;
        }
        if let Some(track) = self.track_of_device(d.id)
            && let Some(v) = self.arrangement.auto.following(
                self.document.overrides(),
                track,
                Target::Device {
                    device: d.id,
                    param: p.id,
                },
            )
        {
            return f64::from(v);
        }
        d.params
            .get(&p.id)
            .copied()
            .or_else(|| self.hosting.value(d.id, p.id))
            .unwrap_or(p.default)
    }
    fn track_of_device(&self, device: Id) -> Option<&lumen_project::model::Track> {
        let (id, _) = crate::document::find_device(self.document.project(), device)?;
        lumen_project::routing::all_tracks(self.document.project()).find(|t| t.id == id)
    }

    fn parameter_mapping(&self, d: &Device, p: &ParamInfo) -> Option<(u8, u8)> {
        let track = self.track_of_device(d.id)?.id;
        self.document
            .mapping_of(
                track,
                Target::Device {
                    device: d.id,
                    param: p.id,
                },
            )
            .map(|m| (m.channel, m.number))
    }

    fn parameter_waiting(&self, d: &Device, p: &ParamInfo) -> bool {
        self.track_of_device(d.id).is_some_and(|t| {
            self.controllers.learning
                == Some((
                    t.id,
                    Target::Device {
                        device: d.id,
                        param: p.id,
                    },
                ))
        })
    }

    pub(super) fn answer_ask(&mut self, device: Id, param: u32, ask: widgets::Ask) {
        let Some((track, _)) = crate::document::find_device(self.document.project(), device) else {
            return;
        };
        let target = Target::Device { device, param };
        match ask {
            widgets::Ask::Automate => {
                self.arrangement.auto.shown.insert(track, target);
            }
            widgets::Ask::Learn => self.start_learning(track, target),
            widgets::Ask::StopLearning => {
                self.stop_learning();
            }
            widgets::Ask::Clear => self.clear_controller(track, target),
            widgets::Ask::Window => self.controllers.window = true,
        }
    }

    fn parameter_taken_over(&self, d: &Device, p: &ParamInfo) -> bool {
        self.track_of_device(d.id).is_some_and(|t| {
            self.document.overrides().contains(
                t.id,
                Target::Device {
                    device: d.id,
                    param: p.id,
                },
            )
        })
    }

    fn parameter_text(&self, d: &Device, p: &ParamInfo, value: f64) -> String {
        if crate::builtin::has_table(d) {
            return crate::builtin::text(d, p.id, value)
                .unwrap_or_else(|| format!("{:.0} %", value * 100.0));
        }
        if let Some(text) = self.hosting.text(d.id, p.id, value) {
            return text.to_owned();
        }
        let device = d.id;
        let mut asked = self.chain.text_asked.borrow_mut();
        let forgotten = self.hosting.texts_forgotten();
        if self.chain.texts_seen.replace(forgotten) != forgotten {
            asked.clear();
        }
        let key = (device.0, p.id);
        let recent = asked.get(&key).is_some_and(|(v, when)| {
            ((v - value).abs() < 1e-9 && when.elapsed() < TEXT_AGAIN) || when.elapsed() < TEXT_EVERY
        });
        if recent {
            if asked
                .get(&key)
                .is_some_and(|(v, _)| (v - value).abs() >= 1e-9)
            {
                self.chain.text_pending.set(true);
            }
        } else {
            asked.insert(key, (value, Instant::now()));
            self.hosting.ask_text(device, p.id, value);
        }
        self.hosting
            .nearest_text(device, p.id, value)
            .map_or_else(|| format!("{:.0} %", value * 100.0), str::to_owned)
    }

    pub(super) fn apply_strip(&mut self, device: Id, output: strip::Output) {
        match output {
            strip::Output::Set(id, value) => self.set_parameter(device, id, value, "strip"),
            strip::Output::Moved(values) => self.move_parameters(device, &values),
            strip::Output::Reset(values) => self.reset_parameters(device, &values),
            strip::Output::Choose(id, value) => {
                self.document.seal();
                let title = self
                    .device_parameters_of(device)
                    .iter()
                    .find(|p| p.id == id)
                    .map_or_else(|| "parameter".to_owned(), |p| p.title.clone());
                self.document.override_param(device, id);
                let _ = self.document.set_device_params(
                    device,
                    &[(id, value)],
                    &format!("Change {title}"),
                    None,
                );
            }
            strip::Output::Type(id, text) => self.type_parameter(device, id, text),
            strip::Output::Editor => self.toggle_editor(device),
            strip::Output::Learn => self.toggle_learn(device),
            strip::Output::Ask(id, ask) => self.answer_ask(device, id, ask),
            strip::Output::End => self.end_editor_plugin(device),
            strip::Output::Remove(id) => {
                let mut ids = self.strip_ids(device);
                ids.retain(|i| *i != id);
                self.chain.shown.insert(device, ids);
            }
            strip::Output::Fill => self.chain.panel = Some(Panel::new(device, true)),
            strip::Output::All => self.chain.panel = Some(Panel::new(device, false)),
        }
    }

    pub(crate) fn strip_ids(&self, device: Id) -> Vec<u32> {
        let found = crate::document::find_device(self.document.project(), device);
        let list = found.map(|(_, d)| self.device_parameters(d)).unwrap_or(&[]);
        strip::resolve(list, self.chain.shown.get(&device).map(Vec::as_slice))
    }

    pub(crate) fn device_parameters_of(&self, device: Id) -> Vec<ParamInfo> {
        crate::document::find_device(self.document.project(), device)
            .map(|(_, d)| self.device_parameters(d).to_vec())
            .unwrap_or_default()
    }

    fn type_parameter(&mut self, device: Id, id: u32, text: String) {
        let Some((_, d)) = crate::document::find_device(self.document.project(), device) else {
            return;
        };
        if !crate::builtin::has_table(d) {
            self.hosting.ask_value(device, id, text);
            return;
        }
        let d = d.clone();
        match crate::builtin::value_of(&d, id, &text) {
            Some(value) => self.set_parameter(device, id, value, "typed"),
            None => {
                let name = crate::document::device_name(&d);
                self.say(format!("{name} did not understand \u{201c}{text}\u{201d}."));
            }
        }
    }

    fn set_parameter(&mut self, device: Id, id: u32, value: f64, from: &str) {
        let timing =
            crate::document::find_device(self.document.project(), device).is_some_and(|(_, d)| {
                crate::builtin::is_builtin(d)
                    && lumen_devices::timing_parameter(crate::builtin::name_of(d), id)
            });
        if timing && from != "typed" {
            self.chain.deferred.insert((device, id), value);
            return;
        }
        self.commit_parameter(device, id, value, from);
    }

    fn commit_parameter(&mut self, device: Id, id: u32, value: f64, from: &str) {
        self.chain.deferred.remove(&(device, id));
        let title = self.parameter_title(device, id);
        self.document.override_param(device, id);
        let merge = if from == "look" {
            None
        } else {
            Some(format!("{from}:{}:{id}", device.0))
        };
        let _ = self.document.set_device_params(
            device,
            &[(id, value)],
            &format!("Change {title}"),
            merge.as_deref(),
        );
    }

    pub(crate) fn apply_deferred(&mut self, ctx: &egui::Context) {
        if self.chain.deferred.is_empty() || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let done: Vec<(Id, u32, f64)> = self
            .chain
            .deferred
            .iter()
            .map(|(&(device, id), &value)| (device, id, value))
            .collect();
        self.chain.deferred.clear();
        for (device, id, value) in done {
            self.commit_parameter(device, id, value, "look");
        }
    }

    fn move_parameters(&mut self, device: Id, values: &[(u32, f64)]) {
        let Some(&(first, _)) = values.first() else {
            return;
        };
        let title = self.parameter_title(device, first);
        for &(id, _) in values {
            self.document.override_param(device, id);
        }
        let _ = self.document.set_device_params(
            device,
            values,
            &format!("Change {title}"),
            Some(&format!("face:{}", device.0)),
        );
    }

    fn reset_parameters(&mut self, device: Id, values: &[(u32, f64)]) {
        let Some(&(first, _)) = values.first() else {
            return;
        };
        let title = self.parameter_title(device, first);
        self.document.seal();
        for &(id, _) in values {
            self.document.override_param(device, id);
        }
        let _ = self
            .document
            .set_device_params(device, values, &format!("Reset {title}"), None);
    }

    fn parameter_title(&self, device: Id, id: u32) -> String {
        self.device_parameters_of(device)
            .iter()
            .find(|p| p.id == id)
            .map_or_else(|| "parameter".to_owned(), |p| p.title.clone())
    }

    fn star(&mut self, device: Id, id: u32) {
        let mut ids = self.strip_ids(device);
        if let Some(at) = ids.iter().position(|i| *i == id) {
            ids.remove(at);
        } else if ids.len() >= SLOTS {
            self.say("The strip holds eight. Take one off first.".to_owned());
            return;
        } else {
            ids.push(id);
        }
        self.chain.shown.insert(device, ids);
    }

    pub(crate) fn chosen_has_parameters(&self) -> bool {
        self.chain
            .selected
            .is_some_and(|d| !self.device_parameters_of(d).is_empty())
    }

    pub(crate) fn chosen_has_editor(&self) -> bool {
        self.chain
            .selected
            .is_some_and(|d| matches!(self.hosting.state(d), Some(crate::hosting::State::Ready)))
    }

    pub(crate) fn open_parameters(&mut self) {
        if let Some(device) = self.chain.selected {
            self.chain.panel = Some(Panel::new(device, false));
        }
    }

    pub(crate) fn parameters_panel(&mut self, ctx: &egui::Context) {
        let Some(mut panel) = self.chain.panel.take() else {
            return;
        };
        let Some(device) = crate::document::find_device(self.document.project(), panel.device)
            .map(|(_, d)| d.clone())
        else {
            return;
        };
        let list: Vec<ParamInfo> = self.device_parameters(&device).to_vec();
        let units: Vec<UnitInfo> = self.device_units(&device);
        let shown = self.strip_ids(device.id);
        let face = crate::chain::face::drawing_of(crate::builtin::name_of(&device)).is_some();
        let title = format!("{} parameters", crate::document::device_name(&device));
        let mut starred: Vec<u32> = Vec::new();
        let mut act = RowOut::default();
        let close = Dialog::new("parameters", &title, 400.0)
            .at(
                pos2(ctx.content_rect().center().x, 96.0),
                Align2::CENTER_TOP,
            )
            .show(ctx, |ui| {
                if panel.fill || face {
                    let words = if face {
                        "This device's controls are on its face; every parameter is here."
                    } else {
                        "Star a parameter to put it on the card."
                    };
                    ui.label(
                        egui::RichText::new(words)
                            .color(theme::p().text_secondary)
                            .size(theme::size::CAPTION),
                    );
                }
                let chip_list = chips(&list, &units, &shown);
                if !chip_list.iter().any(|(c, _, _)| *c == panel.chip) {
                    panel.chip = Chip::All;
                }
                let total = chip_list.first().map_or(0, |c| c.2);
                let search = ui.add(
                    egui::TextEdit::singleline(&mut panel.search)
                        .id(egui::Id::new("parameter search"))
                        .hint_text(format!("Search {total} parameters"))
                        .desired_width(f32::INFINITY),
                );
                search.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search parameters")
                });
                if panel.focus_search {
                    search.request_focus();
                    panel.focus_search = false;
                }
                ui.add_space(theme::space::XS);
                ui.horizontal_wrapped(|ui| {
                    for (i, (chip, label, n)) in chip_list.iter().enumerate() {
                        let text = format!("{label} {n}");
                        if pill_toggle(
                            ui,
                            egui::Id::new(("parameter chip", i)),
                            &text,
                            panel.chip == *chip,
                            "Show these parameters",
                        ) {
                            panel.chip = *chip;
                            panel.cursor = 0;
                        }
                    }
                });
                ui.add_space(theme::space::XS);
                let rows = filter(&list, &units, &shown, &panel.search, panel.chip);
                panel.cursor = panel.cursor.min(rows.len().saturating_sub(1));
                let (down, up, enter) = ui.input(|i| {
                    (
                        i.key_pressed(Key::ArrowDown),
                        i.key_pressed(Key::ArrowUp),
                        i.key_pressed(Key::Enter),
                    )
                });
                if down && panel.cursor + 1 < rows.len() {
                    panel.cursor += 1;
                }
                if up {
                    panel.cursor = panel.cursor.saturating_sub(1);
                }
                if enter && let Some(&at) = rows.get(panel.cursor) {
                    starred.push(list[at].id);
                }
                if rows.is_empty() {
                    ui.add_space(theme::space::M);
                    ui.label(
                        egui::RichText::new(
                            "No parameter matches. Clear the search to see them all.",
                        )
                        .color(theme::p().text_hint),
                    );
                    ui.add_space(theme::space::M);
                }
                egui::ScrollArea::vertical()
                    .max_height(ROW * 9.0)
                    .auto_shrink([false, true])
                    .show_rows(ui, ROW, rows.len(), |ui, range| {
                        for r in range {
                            let p = &list[rows[r]];
                            let look = RowLook {
                                device: &device,
                                p,
                                units: &units,
                                starred_now: (!face).then(|| shown.contains(&p.id)),
                                under_cursor: r == panel.cursor,
                            };
                            self.parameter_row(ui, look, &mut starred, &mut act);
                        }
                    });
                ui.add_space(theme::space::XS);
                let footer = if face {
                    format!("All {total} parameters")
                } else {
                    format!("{} of {} on the strip", shown.len(), total)
                };
                ui.label(
                    egui::RichText::new(footer)
                        .font(FontId::monospace(theme::size::MONO_SMALL))
                        .color(theme::p().text_hint),
                );
            });
        if let Some((id, value)) = act.set {
            self.set_parameter(device.id, id, value, "panel");
        }
        if let Some((id, text)) = act.typed {
            self.type_parameter(device.id, id, text);
        }
        if let Some((id, ask)) = act.ask {
            self.answer_ask(device.id, id, ask);
        }
        for id in starred {
            self.star(device.id, id);
        }
        if !close {
            self.chain.panel = Some(panel);
        }
    }

    fn parameter_row(
        &self,
        ui: &mut Ui,
        look: RowLook<'_>,
        starred: &mut Vec<u32>,
        act: &mut RowOut,
    ) {
        let RowLook {
            device,
            p,
            units,
            starred_now,
            under_cursor,
        } = look;
        let palette = *theme::p();
        let width = ui.available_width();
        let (row, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
        let painter = ui.painter();
        if under_cursor {
            painter.rect_filled(row, 4.0, palette.accent_muted);
        }
        let star =
            Rect::from_center_size(pos2(row.left() + 11.0, row.center().y), vec2(20.0, 20.0));
        let id = egui::Id::new(("parameter row", device.id.0, p.id));
        let mut text_left = star.right() + 6.0;
        match starred_now {
            Some(on) => {
                let response = ui.interact(star, id.with("star"), Sense::click());
                let label = if on {
                    format!("Take {} off the strip", p.title)
                } else {
                    format!("Put {} on the strip", p.title)
                };
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, label.clone())
                });
                let painter = ui.painter();
                painter.text(
                    star.center(),
                    Align2::CENTER_CENTER,
                    if on { "\u{2605}" } else { "\u{2606}" },
                    FontId::proportional(theme::size::BODY),
                    if on {
                        palette.accent
                    } else {
                        palette.text_hint
                    },
                );
                if response.on_hover_text(label).clicked() {
                    starred.push(p.id);
                }
            }
            None => text_left = row.left() + 4.0,
        }
        let bar = Rect::from_min_max(
            pos2(row.right() - 124.0, row.top() + 3.0),
            pos2(row.right() - 4.0, row.bottom() - 3.0),
        );
        let name_room = bar.left() - text_left - 8.0;
        let font = FontId::proportional(theme::size::CAPTION);
        let group = unit_name(units, p.unit);
        let painter = ui.painter();
        let title = widgets::fit(painter, &p.title, font.clone(), name_room);
        let used = painter
            .layout_no_wrap(title.clone(), font.clone(), palette.text)
            .size()
            .x;
        painter.text(
            pos2(text_left, row.center().y),
            Align2::LEFT_CENTER,
            title,
            font.clone(),
            palette.text,
        );
        let left = name_room - used - 6.0;
        if left > 24.0 && p.unit != 0 {
            let group = widgets::fit(painter, &group, font.clone(), left);
            painter.text(
                pos2(text_left + used + 6.0, row.center().y),
                Align2::LEFT_CENTER,
                group,
                FontId::proportional(theme::size::MONO_SMALL),
                palette.text_hint,
            );
        }
        let value = self.parameter_value(device, p);
        let text = self.parameter_text(device, p, value);
        let (response, out) = param_bar(
            ui,
            bar,
            id.with("bar"),
            ParamLook {
                name: &p.title,
                text: &text,
                value,
                default: p.default,
                steps: p.steps,
                read_only: p.read_only(),
                selected: false,
            },
        );
        if self.parameter_taken_over(device, p) {
            crate::arrangement::follow::ring(ui, bar);
        }
        let (mapped, waiting) = (
            self.parameter_mapping(device, p),
            self.parameter_waiting(device, p),
        );
        if waiting {
            crate::arrangement::follow::ring_with(ui, bar, palette.accent);
        }
        widgets::Menu::context(&response).show(|ui| {
            if p.can_automate() {
                if let Some(ask) = widgets::control_items(ui, mapped, waiting) {
                    act.ask = Some((p.id, ask));
                }
            } else {
                widgets::menu_caption(ui, "Not automatable: it is the device's timing.");
            }
        });
        if let Some(v) = out.value {
            act.set = Some((p.id, v));
        }
        if let Some(text) = out.typed {
            act.typed = Some((p.id, text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: u32, title: &str, unit: i32, flags: u32) -> ParamInfo {
        ParamInfo {
            id,
            title: title.into(),
            units: String::new(),
            steps: 0,
            default: 0.5,
            flags,
            unit,
            value: 0.5,
        }
    }

    fn units() -> Vec<UnitInfo> {
        vec![
            UnitInfo {
                id: 0,
                parent: -1,
                name: "Root".into(),
            },
            UnitInfo {
                id: 1,
                parent: 0,
                name: "Filter".into(),
            },
            UnitInfo {
                id: 2,
                parent: 0,
                name: "  ".into(),
            },
        ]
    }

    fn list() -> Vec<ParamInfo> {
        vec![
            p(1, "Cutoff", 1, 1),
            p(2, "Resonance", 1, 1),
            p(3, "Level", 0, 1),
            p(4, "Hidden thing", 1, 1 << 4),
            p(5, "Drive", 2, 1),
        ]
    }

    #[test]
    fn groups_are_named_by_the_plugin_or_by_number() {
        let u = units();
        assert_eq!(unit_name(&u, 1), "Filter");
        assert_eq!(unit_name(&u, 0), "General");
        assert_eq!(unit_name(&u, 2), "Group 2", "a blank name is no name");
        assert_eq!(unit_name(&u, 9), "Group 9");
        assert_eq!(unit_name(&[], 0), "General");
    }

    #[test]
    fn hidden_parameters_are_never_listed_and_a_search_needs_every_word() {
        let (l, u) = (list(), units());
        let all = filter(&l, &u, &[], "", Chip::All);
        assert_eq!(all, [0, 1, 2, 4], "the hidden one is left out");
        assert_eq!(filter(&l, &u, &[], "CUT", Chip::All), [0]);
        assert_eq!(filter(&l, &u, &[], "filter", Chip::All), [0, 1]);
        assert_eq!(filter(&l, &u, &[], "filter res", Chip::All), [1]);
        assert!(filter(&l, &u, &[], "filter level", Chip::All).is_empty());
        assert!(filter(&l, &u, &[], "hidden", Chip::All).is_empty());
    }

    #[test]
    fn a_chip_narrows_to_a_group_or_to_the_starred() {
        let (l, u) = (list(), units());
        assert_eq!(filter(&l, &u, &[], "", Chip::Unit(1)), [0, 1]);
        assert_eq!(filter(&l, &u, &[3, 5], "", Chip::Starred), [2, 4]);
        assert_eq!(filter(&l, &u, &[], "res", Chip::Unit(1)), [1]);
        assert!(filter(&l, &u, &[], "res", Chip::Unit(0)).is_empty());
    }

    #[test]
    fn chips_count_what_they_hold_and_say_nothing_when_there_is_one_group() {
        let (l, u) = (list(), units());
        let c = chips(&l, &u, &[1]);
        let labels: Vec<_> = c.iter().map(|(_, n, k)| (n.as_str(), *k)).collect();
        assert_eq!(
            labels,
            [
                ("All", 4),
                ("Filter", 2),
                ("General", 1),
                ("Group 2", 1),
                ("On the strip", 1)
            ]
        );
        let one = vec![p(1, "A", 0, 1), p(2, "B", 0, 1)];
        let c = chips(&one, &[], &[]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].2, 2);
    }
}
