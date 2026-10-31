//! The arrangement's rulers, in the owner's layout A (M3 slice 5b): a
//! strip for locators and the loop brace above the bar ruler,
//! and a time ruler along the bottom in a chosen format (minutes and
//! seconds, samples or timecode).
//!
//! The bar ruler locates on click (snapped; Alt places freely) and zooms
//! and scrolls when dragged (`App::drag_ruler`).

use eframe::egui::{self, Align2, CursorIcon, FontId, Rect, Sense, Stroke, pos2};
use lumen_core::time::{PPQ, Ticks};

use super::grid::draw_snap_guide;
use super::info::{Held, Info, Mouse};
use crate::commands::Command;
use crate::timefmt::{self, TimeFormat};
use crate::timeview::TimeView;
use crate::{App, motion, ruler, theme, trace, widgets};

/// The strip for locators, scrubbing and the loop brace.
pub(crate) const STRIP_HEIGHT: f32 = 24.0;
/// The loop brace's band, along the strip's bottom; scrubbing and the
/// locator flags take the rest.
pub(crate) const BRACE_BAND: f32 = 10.0;
/// The bar ruler.
pub(crate) const RULER_HEIGHT: f32 = 22.0;
/// The time ruler along the bottom.
pub(crate) const TIME_HEIGHT: f32 = 20.0;
/// Bars wider than this (points) get beat labels on the bar ruler.
const BEATS_FROM: f64 = 240.0;
/// Time labels are at least this far apart (points).
const TIME_GAP: f64 = 90.0;

/// Where the rulers and lanes are, and how they are viewed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Layout {
    pub strip: Rect,
    pub ruler: Rect,
    pub canvas: Rect,
    pub time: Rect,
    pub view: TimeView,
}

impl Layout {
    fn x(&self, quarters: f64) -> f32 {
        self.view.x(self.canvas.left(), quarters)
    }

    /// From the bar ruler down to the time ruler: where guides are drawn.
    pub fn guides(&self) -> Rect {
        Rect::from_x_y_ranges(self.canvas.x_range(), self.ruler.top()..=self.time.top())
    }
}

impl App {
    /// The strip above the bar ruler: the loop brace (edited by
    /// `App::loop_brace`), punch in and out, and the locators
    /// (`App::locator_flags`).
    pub(crate) fn locator_strip(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        at: &Layout,
    ) {
        let palette = theme::p();
        painter.rect_filled(at.strip, 0.0, palette.app);
        self.scrub_zone(ui, painter, at);
        let brace_hint = self.loop_brace(ui, painter, at);
        let looped = self.document.project().transport;
        let (a, b) = (
            at.x(looped.loop_start.as_quarters()),
            at.x(looped.loop_end.as_quarters()),
        );
        let brace = Rect::from_x_y_ranges(a..=b, at.strip.bottom() - 7.0..=at.strip.bottom())
            .intersect(at.strip);
        if brace.width() > 0.0 {
            let (fill, edge) = if looped.loop_enabled {
                (palette.accent_muted, palette.accent)
            } else {
                (palette.control, palette.text_hint)
            };
            painter.rect_filled(brace, 0.0, fill);
            painter.line_segment(
                [brace.left_top(), brace.right_top()],
                Stroke::new(2.0, edge),
            );
            for x in [a, b] {
                if (at.strip.left()..=at.strip.right()).contains(&x) {
                    painter.line_segment(
                        [pos2(x, brace.top()), pos2(x, brace.bottom())],
                        Stroke::new(2.0, edge),
                    );
                }
            }
        }
        super::regions::draw_punch(painter, at, &looped);
        self.locator_flags(ui, painter, at);
        if let Some(hint) = brace_hint {
            let info = super::regions::draw_brace_hint(painter, at, &looped, hint);
            self.set_info(info);
        }
    }

    /// The bar ruler: labels, click to locate, drag to zoom and scroll.
    pub(crate) fn bar_ruler(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, at: &Layout) {
        painter.rect_filled(at.ruler, 0.0, theme::p().raised);
        self.bar_labels(painter, at);

        // Click to locate (snapped; Alt places freely). Drag down or up to
        // zoom, sideways to scroll.
        let response = ui
            .interact(at.ruler, ui.id().with("ruler"), Sense::click_and_drag())
            .on_hover_text(
                "Click to move the playhead (hold Alt to place it freely). \
                 Drag down or up to zoom, sideways to scroll",
            );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Ruler: click to move the playhead",
            )
        });
        widgets::Menu::context(&response).show(|ui| self.view_menu(ui));
        if self.drag_ruler(&response, at.canvas) {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            return;
        }
        if let Some(pointer) = response.hover_pos() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
            self.set_info(
                Info::new("Bar ruler")
                    .on(Held::NONE, Mouse::Click, "move the playhead")
                    .on(Held::ALT, Mouse::Click, "off the grid")
                    .on(Held::NONE, Mouse::Drag, "zoom and scroll")
                    .on(Held::NONE, Mouse::RightClick, "view"),
            );
            let free = ui.input(|i| i.modifiers.alt);
            let raw = ruler::ticks_at(at.view.beat_at(at.canvas.left(), pointer.x));
            let target = self.snapped(raw, free);
            draw_snap_guide(painter, at.x(target.as_quarters()), at.guides());
            if response.clicked() {
                trace::log(|| format!("ruler: locate {target:?} (snapped: {})", !free));
                self.transport.locate(target);
                // A click on the ruler pauses Follow.
                self.transport.follow_paused = true;
            }
        }
    }

    /// Bar numbers, thinned so they never collide; zoomed in, beats and
    /// sixteenths too. When no label falls in view (the deepest zooms), the
    /// left edge's position is shown pinned at the left.
    fn bar_labels(&self, painter: &egui::Painter, at: &Layout) {
        let map = &self.transport.timeline.meter;
        let view = at.view;
        let ruler = at.ruler;
        let mut shown = 0;
        let mut label = |x: f32, text: String| {
            if (ruler.left()..=ruler.right()).contains(&x) {
                shown += 1;
                painter.text(
                    pos2(x + 4.0, ruler.center().y),
                    Align2::LEFT_CENTER,
                    text,
                    FontId::monospace(theme::size::CAPTION),
                    theme::p().text_secondary,
                );
            }
        };
        let meter = map.meter_at(ruler::ticks_at(view.left));
        let bar_quarters = meter.bar().as_quarters();
        let sixteenth = Ticks(PPQ / 4);
        if bar_quarters * view.zoom < BEATS_FROM {
            let mut step = 1;
            while step as f64 * bar_quarters * view.zoom < 40.0 {
                step *= 2;
            }
            let first = map.bbt_at(ruler::ticks_at(view.left)).bar;
            let mut bar = first - first.rem_euclid(step);
            loop {
                let x = at.x(map.bar_start(bar).as_quarters());
                if x > ruler.right() {
                    break;
                }
                label(x, (bar + 1).to_string());
                bar += step;
            }
        } else {
            let mut step = ruler::step(meter, view.zoom as f32).max(sixteenth);
            while step.as_quarters() * view.zoom < 64.0 {
                step = Ticks(step.0 * 2);
            }
            let left = ruler::ticks_at(view.left);
            let mut t = Ticks(left.0 - left.0.rem_euclid(step.0));
            loop {
                let x = at.x(t.as_quarters());
                if x > ruler.right() {
                    break;
                }
                label(x, bbt_label(map, t, sixteenth));
                t = t + step;
            }
        }
        if shown == 0 {
            // Too deep for any label: say where the view is.
            let left = map.bbt_at(ruler::ticks_at(view.left));
            painter.text(
                pos2(ruler.left() + 4.0, ruler.center().y),
                Align2::LEFT_CENTER,
                format!(
                    "{}.{}.{} …",
                    left.bar + 1,
                    left.beat + 1,
                    left.ticks.0 / sixteenth.0 + 1
                ),
                FontId::monospace(theme::size::CAPTION),
                theme::p().text_hint,
            );
        }
    }

    /// The time ruler along the bottom, in the chosen format.
    /// Dragging the time ruler selects a time range on the selected tracks,
    /// or on every track when none is selected, so empty bars can be chosen
    /// too (owner, slice 6b; ADR-0049 keeps the selection). A click moves the
    /// playhead, as on the bar ruler. Both snap; Alt places freely.
    pub(crate) fn time_ruler_input(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        at: &Layout,
    ) {
        let id = ui.id().with("time ruler");
        let response = ui
            .interact(at.time, id, Sense::click_and_drag())
            .on_hover_text(
                "Drag to select time on the selected tracks, or every track; click to move the playhead",
            );
        widgets::Menu::context(&response).show(|ui| self.time_format_menu(ui));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Time ruler: drag to select time on the selected tracks, or every track",
            )
        });
        let Some(pointer) = response.interact_pointer_pos().or(response.hover_pos()) else {
            return;
        };
        let free = ui.input(|i| i.modifiers.alt);
        let ticks = |x: f32| ruler::ticks_at(at.view.beat_at(at.canvas.left(), x)).max(Ticks::ZERO);
        let target = self.snapped(ticks(pointer.x), free);
        ui.ctx().set_cursor_icon(CursorIcon::Text);
        if response.drag_started()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            let start = self.snapped(ticks(origin.x), free);
            ui.data_mut(|d| d.insert_temp(id, start));
        }
        if response.dragged()
            && let Some(start) = ui.data(|d| d.get_temp::<Ticks>(id))
        {
            // The selected tracks keep their selection and get the range;
            // with none selected, every track does (ADR-0049).
            let project = self.document.project();
            let chosen: Vec<lumen_project::model::Id> = if self.arrangement.selected.is_empty() {
                project.tracks.iter().map(|t| t.id).collect()
            } else {
                self.arrangement.selected.clone()
            };
            self.arrangement.range =
                crate::arrangement::select::TimeRange::over(project, start, target, &chosen);
            self.arrangement.clips.clear();
            self.locators.selected = None;
        }
        if response.drag_stopped() {
            ui.data_mut(|d| d.remove::<Ticks>(id));
        }
        draw_snap_guide(painter, at.x(target.as_quarters()), at.guides());
        if response.clicked() {
            self.transport.locate(target);
            self.transport.follow_paused = true;
        }
        if !response.dragged() {
            self.set_info(
                Info::new("Time ruler")
                    .on(
                        Held::NONE,
                        Mouse::Drag,
                        "select time on the selected tracks, or all",
                    )
                    .on(Held::NONE, Mouse::Click, "move the playhead")
                    .on(Held::NONE, Mouse::RightClick, "time format"),
            );
        }
    }

    pub(crate) fn time_ruler(&mut self, painter: &egui::Painter, at: &Layout) {
        let palette = theme::p();
        let rect = at.time;
        painter.rect_filled(rect, 0.0, palette.raised);
        // The selected time range shows on the ruler too.
        if let Some(range) = &self.arrangement.range {
            let span = Rect::from_x_y_ranges(
                at.x(range.start.as_quarters())..=at.x(range.end.as_quarters()),
                rect.y_range(),
            )
            .intersect(rect);
            painter.rect_filled(span, 0.0, palette.accent.gamma_multiply(0.22));
        }
        painter.line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, palette.border),
        );
        let tempo = &self.transport.timeline.tempo;
        let view = at.view;
        let beat = |x: f32| ruler::ticks_at(view.beat_at(at.canvas.left(), x));
        let (from, to) = (
            tempo.seconds_at(beat(rect.left())),
            tempo.seconds_at(beat(rect.right())),
        );
        let points_per_second = f64::from(rect.width()) / (to - from).max(1e-12);
        let rate = self.transport.state().sample_rate;
        for mark in timefmt::marks(
            self.transport.time_format,
            from,
            to,
            points_per_second,
            rate,
            TIME_GAP,
        ) {
            let x = at.x(tempo.ticks_at_seconds(mark.seconds).as_quarters());
            if !(rect.left()..=rect.right()).contains(&x) {
                continue;
            }
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.top() + 5.0)],
                Stroke::new(1.0, palette.text_hint),
            );
            painter.text(
                pos2(x + 4.0, rect.center().y + 1.0),
                Align2::LEFT_CENTER,
                mark.text,
                FontId::monospace(theme::size::CAPTION),
                palette.text_hint,
            );
        }
    }

    /// The time ruler's corner: its format, and a menu to change it.
    pub(crate) fn time_format_button(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let palette = theme::p();
        ui.painter().rect_filled(rect, 0.0, palette.raised);
        ui.painter().line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, palette.border),
        );
        let format = self.transport.time_format;
        let response = ui
            .interact(rect, ui.id().with("time format"), Sense::click())
            .on_hover_text("The time ruler's format: minutes and seconds, samples or timecode");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Time format: {}", format.name()),
            )
        });
        let hover = motion::hover(ui.ctx(), response.id.with("hover"), response.hovered());
        if hover > 0.0 {
            ui.painter().rect_filled(
                rect.shrink(2.0),
                theme::radius::SMALL,
                palette.control_hover.gamma_multiply(hover),
            );
        }
        ui.painter().text(
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            format.short(),
            FontId::proportional(theme::size::CAPTION),
            palette.text_secondary,
        );
        crate::widgets::draw_arrow(
            ui.painter(),
            pos2(rect.right() - 12.0, rect.center().y),
            1.0,
            palette.text_secondary,
        );
        widgets::Menu::below(&response).show(|ui| self.time_format_menu(ui));
    }

    /// The time ruler's formats, from its corner or a right-click on it.
    fn time_format_menu(&mut self, ui: &mut egui::Ui) {
        let format = self.transport.time_format;
        for choice in TimeFormat::ALL {
            if widgets::menu_toggle(ui, choice.name(), None, format == choice).clicked() {
                self.transport.time_format = choice;
                ui.close();
            }
        }
        ui.separator();
        self.menu_command(ui, Command::NextTimeFormat, Command::NextTimeFormat.name());
    }
}

/// "5", "5.2" or "5.2.3": bar, beat and sixteenth, as far as needed.
fn bbt_label(map: &lumen_core::meter::MeterMap, t: Ticks, sixteenth: Ticks) -> String {
    let bbt = map.bbt_at(t);
    if bbt.ticks == Ticks::ZERO && bbt.beat == 0 {
        (bbt.bar + 1).to_string()
    } else if bbt.ticks == Ticks::ZERO {
        format!("{}.{}", bbt.bar + 1, bbt.beat + 1)
    } else {
        format!(
            "{}.{}.{}",
            bbt.bar + 1,
            bbt.beat + 1,
            bbt.ticks.0 / sixteenth.0 + 1
        )
    }
}
