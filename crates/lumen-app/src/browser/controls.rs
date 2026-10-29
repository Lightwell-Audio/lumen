//! The controls above the list: the search field, the type switch, the
//! category chips and the scan line. Each draws itself and says what was
//! done; the panel applies it.

use std::time::Duration;

use eframe::egui::{self, FontId, Id, Rect, Sense, Ui, pos2, vec2};
use lumen_plugin_catalog::catalog::Source;

use super::view::{Chip, TypeFilter};
use crate::icons::{self, Icon};
use crate::plugins::{Plugins, Status};
use crate::{theme, widgets};

/// The magnifier's side, the list's icon size (the interface audit's B1).
const MAGNIFIER: f32 = super::list::ICON;

/// Where the search field's words start, from its left edge: a step in,
/// the magnifier and a step. The magnifier's centre is then on the list's
/// icon column and the words on the list's names.
const TEXT_EDGE: f32 = theme::space::S + MAGNIFIER + theme::space::S;

/// What the search field did this frame.
pub(super) struct Searched {
    /// Esc pressed while the field had the keyboard.
    pub escaped: bool,
}

/// The search field: a sunk well with a magnifier and, once there are words,
/// a way to clear them. `focus` asks for the keyboard (a command did).
pub(super) fn search(
    ui: &mut Ui,
    text: &mut String,
    focus: &mut bool,
    hint: &'static str,
) -> Searched {
    let palette = *theme::p();
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::size::CONTROL_LARGE),
        Sense::hover(),
    );
    widgets::raised::paint_field(ui.painter(), rect, theme::radius::MEDIUM, 0.0);
    // The magnifier sits a step inside the well, over the list's icons.
    let icon = Rect::from_center_size(
        pos2(
            rect.left() + theme::space::S + MAGNIFIER / 2.0,
            rect.center().y,
        ),
        vec2(MAGNIFIER, MAGNIFIER),
    );
    ui.painter()
        .extend(icons::shapes(Icon::Search, icon, palette.text_hint));

    let clear_room = if text.is_empty() {
        0.0
    } else {
        theme::size::CONTROL
    };
    let right_pad = if text.is_empty() {
        theme::space::M
    } else {
        clear_room + theme::space::XS
    };
    let edit = Rect::from_min_max(
        pos2(rect.left() + TEXT_EDGE, rect.top()),
        pos2(rect.right() - right_pad, rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(edit)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let field = child.add(
        egui::TextEdit::singleline(text)
            .id(Id::new(("browser search", hint)))
            .frame(egui::Frame::NONE)
            .font(FontId::proportional(theme::size::BODY))
            .hint_text(hint)
            .desired_width(edit.width()),
    );
    field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, hint));
    if std::mem::take(focus) {
        field.request_focus();
    }
    let mut escaped = field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
    if field.has_focus() {
        widgets::raised::paint_focus(ui.painter(), rect, theme::radius::MEDIUM);
    }

    if !text.is_empty() {
        let clear = Rect::from_center_size(
            pos2(
                rect.right() - theme::space::XS - clear_room / 2.0,
                rect.center().y,
            ),
            vec2(clear_room, theme::size::CONTROL),
        );
        let response = ui.interact(clear, Id::new("browser search clear"), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Clear search")
        });
        let response = widgets::tip(response, "Clear search", None, None);
        let ink = if response.hovered() {
            palette.text
        } else {
            palette.text_hint
        };
        let c = clear.center();
        let stroke = egui::Stroke::new(theme::stroke::LIT, ink);
        ui.painter()
            .line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], stroke);
        ui.painter()
            .line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], stroke);
        if response.clicked() {
            text.clear();
            field.request_focus();
        }
        widgets::raised::focus_ring(ui.painter(), &response, clear.shrink(2.0));
    }
    // egui drops the field's focus on Esc before this code runs, so the
    // field also counts as escaped when it lost focus that way.
    escaped |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
    Searched { escaped }
}

/// All, Instruments and Effects across the search field's width: in equal
/// pieces where they fit so.
/// Returns the choice made, if another.
pub(super) fn type_filter(ui: &mut Ui, kind: TypeFilter) -> Option<TypeFilter> {
    widgets::segmented_full(
        ui,
        Id::new("browser type"),
        kind,
        &[
            widgets::Choice::new(TypeFilter::All, "All")
                .named("All devices")
                .tip("Instruments and effects"),
            widgets::Choice::new(TypeFilter::Instruments, "Instruments")
                .named("Instruments only")
                .tip("Only instruments: devices that make sound from notes"),
            widgets::Choice::new(TypeFilter::Effects, "Effects")
                .named("Effects only")
                .tip("Only effects: devices that change a sound"),
        ],
    )
}

/// The chip a click on `chip` chooses: turning the one that is on off goes
/// back to everything.
fn flip(chip: Chip, on: bool) -> Chip {
    if on { Chip::All } else { chip }
}

/// Category chips shown before the rest go under "More".
const CHIPS_SHOWN: usize = 5;

/// The chips: Favorites, Recent and the commonest categories in one flow,
/// filling each line, their words starting on the search field's left
/// edge, the rest under "More ▾". A chip that is on turns off when clicked
/// again. Returns the chip now chosen (`Chip::All` when the one on was
/// clicked off).
pub(super) fn chips(ui: &mut Ui, chosen: &Chip, categories: &[(String, usize)]) -> Option<Chip> {
    let mut picked = None;
    // A chosen category stays in view even when it is not among the commonest.
    let mut shown: Vec<&(String, usize)> = categories.iter().take(CHIPS_SHOWN).collect();
    if let Chip::Category(wanted) = chosen
        && !shown.iter().any(|(n, _)| n.eq_ignore_ascii_case(wanted))
        && let Some(found) = categories
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(wanted))
    {
        shown.push(found);
    }
    let rest: Vec<&(String, usize)> = categories
        .iter()
        .filter(|c| !shown.iter().any(|s| s.0 == c.0))
        .collect();
    // Each chip has a step of room before and after its words, so the flow
    // starts and ends that much outside the field's edges: the words line
    // up with the field and the filter, and a lit chip's fill hangs out.
    let hang = -(theme::space::S as i8);
    egui::Frame::NONE
        .outer_margin(egui::Margin {
            left: hang,
            right: hang,
            ..Default::default()
        })
        .show(ui, |ui| {
            // The pane clips its body a step in from its edge, which would
            // cut the hanging fill of a lit first chip: let the chips draw
            // to half a step from the pane's edge.
            ui.set_clip_rect(ui.clip_rect().expand2(vec2(theme::space::XS, 0.0)));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(theme::space::XS, theme::space::XS);
                for (chip, label, tip) in [
                    (Chip::Favorites, "Favorites", "Only the devices you starred"),
                    (Chip::Recent, "Recent", "The devices you chose lately"),
                ] {
                    let on = *chosen == chip;
                    if widgets::chip_toggle(ui, Id::new(("browser chip", label)), label, on, tip) {
                        picked = Some(flip(chip, on));
                    }
                }
                for (name, count) in shown {
                    let on = matches!(chosen, Chip::Category(c) if c.eq_ignore_ascii_case(name));
                    let tip = format!("{count} {}", if *count == 1 { "device" } else { "devices" });
                    if widgets::chip_toggle(ui, Id::new(("browser chip", name)), name, on, &tip) {
                        picked = Some(flip(Chip::Category(name.clone()), on));
                    }
                }
                if !rest.is_empty() {
                    let more =
                        widgets::chip_button(ui, Id::new("browser more chips"), "More", true);
                    widgets::Menu::below(&more).show(|ui| {
                        for (name, count) in rest {
                            let label = format!("{name}  ({count})");
                            if widgets::menu_item(ui, &label, None, true).clicked() {
                                picked = Some(Chip::Category(name.clone()));
                            }
                        }
                    });
                }
            })
        });
    picked
}

/// What the scan line asks for.
#[derive(Default)]
pub(super) struct ScanActions {
    pub rescan: bool,
    pub cancel: bool,
    pub dismiss_note: bool,
}

/// "2 min ago" for a scan that finished `since` ago.
pub(super) fn ago(since: Duration) -> String {
    let secs = since.as_secs();
    match secs {
        0..=44 => "just now".into(),
        45..=3599 => format!("{} min ago", (secs + 30) / 60),
        3600..=86_399 => format!("{} h ago", (secs + 1800) / 3600),
        _ => format!("{} d ago", (secs + 43_200) / 86_400),
    }
}

/// The list's header under a hairline: what is installed and when it was
/// looked for, or how far a scan is; Rescan or Cancel as a bare word, like
/// the chips; a card for a problem. Returns what was pressed.
pub(super) fn scan_line(ui: &mut Ui, plugins: &Plugins) -> ScanActions {
    let palette = *theme::p();
    let mut actions = ScanActions::default();
    let catalog = plugins.catalog();
    let installed = catalog
        .devices()
        .iter()
        .filter(|d| d.source == Source::Vst3)
        .count();
    let vendors = catalog
        .vendors()
        .iter()
        .filter(|(v, _)| !v.eq_ignore_ascii_case("lumen"))
        .count();
    let blocked = catalog.blocked().len();

    // The line says little so it fits a narrow pane; the rest is its tip.
    let (text, detail, warn, button) = match plugins.status() {
        Status::Off => {
            let t = "Plugins are off for this run".to_owned();
            (t.clone(), t, false, None)
        }
        Status::Loading => {
            let t = "Reading the plugin list…".to_owned();
            (t.clone(), t, false, None)
        }
        Status::Scanning { done: _, total: 0 } => {
            let t = "Looking for plugins…".to_owned();
            (t.clone(), t, false, Some("Cancel"))
        }
        Status::Scanning { done, total } => {
            let t = format!("Scanning {done} of {total}");
            (t.clone(), t, false, Some("Cancel"))
        }
        Status::Idle | Status::Failed(_) => {
            let mut text = match installed {
                0 => "No plugins found".to_owned(),
                1 => "1 plugin".to_owned(),
                n => format!("{n} plugins"),
            };
            if blocked > 0 {
                text.push_str(&format!(" · {blocked} blocked"));
            }
            let mut detail = text.clone();
            if vendors > 0 {
                detail.push_str(&format!(
                    " · {vendors} {}",
                    if vendors == 1 { "vendor" } else { "vendors" }
                ));
            }
            match plugins.finished_at().and_then(|at| at.elapsed().ok()) {
                Some(since) => detail.push_str(&format!(" · scanned {}", ago(since))),
                None => detail.push_str(" · from the saved list"),
            }
            (text, detail, false, Some("Rescan"))
        }
    };
    // The hairline parts the filters from the list (the interface audit's B3).
    ui.add_space(theme::space::XS);
    let (rule, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), theme::stroke::HAIRLINE),
        Sense::hover(),
    );
    ui.painter().hline(
        rule.x_range(),
        rule.center().y,
        egui::Stroke::new(theme::stroke::HAIRLINE, palette.border),
    );
    let line = vec2(ui.available_width(), theme::size::CONTROL);
    ui.allocate_ui_with_layout(
        line,
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            if let Some(label) = button {
                let response =
                    widgets::chip_button(ui, Id::new(("browser scan", label)), label, false);
                let tip = match label {
                    "Cancel" => "Stop scanning; what was found stays",
                    _ => "Look for new and changed plugins",
                };
                if widgets::tip(response, tip, None, None).clicked() {
                    if label == "Cancel" {
                        actions.cancel = true;
                    } else {
                        actions.rescan = true;
                    }
                }
            }
            // What is left of the line is the words, from the left edge, cut
            // with "…" when short.
            let rest = vec2(ui.available_width(), theme::size::CONTROL);
            ui.allocate_ui_with_layout(
                rest,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let label = ui.add(
                        egui::Label::new(
                            egui::RichText::new(&text)
                                .size(theme::size::CAPTION)
                                .color(if warn {
                                    palette.warning
                                } else {
                                    palette.text_secondary
                                }),
                        )
                        .truncate(),
                    );
                    label.on_hover_text(&detail);
                },
            );
        },
    );
    if let Status::Scanning { done, total } = plugins.status() {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), Sense::hover());
        ui.painter().rect_filled(rect, 1.5, palette.well);
        let fraction = if *total == 0 {
            0.0
        } else {
            *done as f32 / *total as f32
        };
        let filled = Rect::from_min_size(rect.min, vec2(rect.width() * fraction, rect.height()));
        ui.painter().rect_filled(filled, 1.5, palette.accent);
    }
    if let Status::Failed(why) = plugins.status() {
        card(ui, why, true, None);
    }
    if let Some(note) = plugins.note() {
        card(ui, note, false, Some(&mut actions.dismiss_note));
    }
    actions
}

/// A short card for a problem: the words wrapped, and a Dismiss when it can
/// be put away.
fn card(ui: &mut Ui, text: &str, warning: bool, dismiss: Option<&mut bool>) {
    let palette = *theme::p();
    egui::Frame::new()
        .fill(palette.raised)
        .stroke(egui::Stroke::new(theme::stroke::HAIRLINE, palette.border))
        .corner_radius(theme::radius::LARGE)
        .inner_margin(egui::Margin::same(theme::space::S as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if warning {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    ui.painter()
                        .extend(icons::shapes(Icon::Warning, rect, palette.warning));
                }
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(text)
                            .size(theme::size::CAPTION)
                            .color(palette.text),
                    )
                    .wrap(),
                );
            });
            if let Some(dismiss) = dismiss
                && widgets::raised::button(ui, "Dismiss").clicked()
            {
                *dismiss = true;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scan_that_finished_is_aged_in_the_unit_that_fits() {
        assert_eq!(ago(Duration::from_secs(5)), "just now");
        assert_eq!(ago(Duration::from_secs(60)), "1 min ago");
        assert_eq!(ago(Duration::from_secs(150)), "3 min ago");
        assert_eq!(ago(Duration::from_secs(2 * 3600)), "2 h ago");
        assert_eq!(ago(Duration::from_secs(3 * 86_400)), "3 d ago");
    }
}
