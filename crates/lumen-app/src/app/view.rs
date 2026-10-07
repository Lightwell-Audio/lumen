//! The main window's content: the transport bar, the arrangement with its
//! ruler, playhead and drop overlay, and the device banner.

use std::path::PathBuf;

use eframe::egui::{self, RichText, Sense, Stroke, pos2, vec2};
use lumen_core::product::PRODUCT_NAME;
use lumen_core::time::Ticks;

use crate::{App, arrangement, audio, bench, canvas, lanes, media, motion, ruler, theme, trace};

/// The height of the ruler above the lanes.
const RULER_HEIGHT: f32 = 22.0;

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
                self.left_beat = view.left_beat;
                self.px_per_beat = view.px_per_beat;
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

        // `contains_pointer`, not `hovered`: the ruler's own click area sits on
        // top, and scrolling and zooming must work over it too. The wheel
        // scrolls the tracks; Shift+wheel or a sideways trackpad swipe
        // scrolls the timeline; Ctrl+wheel (or a pinch) zooms.
        if response.contains_pointer() {
            // egui turns Ctrl+wheel (and trackpad pinch) into a zoom factor
            // and reports no scroll for it, so zoom must read `zoom_delta`.
            let (delta, zoom, pointer) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            if zoom != 1.0 {
                let anchor = pointer.map_or(canvas.center().x, |p| p.x) - canvas.left();
                let beat = self.left_beat + anchor / self.px_per_beat;
                self.px_per_beat = (self.px_per_beat * zoom).clamp(2.0, 400.0);
                self.left_beat = beat - anchor / self.px_per_beat;
            } else {
                self.left_beat -= delta.x / self.px_per_beat;
                if delta.y != 0.0 {
                    let content = self.arrangement.height_of_content(
                        ui.ctx(),
                        self.document.project(),
                        self.lanes.pending().len(),
                    );
                    self.arrangement.scroll(delta.y, content, canvas.height());
                }
            }
            self.left_beat = self.left_beat.max(0.0);
        }
        ui.painter().rect_filled(corner, 0.0, theme::p().raised);
        self.arrangement
            .headers(ui, headers, &mut self.document, &self.lanes);

        let painter = ui.painter_at(full);
        painter.rect_filled(full, 0.0, theme::p().canvas);
        let meter = self.transport.timeline.meter.meter_at(Ticks::ZERO);
        painter.add(canvas::grid_shape(
            canvas,
            ui.ctx().pixels_per_point(),
            self.left_beat,
            self.px_per_beat,
            canvas::GridMeter {
                bar: meter.bar().as_quarters() as f32,
                beat: meter.beat().as_quarters() as f32,
                colors: [theme::p().grid_bar, theme::p().grid_beat],
            },
        ));
        let x_of = |quarters: f32| canvas.left() + (quarters - self.left_beat) * self.px_per_beat;
        self.arrangement.lanes(
            ui.ctx(),
            &painter.with_clip_rect(canvas),
            canvas,
            self.document.project(),
            &mut self.lanes,
            x_of,
        );
        // A hover card with the file's details.
        let hover = ui.interact(canvas, ui.id().with("clips"), Sense::hover());
        if let Some(pointer) = hover.hover_pos()
            && let Some(item) = self.arrangement.item_at(
                ui.ctx(),
                pointer,
                canvas,
                self.document.project(),
                &self.lanes,
                x_of,
            )
        {
            hover.on_hover_ui_at_pointer(|ui| lanes::file_card(ui, item));
        }

        // Dropped files land under the pointer (snapped) when the system
        // reports it, otherwise at the playhead.
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| canvas.contains(*p));
        self.drop_target = match pointer {
            Some(p) => ruler::snap(
                &self.transport.timeline.meter,
                ruler::ticks_at_x(p.x, canvas.left(), self.left_beat, self.px_per_beat),
                self.px_per_beat,
            ),
            None => self.transport.state().ticks,
        };

        // Ruler: loop range, then bar numbers thinned so labels never collide.
        painter.rect_filled(ruler, 0.0, theme::p().raised);
        let looped = self.transport.loop_region;
        if looped.enabled {
            let span = egui::Rect::from_x_y_ranges(
                x_of(looped.start.as_quarters() as f32)..=x_of(looped.end.as_quarters() as f32),
                ruler.y_range(),
            );
            painter.rect_filled(
                span.intersect(ruler),
                0.0,
                theme::p().accent.gamma_multiply(0.18),
            );
        }
        let bar_quarters = meter.bar().as_quarters() as f32;
        let mut step = 1;
        while step as f32 * bar_quarters * self.px_per_beat < 40.0 {
            step *= 2;
        }
        let first_bar = (self.left_beat / bar_quarters).floor() as i64;
        let mut bar = first_bar - first_bar.rem_euclid(step);
        loop {
            let start = self.transport.timeline.meter.bar_start(bar);
            let x = x_of(start.as_quarters() as f32);
            if x > full.right() || bar >= canvas::GRID_BARS as i64 {
                break;
            }
            if x >= full.left() {
                painter.text(
                    pos2(x + 4.0, ruler.center().y),
                    egui::Align2::LEFT_CENTER,
                    (bar + 1).to_string(),
                    egui::FontId::monospace(theme::size::CAPTION),
                    theme::p().text_secondary,
                );
            }
            bar += step;
        }

        // Click-to-locate: the ruler moves the playhead to the nearest line of
        // the zoom-adaptive grid (Alt bypasses snapping). A ghost line shows
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
            let raw = ruler::ticks_at_x(pointer.x, canvas.left(), self.left_beat, self.px_per_beat);
            let target = if free {
                raw
            } else {
                ruler::snap(&self.transport.timeline.meter, raw, self.px_per_beat)
            };
            let gx = x_of(target.as_quarters() as f32);
            painter.line_segment(
                [pos2(gx, ruler.top()), pos2(gx, full.bottom())],
                Stroke::new(1.0, theme::p().accent.gamma_multiply(0.5)),
            );
            if ruler_response.clicked() {
                trace::log(|| format!("ruler: locate {target:?} (snapped: {})", !free));
                self.transport.locate(target);
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
        let mx = x_of(marker);
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
        let x = x_of(head);
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
