//! The Mixer tab (M4 slice 5, ADR-0040): a strip per track in the
//! arrangement's order, skipping tracks hidden in folded groups (one fold
//! for both views, the owner's B4), the returns after a gap, and the master
//! pinned at the right with the loudness column (the owner's L2) beside it.
//! The strips scroll sideways; only those in view are drawn.

use eframe::egui::{
    self, Align2, FontId, Rangef, Rect, Sense, Shape, Stroke, Ui, UiBuilder, pos2, vec2,
};
use lumen_project::groups;
use lumen_project::model::{Id, TrackKind};

use super::Arrangement;
use super::reorder::{Mark, Spot};
use super::strip::{self, Sections, StripLook};
use crate::commands::Keymap;
use crate::document::Document;
use crate::meters::{FLOOR_DB, HISTORY};
use crate::{fonts, theme, widgets};

/// The gap between the tracks and the returns, and before the master.
const RETURNS_GAP: f32 = 10.0;
/// How fast a strip drag near an edge scrolls the strips, at the edge.
const DRAG_SCROLL: f32 = 900.0;
/// The loudness column's width (the owner's L2).
const LOUDNESS: f32 = 108.0;
/// The loudness bar's range, in LUFS, and the band it marks (the common
/// streaming target, −14 LUFS ± 1).
const LUFS_FLOOR: f64 = -42.0;
const TARGET: (f64, f64) = (-15.0, -13.0);

/// The shortest the Mixer tab's content can be: a strip with everything
/// folded away.
pub(crate) fn min_height() -> f32 {
    Sections::min_height()
}

impl Arrangement {
    /// The Mixer tab's content in `ui`'s whole area.
    pub(crate) fn mixer_panel(&mut self, ui: &mut Ui, doc: &mut Document, keymap: &Keymap) {
        let palette = *theme::p();
        let area = ui.max_rect();
        ui.painter().rect_filled(area, 0.0, palette.canvas);
        let heard = lumen_project::solo::hearing(doc.project());
        let look = StripLook {
            sections: Sections::fitting(area.height()),
            heard: &heard,
            keymap,
        };

        // Pinned at the right: the master and the loudness column.
        let loudness =
            Rect::from_x_y_ranges(area.right() - LOUDNESS..=area.right(), area.y_range());
        let master = Rect::from_x_y_ranges(
            loudness.left() - 1.0 - strip::WIDTH..=loudness.left() - 1.0,
            area.y_range(),
        );
        let strips =
            Rect::from_x_y_ranges(area.left()..=master.left() - RETURNS_GAP, area.y_range());
        ui.painter().vline(
            master.left() - RETURNS_GAP / 2.0,
            area.y_range(),
            Stroke::new(1.0, palette.border_strong),
        );

        // The strips in order: tracks shown in the arrangement, then the
        // returns.
        let project = doc.project();
        let shown: Vec<(usize, bool)> = project
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind != TrackKind::Return && !self.hidden(project, t.id))
            .map(|(i, _)| (i, false))
            .chain(
                project
                    .tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.kind == TrackKind::Return)
                    .map(|(i, _)| (i, true)),
            )
            .collect();
        let mut x = 0.0;
        let mut placed = Vec::with_capacity(shown.len());
        let mut after_tracks = false;
        for &(index, ret) in &shown {
            if ret && !after_tracks {
                x += RETURNS_GAP;
                after_tracks = true;
            }
            placed.push((index, x));
            x += strip::WIDTH;
        }
        let width = x;

        // The empty mixer: right-click for the project's pan law and the
        // clip lights (strips, drawn over it, have their own menus).
        let background = ui.interact(strips, ui.id().with("mixer background"), Sense::click());
        background
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Mixer"));
        if background.hovered() {
            use crate::app::info::{Held, Info, Mouse};
            self.info = Some(Info::new("Mixer").on(
                Held::NONE,
                Mouse::RightClick,
                "pan law and clip lights",
            ));
        }
        background.context_menu(|ui| self.mixer_menu(ui, doc, keymap));

        // Where every strip is on screen, for a drag's drop place.
        let mut on_screen: Vec<(usize, Rangef)> = Vec::with_capacity(placed.len());
        ui.scope_builder(UiBuilder::new().max_rect(strips), |ui| {
            if placed.is_empty() {
                ui.painter().text(
                    strips.left_center() + vec2(16.0, 0.0),
                    Align2::LEFT_CENTER,
                    "No tracks to mix yet: add one with + Add track or Ctrl+T",
                    FontId::proportional(theme::size::BODY),
                    palette.text_hint,
                );
                return;
            }
            let mut area = egui::ScrollArea::horizontal()
                .id_salt("mixer strips")
                .auto_shrink([false, false]);
            if self.mixer_push != 0.0 {
                // A strip drag near an edge scrolls, faster the closer.
                let dt = ui.input(|i| i.stable_dt.min(0.1));
                let to = (self.mixer_offset + self.mixer_push * DRAG_SCROLL * dt)
                    .clamp(0.0, (width - strips.width()).max(0.0));
                area = area.horizontal_scroll_offset(to);
                ui.ctx().request_repaint();
            }
            let output = area.show_viewport(ui, |ui, viewport| {
                ui.set_min_size(vec2(width, strips.height()));
                let origin = ui.min_rect().min;
                for &(index, x) in &placed {
                    let span = Rangef::new(x, x + strip::WIDTH);
                    on_screen.push((index, Rangef::new(span.min + origin.x, span.max + origin.x)));
                    if span.max < viewport.min.x || span.min > viewport.max.x {
                        continue;
                    }
                    let rect = Rect::from_x_y_ranges(
                        origin.x + span.min..=origin.x + span.max,
                        strips.y_range(),
                    );
                    self.strip(ui, doc, Some(index), rect, &look);
                }
            });
            self.mixer_offset = output.state.offset.x;
        });
        self.follow_strips_drag(ui, doc, &on_screen, strips);
        self.strip(ui, doc, None, master, &look);
        self.loudness_column(ui, loudness);
    }

    /// The mixer's own menu: the project's pan law (the owner's choice in
    /// slice 2: from the mixer and the palette), and clearing clip lights.
    fn mixer_menu(&mut self, ui: &mut Ui, doc: &mut crate::document::Document, keymap: &Keymap) {
        use crate::commands::Command;
        use lumen_core::pan::PanLaw;
        ui.label(
            egui::RichText::new("Pan law (mono tracks)")
                .small()
                .color(theme::p().text_hint),
        );
        let current = doc.project().pan_law;
        for (law, command) in [
            (PanLaw::ConstantPower, Command::PanLawConstantPower),
            (PanLaw::Compromise, Command::PanLawCompromise),
            (PanLaw::Linear, Command::PanLawLinear),
            (PanLaw::Balance, Command::PanLawBalance),
        ] {
            let keys = keymap.keys(command);
            if widgets::menu_toggle(ui, law.name(), keys.as_ref(), current == law).clicked() {
                doc.set_pan_law(law);
                ui.close();
            }
        }
        ui.separator();
        let keys = keymap.keys(Command::ClearClips);
        if widgets::menu_item(
            ui,
            "Clear clip lights",
            keys.as_ref(),
            self.meters.any_clipped(),
        )
        .clicked()
        {
            self.meters.clear_all_clips();
            ui.close();
        }
    }

    /// A strip drag (the owner's B8): where it would land among the strips
    /// of its kind (tracks, or returns), the drop line or the group lit,
    /// and the drop itself.
    fn follow_strips_drag(
        &mut self,
        ui: &mut Ui,
        doc: &mut crate::document::Document,
        on_screen: &[(usize, Rangef)],
        strips: Rect,
    ) {
        let Some((moving, _, _)) = self.strip_drag() else {
            self.mixer_push = 0.0;
            return;
        };
        let moving: Vec<Id> = moving.to_vec();
        let tracks = &doc.project().tracks;
        let is_return = |i: usize| tracks[i].kind == TrackKind::Return;
        let returns = on_screen
            .iter()
            .any(|&(i, _)| moving.contains(&tracks[i].id) && is_return(i));
        let spots: Vec<Spot> = on_screen
            .iter()
            .filter(|&&(i, _)| !moving.contains(&tracks[i].id) && is_return(i) == returns)
            .map(|&(i, span)| Spot {
                id: tracks[i].id,
                top: span.min,
                height: span.span(),
                // Returns are never grouped: none of them takes a drop.
                group: tracks[i].kind == TrackKind::Group,
                parent: tracks[i].parent,
                depth: groups::depth(tracks, tracks[i].id),
            })
            .collect();
        self.mixer_push = self.follow_strip_drag(ui, &spots, strips, doc);
        if let Some((_, Some(at), refused)) = self.strip_drag() {
            let palette = theme::p();
            let accent = if refused {
                palette.text_disabled
            } else {
                palette.accent
            };
            let painter = ui.painter_at(strips);
            match at.mark {
                Mark::Line(x) => {
                    painter.vline(x, strips.y_range(), Stroke::new(2.0, accent));
                }
                Mark::Onto(x, width) => {
                    let rect = Rect::from_x_y_ranges(x..=x + width, strips.y_range());
                    painter.rect_filled(rect, 0.0, accent.gamma_multiply(0.10));
                    painter.rect_stroke(
                        rect.shrink(1.0),
                        0.0,
                        Stroke::new(2.0, accent),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
    }

    /// The master's loudness (the owner's L2): integrated large, a
    /// short-term bar with the momentary line and the −14 LUFS band, the
    /// last minute's short-term history, then momentary, short-term and true
    /// peak. Reset starts integrated loudness and true peak again, as Play
    /// from stop does (B5).
    fn loudness_column(&mut self, ui: &mut Ui, rect: Rect) {
        let palette = *theme::p();
        let reading = self.meters.loudness();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, palette.raised);
        let left = rect.left() + 8.0;
        let mut y = rect.top() + 12.0;
        painter.text(
            pos2(left, y),
            Align2::LEFT_CENTER,
            "Loudness",
            fonts::medium(theme::size::BODY),
            palette.text,
        );
        y += 22.0;
        let lufs = |v: f64| {
            if meters_heard(v) {
                format!("{v:.1}").replace('-', "\u{2212}")
            } else {
                "\u{2014}".into()
            }
        };
        painter.text(
            pos2(left, y),
            Align2::LEFT_CENTER,
            lufs(reading.integrated),
            FontId::monospace(theme::size::READOUT),
            palette.text,
        );
        y += 16.0;
        painter.text(
            pos2(left, y),
            Align2::LEFT_CENTER,
            "LUFS integrated",
            FontId::proportional(theme::size::CAPTION),
            palette.text_hint,
        );
        y += 14.0;

        // The bar and the history fill what the numbers below leave.
        let numbers = 3.0 * 16.0 + 8.0;
        let reset_h = 20.0;
        let bottom = rect.bottom() - 8.0 - reset_h - 6.0 - numbers;
        let ys = Rangef::new(y, bottom.max(y + 20.0));
        let at = |v: f64| {
            let t = ((v - LUFS_FLOOR) / -LUFS_FLOOR).clamp(0.0, 1.0) as f32;
            ys.max - t * ys.span()
        };
        let bar = Rect::from_x_y_ranges(left..=left + 12.0, ys);
        painter.rect_filled(bar, 2.0, palette.well);
        let band = Rect::from_x_y_ranges(bar.x_range().expand(3.0), at(TARGET.1)..=at(TARGET.0));
        painter.rect_filled(band, 0.0, palette.accent.gamma_multiply(0.14));
        if meters_heard(reading.short_term) {
            let top = at(reading.short_term);
            let color = if reading.short_term > -9.0 {
                palette.warning
            } else {
                palette.accent
            };
            painter.rect_filled(
                Rect::from_x_y_ranges(bar.x_range().shrink(2.0), top..=ys.max),
                0.0,
                color,
            );
        }
        if meters_heard(reading.momentary) {
            painter.hline(
                bar.x_range(),
                at(reading.momentary),
                Stroke::new(2.0, palette.text),
            );
        }
        for mark in [-6.0, -14.0, -23.0, -36.0] {
            painter.text(
                pos2(bar.right() + 5.0, at(mark)),
                Align2::LEFT_CENTER,
                format!("{mark:.0}").replace('-', "\u{2212}"),
                FontId::monospace(theme::size::SCALE),
                if mark == -14.0 {
                    palette.accent
                } else {
                    palette.text_hint
                },
            );
        }
        // The last minute of short-term loudness.
        let graph = Rect::from_x_y_ranges(bar.right() + 30.0..=rect.right() - 6.0, ys);
        painter.rect_stroke(
            graph,
            0.0,
            Stroke::new(1.0, palette.border_strong),
            egui::StrokeKind::Inside,
        );
        let history: Vec<f64> = self.meters.history().collect();
        let step = graph.width() / HISTORY as f32;
        let points: Vec<egui::Pos2> = history
            .iter()
            .enumerate()
            .filter(|(_, v)| meters_heard(**v))
            .map(|(i, v)| {
                let x = graph.right() - (history.len() - 1 - i) as f32 * step;
                pos2(x, at(*v))
            })
            .collect();
        if points.len() > 1 {
            painter.with_clip_rect(graph).add(Shape::line(
                points,
                Stroke::new(1.0, palette.accent.gamma_multiply(0.8)),
            ));
        }

        // Momentary, short-term and true peak.
        let mut y = ys.max + 14.0;
        let true_peak = if reading.true_peak > FLOOR_DB {
            widgets::readout_db(reading.true_peak)
        } else {
            "\u{2014}".into()
        };
        for (label, value, warn) in [
            ("M", lufs(reading.momentary), false),
            ("S", lufs(reading.short_term), false),
            ("TP", true_peak, reading.true_peak > -1.0),
        ] {
            painter.text(
                pos2(left, y),
                Align2::LEFT_CENTER,
                label,
                FontId::monospace(theme::size::SCALE),
                palette.text_hint,
            );
            painter.text(
                pos2(rect.right() - 8.0, y),
                Align2::RIGHT_CENTER,
                value,
                FontId::monospace(theme::size::CAPTION),
                if warn {
                    palette.warning
                } else {
                    palette.text_secondary
                },
            );
            y += 16.0;
        }

        // What a screen reader hears, and Reset.
        let readout = ui.interact(
            Rect::from_x_y_ranges(rect.x_range(), rect.top()..=ys.max),
            ui.id().with("loudness readout"),
            Sense::hover(),
        );
        let said = format!(
            "Loudness: integrated {} LUFS, short-term {} LUFS, momentary {} LUFS",
            lufs(reading.integrated),
            lufs(reading.short_term),
            lufs(reading.momentary)
        );
        readout.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &said));
        let reset = Rect::from_x_y_ranges(
            rect.x_range().shrink(8.0),
            rect.bottom() - 8.0 - reset_h..=rect.bottom() - 8.0,
        );
        let response = ui
            .put(reset, egui::Button::new("\u{21BA} Reset"))
            .on_hover_text(
                "Start integrated loudness and true peak again (Play from stop does too)",
            );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Reset integrated loudness")
        });
        if response.clicked() {
            self.meters.reset_loudness();
        }
    }
}

/// Whether a loudness value is something to show (not silence).
fn meters_heard(lufs: f64) -> bool {
    lufs.is_finite() && lufs > LUFS_FLOOR - 30.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_shows_a_dash() {
        assert!(!meters_heard(f64::NEG_INFINITY));
        assert!(!meters_heard(-120.0));
        assert!(meters_heard(-23.0));
    }
}
