//! A plugin's parameters in the app (M6 slice 4): the strip's knobs filled
//! from the plugin, what they do, and the panel with every parameter (the
//! owner's L1: one list, group chips, a star to put a parameter on the strip).

use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, FontId, Key, Rect, Sense, Ui, pos2, vec2};
use lumen_hosting::{ParamInfo, UnitInfo};
use lumen_project::model::{Device, Id};

use super::strip::{self, SLOTS, Slot};
use crate::widgets::param::{ParamLook, param_bar};
use crate::widgets::{self, Dialog, pill_toggle};
use crate::{App, theme};

/// Which parameters the panel lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chip {
    All,
    /// One group of the plugin's.
    Unit(i32),
    /// The ones on the strip.
    Starred,
}

/// The open panel.
/// What the panel's rows asked for in a frame.
#[derive(Default)]
struct RowOut {
    /// A bar was moved.
    set: Option<(u32, f64)>,
    /// Text typed for a parameter, for the plugin to read.
    typed: Option<(u32, String)>,
}

#[derive(Debug, Clone)]
pub(crate) struct Panel {
    pub device: Id,
    search: String,
    chip: Chip,
    /// The search field takes the keyboard when the panel opens.
    focus_search: bool,
    /// The row Enter stars.
    cursor: usize,
    /// Opened from an empty slot: say what to do.
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

/// How long the plugin is left before its text for a value is asked again.
const TEXT_EVERY: Duration = Duration::from_millis(50);
/// A row of the list.
const ROW: f32 = 28.0;

/// The name of a group: the plugin's, or "General" for the root and
/// "Group n" for one it did not name.
pub(crate) fn unit_name(units: &[UnitInfo], id: i32) -> String {
    match units.iter().find(|u| u.id == id) {
        Some(u) if !u.name.trim().is_empty() && u.id != 0 => u.name.trim().to_owned(),
        _ if id == 0 => "General".to_owned(),
        _ => format!("Group {id}"),
    }
}

/// The parameters the panel lists for `search` and `chip`, as indexes into
/// `list`: never hidden ones; the words of the search all have to be in the
/// name or the group's.
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

/// The chips: all, one for each group with something to list, and the
/// starred, each with how many it holds.
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
    // More than one group, or the chips say nothing the list does not.
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
    /// The knobs of `d`'s strip, and how many parameters it lists.
    pub(super) fn strip_of(&self, d: &Device) -> (Vec<Option<Slot>>, usize) {
        let list = self.hosting.parameters(d.id);
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

    fn slot_of(&self, d: &Device, p: &ParamInfo) -> Slot {
        let value = self.parameter_value(d, p);
        Slot {
            id: p.id,
            name: p.title.clone(),
            text: self.parameter_text(d.id, p, value),
            value,
            default: p.default,
            steps: p.steps,
            read_only: p.read_only(),
        }
    }

    /// A parameter's value now: what the project holds, else what the plugin
    /// has as far as Lumen knows, else its default.
    fn parameter_value(&self, d: &Device, p: &ParamInfo) -> f64 {
        d.params
            .get(&p.id)
            .copied()
            .or_else(|| self.hosting.value(d.id, p.id))
            .unwrap_or(p.default)
    }

    /// The plugin's own words for a value. The plugin is asked when the value
    /// changes (not faster than every [`TEXT_EVERY`]); until it answers, the
    /// answer to an earlier value stands, or a percentage.
    fn parameter_text(&self, device: Id, p: &ParamInfo, value: f64) -> String {
        let cached = self.hosting.text(device, p.id);
        if let Some((at, text)) = cached
            && (at - value).abs() < 1e-9
        {
            return text.clone();
        }
        let mut asked = self.chain.text_asked.borrow_mut();
        let key = (device.0, p.id);
        let recent = asked
            .get(&key)
            .is_some_and(|(v, when)| (v - value).abs() < 1e-9 || when.elapsed() < TEXT_EVERY);
        if recent {
            // Waiting on a frame to ask again, if the value moved on.
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
        cached.map_or_else(|| format!("{:.0} %", value * 100.0), |(_, t)| t.clone())
    }

    /// Applies what a strip's knob or button asked for.
    pub(super) fn apply_strip(&mut self, device: Id, output: strip::Output) {
        match output {
            strip::Output::Set(id, value) => self.set_parameter(device, id, value, "strip"),
            strip::Output::Type(id, text) => self.hosting.ask_value(device, id, text),
            strip::Output::Editor => self.toggle_editor(device),
            strip::Output::Learn => self.toggle_learn(device),
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

    /// The ids on `device`'s strip now.
    pub(crate) fn strip_ids(&self, device: Id) -> Vec<u32> {
        strip::resolve(
            self.hosting.parameters(device),
            self.chain.shown.get(&device).map(Vec::as_slice),
        )
    }

    /// Sets a parameter as one undo step per drag. `from` keeps the strip's
    /// drag and the panel's apart.
    fn set_parameter(&mut self, device: Id, id: u32, value: f64, from: &str) {
        let title = self
            .hosting
            .parameters(device)
            .iter()
            .find(|p| p.id == id)
            .map_or_else(|| "parameter".to_owned(), |p| p.title.clone());
        let _ = self.document.set_device_params(
            device,
            &[(id, value)],
            &format!("Change {title}"),
            Some(&format!("{from}:{}:{id}", device.0)),
        );
    }

    /// Puts a parameter on the strip, or takes it off. Says why when it
    /// cannot (the strip holds eight).
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

    /// Whether the chosen device is a running plugin that lists parameters.
    pub(crate) fn chosen_has_parameters(&self) -> bool {
        self.chain
            .selected
            .is_some_and(|d| !self.hosting.parameters(d).is_empty())
    }

    /// Whether the chosen device is a plugin that is running, so it has a
    /// window to ask for.
    pub(crate) fn chosen_has_editor(&self) -> bool {
        self.chain
            .selected
            .is_some_and(|d| matches!(self.hosting.state(d), Some(crate::hosting::State::Ready)))
    }

    /// Opens the panel for the chosen device (the command).
    pub(crate) fn open_parameters(&mut self) {
        if let Some(device) = self.chain.selected {
            self.chain.panel = Some(Panel::new(device, false));
        }
    }

    /// The panel with every parameter of one plugin, when open.
    pub(crate) fn parameters_panel(&mut self, ctx: &egui::Context) {
        let Some(mut panel) = self.chain.panel.take() else {
            return;
        };
        // Gone with its device.
        let Some(device) = crate::document::find_device(self.document.project(), panel.device)
            .map(|(_, d)| d.clone())
        else {
            return;
        };
        let list: Vec<ParamInfo> = self.hosting.parameters(device.id).to_vec();
        let units: Vec<UnitInfo> = self.hosting.units(device.id).to_vec();
        let shown = self.strip_ids(device.id);
        let title = format!("{} parameters", crate::document::device_name(&device));
        let mut starred: Vec<u32> = Vec::new();
        let mut act = RowOut::default();
        let close = Dialog::new("parameters", &title, 400.0)
            .at(
                pos2(ctx.content_rect().center().x, 96.0),
                Align2::CENTER_TOP,
            )
            .show(ctx, |ui| {
                if panel.fill {
                    ui.label(
                        egui::RichText::new("Star a parameter to put it on the card.")
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
                            self.parameter_row(
                                ui,
                                &device,
                                p,
                                &units,
                                shown.contains(&p.id),
                                r == panel.cursor,
                                &mut starred,
                                &mut act,
                            );
                        }
                    });
                ui.add_space(theme::space::XS);
                ui.label(
                    egui::RichText::new(format!("{} of {} on the strip", shown.len(), total))
                        .font(FontId::monospace(theme::size::MONO_SMALL))
                        .color(theme::p().text_hint),
                );
            });
        if let Some((id, value)) = act.set {
            self.set_parameter(device.id, id, value, "panel");
        }
        if let Some((id, text)) = act.typed {
            self.hosting.ask_value(device.id, id, text);
        }
        for id in starred {
            self.star(device.id, id);
        }
        if !close {
            self.chain.panel = Some(panel);
        }
    }

    /// One row of the panel: the star, the name and its group, the bar.
    #[allow(clippy::too_many_arguments)]
    fn parameter_row(
        &self,
        ui: &mut Ui,
        device: &Device,
        p: &ParamInfo,
        units: &[UnitInfo],
        starred_now: bool,
        under_cursor: bool,
        starred: &mut Vec<u32>,
        act: &mut RowOut,
    ) {
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
        let response = ui.interact(star, id.with("star"), Sense::click());
        let label = if starred_now {
            format!("Take {} off the strip", p.title)
        } else {
            format!("Put {} on the strip", p.title)
        };
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, starred_now, label.clone())
        });
        let painter = ui.painter();
        painter.text(
            star.center(),
            Align2::CENTER_CENTER,
            if starred_now { "\u{2605}" } else { "\u{2606}" },
            FontId::proportional(theme::size::BODY),
            if starred_now {
                palette.accent
            } else {
                palette.text_hint
            },
        );
        if response.on_hover_text(label).clicked() {
            starred.push(p.id);
        }
        let bar = Rect::from_min_max(
            pos2(row.right() - 124.0, row.top() + 3.0),
            pos2(row.right() - 4.0, row.bottom() - 3.0),
        );
        let name_room = bar.left() - (star.right() + 6.0) - 8.0;
        let font = FontId::proportional(theme::size::CAPTION);
        let group = unit_name(units, p.unit);
        let title = widgets::fit(painter, &p.title, font.clone(), name_room);
        let used = painter
            .layout_no_wrap(title.clone(), font.clone(), palette.text)
            .size()
            .x;
        painter.text(
            pos2(star.right() + 6.0, row.center().y),
            Align2::LEFT_CENTER,
            title,
            font.clone(),
            palette.text,
        );
        let left = name_room - used - 6.0;
        if left > 24.0 && p.unit != 0 {
            let group = widgets::fit(painter, &group, font.clone(), left);
            painter.text(
                pos2(star.right() + 6.0 + used + 6.0, row.center().y),
                Align2::LEFT_CENTER,
                group,
                FontId::proportional(theme::size::MONO_SMALL),
                palette.text_hint,
            );
        }
        let value = self.parameter_value(device, p);
        let text = self.parameter_text(device.id, p, value);
        let (_, out) = param_bar(
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
        // The name, in any case; the group's name counts too.
        assert_eq!(filter(&l, &u, &[], "CUT", Chip::All), [0]);
        assert_eq!(filter(&l, &u, &[], "filter", Chip::All), [0, 1]);
        assert_eq!(filter(&l, &u, &[], "filter res", Chip::All), [1]);
        assert!(filter(&l, &u, &[], "filter level", Chip::All).is_empty());
        // The hidden one cannot be found either.
        assert!(filter(&l, &u, &[], "hidden", Chip::All).is_empty());
    }

    #[test]
    fn a_chip_narrows_to_a_group_or_to_the_starred() {
        let (l, u) = (list(), units());
        assert_eq!(filter(&l, &u, &[], "", Chip::Unit(1)), [0, 1]);
        assert_eq!(filter(&l, &u, &[3, 5], "", Chip::Starred), [2, 4]);
        // A chip and a search together.
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
        // One group, nothing on the strip: just "All".
        let one = vec![p(1, "A", 0, 1), p(2, "B", 0, 1)];
        let c = chips(&one, &[], &[]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].2, 2);
    }
}
