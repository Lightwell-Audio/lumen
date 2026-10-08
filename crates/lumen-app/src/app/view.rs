//! The main window's content: the transport bar, the arrangement with its
//! ruler, playhead and drop overlay, and the device banner.

use std::path::PathBuf;

use eframe::egui::{self, RichText, Sense, Stroke, pos2, vec2};
use lumen_core::product::PRODUCT_NAME;

use super::grid::draw_snap_guide;
use super::rulers;
use crate::arrangement::FileDrop;
use crate::commands::Command;
use crate::document::DropInto;
use crate::icons::{self, Icon};
use crate::{App, arrangement, audio, bench, canvas, lanes, media, motion, ruler, theme};

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
            let keys = self.keymap.text(Command::OpenPalette);
            if icons::button(
                ui,
                Icon::Search,
                "Command palette",
                keys.as_deref(),
                self.palette.is_open(),
                true,
                "",
            )
            .clicked()
            {
                self.toggle_palette();
            }
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

    /// While files are dragged in from the system: where the pointer is
    /// (`App::drag_pointer`), and a frame after frame, so the line or the
    /// lit lane follows it: the system sends no events while it moves.
    pub(crate) fn follow_file_drag(&mut self, ctx: &egui::Context) {
        let (hovered, dropped) = ctx.input(|i| {
            (
                !i.raw.hovered_files.is_empty(),
                !i.raw.dropped_files.is_empty(),
            )
        });
        self.drag_pointer = if hovered || dropped {
            self.system_cursor.or_else(|| {
                self.owner
                    .and_then(|window| window.cursor(ctx.pixels_per_point()))
            })
        } else {
            None
        };
        if hovered {
            ctx.request_repaint();
        }
    }

    pub(crate) fn arrangement(&mut self, ui: &mut egui::Ui) {
        let whole = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(whole, Sense::hover());
        // Layout A (owner, slice 5b): the locator strip and the bar ruler
        // on top, the time ruler at the bottom; track headers on the left
        // between their corners; the lanes in the middle.
        let header = arrangement::HEADER_WIDTH;
        let top = rulers::STRIP_HEIGHT + rulers::RULER_HEIGHT;
        let corner = egui::Rect::from_min_size(whole.min, vec2(header, top));
        let time_corner = egui::Rect::from_min_max(
            pos2(whole.left(), whole.bottom() - rulers::TIME_HEIGHT),
            pos2(whole.left() + header, whole.bottom()),
        );
        let headers = egui::Rect::from_min_max(
            pos2(whole.left(), corner.bottom()),
            pos2(corner.right(), time_corner.top()),
        );
        let full = egui::Rect::from_min_max(pos2(corner.right(), whole.top()), whole.max);
        let strip = egui::Rect::from_min_size(full.min, vec2(full.width(), rulers::STRIP_HEIGHT));
        let ruler = egui::Rect::from_min_size(
            pos2(full.left(), strip.bottom()),
            vec2(full.width(), rulers::RULER_HEIGHT),
        );
        let time = egui::Rect::from_min_max(
            pos2(full.left(), full.bottom() - rulers::TIME_HEIGHT),
            full.max,
        );
        let canvas = egui::Rect::from_min_max(pos2(full.left(), ruler.bottom()), time.right_top());

        self.canvas = canvas;
        self.navigate(ui, &response, canvas);
        self.follow(canvas);
        let view = self.nav.view;
        let palette = theme::p();
        ui.painter().rect_filled(corner, 0.0, palette.raised);
        let strip_corner =
            egui::Rect::from_min_size(corner.min, vec2(header, rulers::STRIP_HEIGHT));
        ui.painter().rect_filled(strip_corner, 0.0, palette.app);
        self.locator_buttons(ui, strip_corner);
        self.grid_button(
            ui,
            egui::Rect::from_min_max(
                pos2(corner.left(), corner.top() + rulers::STRIP_HEIGHT),
                corner.max,
            ),
        );
        self.time_format_button(ui, time_corner);
        self.arrangement
            .headers(ui, headers, &mut self.document, &self.lanes);

        let painter = ui.painter_at(full);
        painter.rect_filled(full, 0.0, palette.canvas);
        painter.add(canvas::grid_shape(
            canvas,
            ui.ctx().pixels_per_point(),
            view,
            &canvas::GridLook {
                grid: self.grid,
                meter: self.transport.timeline.meter.clone(),
                colors: [palette.grid_bar, palette.grid_beat, palette.grid_sub],
            },
        ));
        let at = rulers::Layout {
            strip,
            ruler,
            canvas,
            time,
            view,
        };
        let x_of = |quarters: f64| view.x(canvas.left(), quarters);
        self.arrangement.lanes(
            ui.ctx(),
            &painter.with_clip_rect(canvas),
            canvas,
            &self.document,
            &mut self.lanes,
            view,
        );
        // Clip selection and editing (slice 6); a hover card with the
        // file's details; right-click the lanes for the grid menu (5a).
        let hover = ui.interact(canvas, ui.id().with("clips"), Sense::click_and_drag());
        self.clip_editing(ui, &hover, canvas, &painter);
        hover.context_menu(|ui| self.lanes_menu(ui));
        // Clip properties (slice 6e): renaming in the title bar, and the
        // pinned card, which takes the hover card's place.
        self.clip_rename(ui, canvas);
        self.clip_card(ui.ctx(), canvas);
        if let Some(pointer) = hover.hover_pos()
            && !hover.dragged()
            && self.clip_props.pinned().is_none()
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
        // freely) when the system reports it, otherwise at the playhead, on
        // new tracks at the end. Over an audio track's lane they land on it;
        // near the line between two tracks they make new tracks there
        // (slice 6e). The lane or the line, and a snap guide, show where
        // while files are dragged over.
        let (pointer, free, dragging) = ui.input(|i| {
            (
                self.drag_pointer
                    .or_else(|| i.pointer.hover_pos())
                    .filter(|p| canvas.contains(*p)),
                i.modifiers.alt,
                i.raw
                    .hovered_files
                    .iter()
                    .map(|f| {
                        f.path
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                    })
                    .collect::<Vec<_>>(),
            )
        });
        self.drop_target = match pointer {
            Some(p) => self.snapped(ruler::ticks_at(view.beat_at(canvas.left(), p.x)), free),
            None => self.transport.state().ticks,
        };
        let pending = self.lanes.pending().len();
        let (into, line) = match pointer {
            Some(p) => {
                self.arrangement
                    .file_drop(ui.ctx(), self.document.project(), pending, canvas, p.y)
            }
            None => (DropInto::NewTracks { before: None }, canvas.top()),
        };
        self.drop_into = into;
        if !dragging.is_empty() && pointer.is_some() {
            let x = x_of(self.drop_target.as_quarters());
            draw_snap_guide(&painter, x, at.guides());
            let label = match dragging.as_slice() {
                [one] => one.clone(),
                many => format!("{} files", many.len()),
            };
            self.arrangement.draw_file_drop(
                ui.ctx(),
                &painter,
                self.document.project(),
                pending,
                canvas,
                &FileDrop {
                    into,
                    line,
                    x,
                    label: &label,
                },
            );
        }

        self.locator_strip(ui, &painter, &at);
        self.bar_ruler(ui, &painter, &at);
        self.time_ruler(&painter, &at);
        self.time_ruler_input(ui, &painter, &at);

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
                palette.start_marker,
                Stroke::new(1.0, palette.canvas),
            ));
        }

        // Playhead: from the bar ruler through the time ruler.
        let x = x_of(settled(head, state.ticks.as_quarters()));
        if (full.left()..=full.right()).contains(&x) {
            painter.line_segment(
                [pos2(x, ruler.top()), pos2(x, full.bottom())],
                Stroke::new(1.0, palette.accent),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(x - 5.0, ruler.top()),
                    pos2(x + 5.0, ruler.top()),
                    pos2(x, ruler.top() + 7.0),
                ],
                palette.accent,
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
            .panel(ui.ctx(), pos2(canvas.left() + 8.0, canvas.bottom() - 8.0));
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
