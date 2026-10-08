//! The main window's content: the transport bar, the arrangement with its
//! ruler, playhead and drop overlay, and the device banner.

use std::path::PathBuf;

use eframe::egui::{self, RichText, Sense, Stroke, pos2, vec2};
use lumen_core::product::PRODUCT_NAME;
use lumen_core::time::{PPQ, Ticks};

use super::grid::draw_snap_guide;
use crate::{App, arrangement, audio, bench, canvas, lanes, media, motion, ruler, theme, trace};

/// The height of the ruler above the lanes.
const RULER_HEIGHT: f32 = 22.0;
/// Bars wider than this (points) get beat labels on the ruler.
const RULER_BEATS_FROM: f64 = 240.0;

/// A glided position, or the exact one once the glide has settled: the
/// glide runs in f32, which is pixels off at sample-level zoom.
fn settled(glided: f32, exact: f64) -> f64 {
    if (f64::from(glided) - exact).abs() < 1e-3 {
        exact
    } else {
        f64::from(glided)
    }
}

impl App {
    /// `--bench-frames`: puts the view where the benchmark wants it this
    /// frame, or reports and exits when it is done (`crate::bench`).
    pub(crate) fn drive_bench(&mut self, ctx: &egui::Context) {
        let Some(bench) = &mut self.bench else {
            return;
        };
        let now = std::time::Instant::now();
        let step = bench.step(now);
        let idle_left = bench.idle_left(now);
        let out = bench.out().to_path_buf();
        match step {
            bench::Step::Drive(view) => {
                self.nav.set(crate::timeview::TimeView {
                    left: f64::from(view.left_beat),
                    zoom: f64::from(view.px_per_beat),
                });
                let content = self.arrangement.height_of_content(
                    ctx,
                    self.document.project(),
                    self.lanes.pending().len(),
                );
                // The lanes take a little over half the window.
                let visible = ctx.content_rect().height() * 0.6;
                self.arrangement.top = view.top * (content - visible).max(0.0);
                ctx.request_repaint();
            }
            bench::Step::Idle => {
                if let Some(bench) = &mut self.bench {
                    for cause in ctx.repaint_causes() {
                        bench.idle_cause(format!("{}:{} {}", cause.file, cause.line, cause.reason));
                    }
                }
                ctx.request_repaint_after(idle_left);
            }
            bench::Step::Done(report, pass) => {
                println!(
                    "{report}
report: {}",
                    out.display()
                );
                std::process::exit(if pass { 0 } else { 3 });
            }
        }
    }

    pub(crate) fn transport(&mut self, ui: &mut egui::Ui) {
        // The panel toggles are laid out first, from the right, so they keep
        // their place at any width or scale; the transport takes the rest and
        // the status text shortens to fit.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.toggle_value(&mut self.show_detail, "Detail")
                .on_hover_text("Show or hide the detail panel (Shift+Tab)");
            ui.toggle_value(&mut self.show_browser, "Browser")
                .on_hover_text("Show or hide the browser");
            ui.separator();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                // Whatever does not fit is cut off here rather than drawn
                // over the panel toggles.
                ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                self.app_menu(ui);
                ui.separator();
                self.transport.ui(ui, &mut self.document);
            });
        });
    }

    pub(crate) fn arrangement(&mut self, ui: &mut egui::Ui) {
        let whole = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(whole, Sense::hover());
        // Track headers on the left, below the ruler's height; the ruler and
        // lanes take the rest.
        let corner =
            egui::Rect::from_min_size(whole.min, vec2(arrangement::HEADER_WIDTH, RULER_HEIGHT));
        let headers = egui::Rect::from_min_max(
            pos2(whole.left(), corner.bottom()),
            pos2(corner.right(), whole.bottom()),
        );
        let full = egui::Rect::from_min_max(pos2(corner.right(), whole.top()), whole.max);
        let ruler = egui::Rect::from_min_size(full.min, vec2(full.width(), RULER_HEIGHT));
        let canvas = egui::Rect::from_min_max(pos2(full.left(), ruler.bottom()), full.max);

        self.navigate(ui, &response, canvas);
        self.follow(canvas);
        let view = self.nav.view;
        ui.painter().rect_filled(corner, 0.0, theme::p().raised);
        self.grid_button(ui, corner);
        self.arrangement
            .headers(ui, headers, &mut self.document, &self.lanes);

        let painter = ui.painter_at(full);
        painter.rect_filled(full, 0.0, theme::p().canvas);
        let meter = self.transport.timeline.meter.meter_at(Ticks::ZERO);
        painter.add(canvas::grid_shape(
            canvas,
            ui.ctx().pixels_per_point(),
            view,
            &canvas::GridLook {
                grid: self.grid,
                meter: self.transport.timeline.meter.clone(),
                colors: [
                    theme::p().grid_bar,
                    theme::p().grid_beat,
                    theme::p().grid_sub,
                ],
            },
        ));
        let x_of = |quarters: f64| view.x(canvas.left(), quarters);
        self.arrangement.lanes(
            ui.ctx(),
            &painter.with_clip_rect(canvas),
            canvas,
            &self.document,
            &mut self.lanes,
            view,
        );
        // A hover card with the file's details.
        // Right-click the lanes for the grid menu (slice 5a).
        let hover = ui.interact(canvas, ui.id().with("clips"), Sense::click());
        hover.context_menu(|ui| self.grid_menu(ui));
        if let Some(pointer) = hover.hover_pos()
            && let Some(item) = self.arrangement.item_at(
                ui.ctx(),
                pointer,
                canvas,
                self.document.project(),
                &self.lanes,
                view,
            )
        {
            hover.on_hover_ui_at_pointer(|ui| lanes::file_card(ui, item));
        }

        // Dropped files land under the pointer (snapped; Alt places them
        // freely) when the system reports it, otherwise at the playhead. A
        // snap guide shows where while files are dragged over.
        let (pointer, free, dragging_files) = ui.input(|i| {
            (
                i.pointer.hover_pos().filter(|p| canvas.contains(*p)),
                i.modifiers.alt,
                !i.raw.hovered_files.is_empty(),
            )
        });
        self.drop_target = match pointer {
            Some(p) => self.snapped(ruler::ticks_at(view.beat_at(canvas.left(), p.x)), free),
            None => self.transport.state().ticks,
        };
        if dragging_files && pointer.is_some() {
            draw_snap_guide(&painter, x_of(self.drop_target.as_quarters()), full);
        }

        // Ruler: loop range, then bar numbers thinned so labels never collide.
        painter.rect_filled(ruler, 0.0, theme::p().raised);
        let looped = self.transport.loop_region;
        if looped.enabled {
            let span = egui::Rect::from_x_y_ranges(
                x_of(looped.start.as_quarters())..=x_of(looped.end.as_quarters()),
                ruler.y_range(),
            );
            painter.rect_filled(
                span.intersect(ruler),
                0.0,
                theme::p().accent.gamma_multiply(0.18),
            );
        }
        let label = |x: f32, text: String| {
            if (full.left()..=full.right()).contains(&x) {
                painter.text(
                    pos2(x + 4.0, ruler.center().y),
                    egui::Align2::LEFT_CENTER,
                    text,
                    egui::FontId::monospace(theme::size::CAPTION),
                    theme::p().text_secondary,
                );
            }
        };
        let bar_quarters = meter.bar().as_quarters();
        if bar_quarters * view.zoom < RULER_BEATS_FROM {
            // Bar numbers, thinned so labels never collide.
            let mut step = 1;
            while step as f64 * bar_quarters * view.zoom < 40.0 {
                step *= 2;
            }
            let first_bar = (view.left / bar_quarters).floor() as i64;
            let mut bar = first_bar - first_bar.rem_euclid(step);
            loop {
                let start = self.transport.timeline.meter.bar_start(bar);
                let x = x_of(start.as_quarters());
                if x > full.right() {
                    break;
                }
                label(x, (bar + 1).to_string());
                bar += step;
            }
        } else {
            // Zoomed in (slice 4d): label beats and sixteenths too, as
            // bar.beat.sixteenth, thinned to one label per 64 points.
            // Other ruler formats arrive with slice 5.
            let sixteenth = Ticks(PPQ / 4);
            let mut step = ruler::step(meter, view.zoom as f32).max(sixteenth);
            while step.as_quarters() * view.zoom < 64.0 {
                step = Ticks(step.0 * 2);
            }
            let left = Ticks((view.left * PPQ as f64) as i64).max(Ticks::ZERO);
            let mut t = Ticks(left.0 - left.0.rem_euclid(step.0));
            loop {
                let x = x_of(t.as_quarters());
                if x > full.right() {
                    break;
                }
                let bbt = self.transport.timeline.meter.bbt_at(t);
                let text = if bbt.ticks == Ticks::ZERO && bbt.beat == 0 {
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
                };
                label(x, text);
                t = t + step;
            }
        }

        // Click-to-locate: the ruler moves the playhead to the nearest line of
        // the editing grid (Alt bypasses snapping). The snap guide shows
        // where a click will land.
        let ruler_response = ui
            .interact(ruler, ui.id().with("ruler"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Click to move the playhead. Hold Alt to place it freely");
        ruler_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Ruler: click to move the playhead",
            )
        });
        if let Some(pointer) = ruler_response.hover_pos() {
            let free = ui.input(|i| i.modifiers.alt);
            let raw = ruler::ticks_at(view.beat_at(canvas.left(), pointer.x));
            let target = self.snapped(raw, free);
            draw_snap_guide(&painter, x_of(target.as_quarters()), full);
            if ruler_response.clicked() {
                trace::log(|| format!("ruler: locate {target:?} (snapped: {})", !free));
                self.transport.locate(target);
                // As in Ableton, a click on the ruler pauses Follow.
                self.transport.follow_paused = true;
            }
        }

        // The start marker: where Play starts. A neutral arrow on the ruler
        // only, at its bottom edge, so it stays visible when the playhead
        // (blue, at the top edge) is at the same place.
        // Both glide to a new place while stopped (motion: GLIDE) and follow
        // the audio exactly while playing.
        let state = self.transport.state();
        let ctx = ui.ctx().clone();
        let marker = motion::glide(
            &ctx,
            egui::Id::new("start marker"),
            state.marker.as_quarters() as f32,
            !state.playing,
        );
        let head = motion::glide(
            &ctx,
            egui::Id::new("playhead"),
            state.ticks.as_quarters() as f32,
            !state.playing,
        );
        let mx = x_of(settled(marker, state.marker.as_quarters()));
        if (full.left()..=full.right()).contains(&mx) {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(mx - 6.0, ruler.bottom() - 9.0),
                    pos2(mx + 6.0, ruler.bottom() - 9.0),
                    pos2(mx, ruler.bottom() - 1.0),
                ],
                theme::p().start_marker,
                Stroke::new(1.0, theme::p().canvas),
            ));
        }

        // Playhead.
        let x = x_of(settled(head, state.ticks.as_quarters()));
        if (full.left()..=full.right()).contains(&x) {
            painter.line_segment(
                [pos2(x, ruler.top()), pos2(x, full.bottom())],
                Stroke::new(1.0, theme::p().accent),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(x - 5.0, ruler.top()),
                    pos2(x + 5.0, ruler.top()),
                    pos2(x, ruler.top() + 7.0),
                ],
                theme::p().accent,
                Stroke::NONE,
            ));
        }

        // While files are dragged over the window, say which will be added.
        let hovered: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .hovered_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !hovered.is_empty() {
            let supported = hovered.iter().filter(|p| media::is_audio_file(p)).count();
            let color = if supported > 0 {
                theme::p().accent
            } else {
                theme::p().warning
            };
            painter.rect_stroke(
                canvas.shrink(2.0),
                4.0,
                Stroke::new(2.0, color),
                egui::StrokeKind::Inside,
            );
            let mut lines: Vec<String> = hovered
                .iter()
                .take(8)
                .map(|p| {
                    let name = p
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    match media::format_name(p) {
                        Some(format) => format!("✓  {name}  ({format})"),
                        None => format!("✕  {name}  (not an audio file {PRODUCT_NAME} reads)"),
                    }
                })
                .collect();
            if hovered.len() > 8 {
                lines.push(format!("…and {} more", hovered.len() - 8));
            }
            let headline = match (supported, hovered.len()) {
                (0, _) => "Nothing here can be added".to_owned(),
                (n, total) if n == total => {
                    format!(
                        "Drop to add {n} audio file{}",
                        if n == 1 { "" } else { "s" }
                    )
                }
                (n, total) => format!("Drop to add {n} of {total} files"),
            };
            painter.text(
                canvas.center(),
                egui::Align2::CENTER_CENTER,
                format!("{headline}\n\n{}", lines.join("\n")),
                egui::FontId::proportional(theme::size::TITLE),
                color,
            );
        } else if self.lanes.is_empty(self.document.project()) {
            painter.text(
                canvas.center(),
                egui::Align2::CENTER_CENTER,
                "Drop audio files here\nWAV, AIFF, FLAC, MP3 or Ogg Vorbis",
                egui::FontId::proportional(theme::size::BODY),
                theme::p().text_hint,
            );
        }
        self.lanes
            .panel(ui.ctx(), pos2(full.left() + 8.0, full.bottom() - 8.0));
        self.toast(ui.ctx(), pos2(full.right() - 12.0, canvas.top() + 10.0));
    }

    /// A banner under the transport when the audio device needs attention:
    /// lost (reconnecting), busy in another application, or failed. Each
    /// offers a way forward.
    pub(crate) fn device_banner(&mut self, ui: &mut egui::Ui) {
        let audio = &self.transport.audio;
        let current = match &audio.status {
            audio::Status::Lost(_) => Some((
                format!(
                    "{} was disconnected. {} reconnects when it is back.",
                    audio.device,
                    lumen_core::product::PRODUCT_NAME
                ),
                false,
            )),
            audio::Status::Off(_) if audio.is_busy() => Some((
                format!(
                    "{} is in use by another application. Close it there, then try again.",
                    audio.device
                ),
                true,
            )),
            audio::Status::Off(why)
                if !audio.is_on() && why != "Audio is off" && !why.is_empty() =>
            {
                Some((format!("Audio could not start: {why}"), true))
            }
            _ => None,
        };
        // The banner slides in and out (motion: BASE); while it slides
        // away it keeps showing its last message, with its buttons off.
        let mut open = current.is_some();
        if let Some(current) = current {
            self.banner = Some(current);
        }
        let Some((message, retry)) = self.banner.clone() else {
            return;
        };
        let live = open;
        let shown = egui::Panel::top("device banner").show_collapsible(ui, &mut open, |ui| {
            ui.horizontal(|ui| {
                let warning = theme::p().warning;
                ui.label(RichText::new("⚠").color(warning));
                ui.label(RichText::new(&message).color(warning));
                ui.add_enabled_ui(live, |ui| {
                    if retry
                        && self.transport.audio.can_switch()
                        && ui.button("Try again").clicked()
                    {
                        self.transport.audio.set_on(true);
                    }
                    if ui.button("Audio settings…").clicked() {
                        self.transport.settings_window.open = true;
                    }
                });
            });
        });
        if shown.is_none() {
            self.banner = None;
        }
    }
}
