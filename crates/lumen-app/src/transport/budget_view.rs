//! The block budget in the audio panel (ADR-0075, the owner's B): one bar
//! for an average block, Lumen's own work then each plugin's (solid, the
//! plugin working; hatched, the rest of the wait), the plugins' deadline
//! marked; under it every plugin by time, in a list that scrolls. The five
//! costliest plugins have their own segments and the rest share one; a row
//! under the pointer lights its segment.

use std::time::Duration;

use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use super::budget::{Budget, MISS_WINDOW, Snapshot};
use crate::{theme, widgets};

/// Plugins with a segment of their own; the rest share one.
const OWN: usize = 5;

/// Space between a hatch's lines.
const HATCH: f32 = 5.0;

/// How much an unlit segment keeps while a row is hovered.
const UNLIT: f32 = 0.3;

fn ms(d: Duration) -> String {
    format!("{:.2}", d.as_secs_f64() * 1e3)
}

fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// The colour of plugin `i`'s segment and swatch.
fn shade(i: usize) -> Color32 {
    let p = theme::p();
    p.budget.get(i).copied().unwrap_or(p.text_disabled)
}

/// Fills `rect` with a hatch of `color` over the muted accent.
fn hatch(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.rect_filled(rect, 0.0, theme::p().accent_muted);
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let stroke = Stroke::new(theme::stroke::LIT, color);
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        painter.line_segment(
            [pos2(x, rect.bottom()), pos2(x + rect.height(), rect.top())],
            stroke,
        );
        x += HATCH;
    }
}

/// A legend or list swatch at the start of `rect`'s row.
fn swatch(painter: &egui::Painter, at: egui::Pos2, color: Color32, hatched: bool) -> Rect {
    let side = theme::size::BUDGET_SWATCH;
    let rect = Rect::from_min_size(pos2(at.x, at.y - side / 2.0), vec2(side, side));
    if hatched {
        hatch(painter, rect, color);
    } else {
        painter.rect_filled(rect, f32::from(theme::radius::SMALL) / 2.0, color);
    }
    rect
}

/// The panel's budget section. Draws nothing until a stretch has been taken.
pub(super) fn show(ui: &mut Ui, budget: &mut Budget) {
    let Some(snap) = budget.shown.as_ref() else {
        return;
    };
    let p = theme::p();
    let over = snap.used > snap.deadline;
    let share = snap.used.as_secs_f64() / snap.block.as_secs_f64().max(f64::EPSILON);
    ui.horizontal(|ui| {
        ui.label("Each block");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!(
                    "{} of {} ms · {:.0}%",
                    ms(snap.used),
                    ms(snap.block),
                    share * 100.0
                ))
                .font(mono(theme::size::MONO_SMALL))
                .color(if over { p.warning } else { p.text }),
            )
            .on_hover_text(
                "The audio's mean time per block over the last second, of the time each \
                 block allows. Plugins must answer by the marked line",
            );
        });
    });
    bar(ui, snap, budget.hover);
    legend(ui);
    let missing = snap.missing();
    if missing > 0 {
        warning(ui, missing);
    }
    let (hover, height) = list(ui, snap, budget.list_room);
    budget.hover = hover;
    budget.list_height = height;
}

fn bar(ui: &mut Ui, snap: &Snapshot, hover: Option<usize>) {
    let p = theme::p();
    let width = ui.available_width();
    let label_room = theme::size::SCALE + theme::space::XXS;
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, theme::size::BUDGET_BAR + label_room),
        Sense::hover(),
    );
    let track = Rect::from_min_size(rect.min, vec2(width, theme::size::BUDGET_BAR));
    let painter = ui.painter_at(rect);
    let radius = f32::from(theme::radius::SMALL);
    painter.rect_filled(track, radius, p.well);
    let per_second = width as f64 / snap.block.as_secs_f64().max(f64::EPSILON);
    let span = |d: Duration| (d.as_secs_f64() * per_second) as f32;
    let lit = |on: bool| if hover.is_none() || on { 1.0 } else { UNLIT };
    let clipped = painter.with_clip_rect(track);
    let mut x = track.left();
    let mut piece = |len: f32, color: Color32, hatched: bool| {
        if len <= 0.0 {
            return;
        }
        let seg = Rect::from_x_y_ranges(x..=x + len, track.y_range());
        if hatched {
            hatch(&clipped, seg, color);
        } else {
            clipped.rect_filled(seg, 0.0, color);
        }
        x += len;
    };
    piece(
        span(snap.lumen),
        p.text_secondary.gamma_multiply(lit(false)),
        false,
    );
    for (i, plugin) in snap.plugins.iter().take(OWN).enumerate() {
        let color = shade(i).gamma_multiply(lit(hover == Some(i)));
        piece(span(plugin.working()), color, false);
        piece(span(plugin.trip()), color, true);
    }
    let rest: Duration = snap.plugins.iter().skip(OWN).map(|p| p.waited).sum();
    let rest_lit = hover.is_some_and(|h| h >= OWN);
    piece(span(rest), shade(OWN).gamma_multiply(lit(rest_lit)), false);

    let line = track.left() + span(snap.deadline);
    painter.line_segment(
        [
            pos2(line, track.top()),
            pos2(line, track.bottom() + theme::space::XXS),
        ],
        Stroke::new(theme::stroke::FOCUS, p.warning),
    );
    painter.text(
        pos2(line, track.bottom() + theme::space::XXS),
        Align2::CENTER_TOP,
        ms(snap.deadline),
        mono(theme::size::SCALE),
        p.warning,
    );
    let summary = format!(
        "Block budget: {} of {} ms, Lumen {} ms, {} plugins, plugins answer by {} ms",
        ms(snap.used),
        ms(snap.block),
        ms(snap.lumen),
        snap.plugins.len(),
        ms(snap.deadline)
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &summary));
}

fn legend(ui: &mut Ui) {
    let p = theme::p();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::space::M;
        let caption = |ui: &mut Ui, text: &str, mark: &dyn Fn(&egui::Painter, egui::Pos2)| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::space::XS;
                let side = theme::size::BUDGET_SWATCH;
                let (rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
                mark(ui.painter(), rect.left_center());
                ui.label(
                    RichText::new(text)
                        .size(theme::size::CAPTION)
                        .color(p.text_secondary),
                );
            });
        };
        caption(ui, "work", &|painter, at| {
            swatch(painter, at, p.accent, false);
        });
        caption(ui, "waiting on the trip", &|painter, at| {
            swatch(painter, at, p.accent, true);
        });
        caption(ui, "plugins answer by", &|painter, at| {
            let x = at.x + theme::size::BUDGET_SWATCH / 2.0;
            let half = theme::size::BUDGET_SWATCH / 2.0;
            painter.line_segment(
                [pos2(x, at.y - half), pos2(x, at.y + half)],
                Stroke::new(theme::stroke::FOCUS, p.warning),
            );
        });
    });
}

fn warning(ui: &mut Ui, missing: usize) {
    let p = theme::p();
    let minutes = MISS_WINDOW.as_secs() / 60;
    let who = if missing == 1 {
        "1 plugin missed blocks".to_string()
    } else {
        format!("{missing} plugins missed blocks")
    };
    let window = if minutes == 1 {
        "the last minute".to_string()
    } else {
        format!("the last {minutes} minutes")
    };
    egui::Frame::new()
        .fill(p.warning.gamma_multiply(0.10))
        .stroke(Stroke::new(
            theme::stroke::HAIRLINE,
            p.warning.gamma_multiply(0.35),
        ))
        .corner_radius(theme::radius::MEDIUM)
        .inner_margin(egui::Margin::symmetric(
            theme::space::S as i8,
            theme::space::XS as i8 + 2,
        ))
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.label(RichText::new("▲").color(p.warning));
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "{who} in {window} (marked below). Each missed block plays \
                             silent or dry and can pop. A larger buffer gives them more time."
                        ))
                        .size(theme::size::CAPTION),
                    )
                    .wrap(),
                );
            });
        });
}

/// The rows: Lumen, then every plugin in a list that scrolls, at most
/// `room` tall (and at least two rows). Returns the plugin row under the
/// pointer and the list's height.
fn list(ui: &mut Ui, snap: &Snapshot, room: f32) -> (Option<usize>, f32) {
    let p = theme::p();
    if !snap.plugins.is_empty() {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("PLUGINS · {}", snap.plugins.len()))
                    .size(theme::size::CAPTION)
                    .color(p.text_secondary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new("by time · work + trip, ms")
                        .size(theme::size::CAPTION)
                        .color(p.text_secondary),
                );
            });
        });
    }
    let row_height = theme::size::BODY + theme::size::CAPTION + theme::space::M;
    let lumen = ui
        .allocate_exact_size(
            vec2(ui.available_width(), theme::size::BODY + theme::space::S),
            Sense::hover(),
        )
        .0;
    let painter = ui.painter();
    swatch(
        painter,
        pos2(lumen.left() + theme::space::XS, lumen.center().y),
        p.text_secondary,
        false,
    );
    painter.text(
        pos2(
            lumen.left() + theme::space::L + theme::space::XS,
            lumen.center().y,
        ),
        Align2::LEFT_CENTER,
        "Lumen",
        egui::FontId::proportional(theme::size::BODY),
        p.text,
    );
    painter.text(
        pos2(lumen.right() - theme::space::XS, lumen.center().y),
        Align2::RIGHT_CENTER,
        ms(snap.lumen),
        mono(theme::size::MONO_SMALL),
        p.text,
    );
    if snap.plugins.is_empty() {
        return (None, 0.0);
    }
    let mut hovered = None;
    let tallest = room.clamp(2.0 * row_height, theme::size::BUDGET_LIST);
    let shown = egui::ScrollArea::vertical()
        .id_salt("budget plugins")
        .max_height(tallest)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, plugin) in snap.plugins.iter().enumerate() {
                let (row, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), row_height), Sense::hover());
                let painter = ui.painter_at(row);
                if response.hovered() {
                    hovered = Some(i);
                    painter.rect_filled(row, f32::from(theme::radius::SMALL), p.ember);
                }
                let cost = format!("{} + {}", ms(plugin.work), ms(plugin.trip()));
                let cost_font = mono(theme::size::MONO_SMALL);
                let cost_width = painter
                    .layout_no_wrap(cost.clone(), cost_font.clone(), p.text)
                    .size()
                    .x;
                let right = row.right() - theme::space::XS;
                painter.text(
                    pos2(right, row.center().y),
                    Align2::RIGHT_CENTER,
                    &cost,
                    cost_font,
                    p.text,
                );
                let mut text_right = right - cost_width - theme::space::S;
                if plugin.missed > 0 {
                    let mark = painter.text(
                        pos2(text_right, row.center().y),
                        Align2::RIGHT_CENTER,
                        format!("▲ {}", plugin.missed),
                        egui::FontId::proportional(theme::size::CAPTION),
                        p.warning,
                    );
                    text_right = mark.left() - theme::space::S;
                }
                let colour = if i < OWN { shade(i) } else { shade(OWN) };
                swatch(
                    &painter,
                    pos2(row.left() + theme::space::XS, row.center().y),
                    colour,
                    false,
                );
                let left = row.left() + theme::space::L + theme::space::XS;
                let room = (text_right - left).max(0.0);
                let name_font = egui::FontId::proportional(theme::size::BODY);
                let place_font = egui::FontId::proportional(theme::size::CAPTION);
                let name = widgets::fit(&painter, &plugin.name, name_font.clone(), room);
                let place = widgets::fit(&painter, &plugin.place, place_font.clone(), room);
                let middle = row.center().y;
                painter.text(
                    pos2(left, middle),
                    Align2::LEFT_BOTTOM,
                    name,
                    name_font,
                    p.text,
                );
                painter.text(
                    pos2(left, middle),
                    Align2::LEFT_TOP,
                    place,
                    place_font,
                    p.text_hint,
                );
                let missed = if plugin.missed > 0 {
                    format!(", missed {} blocks", plugin.missed)
                } else {
                    String::new()
                };
                let label = format!(
                    "{}, {}: work {} ms, waiting on the trip {} ms{missed}",
                    plugin.name,
                    plugin.place,
                    ms(plugin.work),
                    ms(plugin.trip())
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label)
                });
            }
        });
    (hovered, shown.inner_rect.height())
}
