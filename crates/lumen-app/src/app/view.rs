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
use crate::widgets::raised;
use crate::{
    App, arrangement, audio, bench, canvas, lanes, light, media, motion, ruler, theme, widgets,
};

/// A glided position, or the exact one once the glide has settled: the
/// glide runs in f32, which is pixels off at sample-level zoom.
/// The mark above an empty arrangement's words (`text`), half risen
/// until files that can be added are dragged over.
fn empty_mark(painter: &egui::Painter, text: egui::Rect, look: widgets::mark::Look) {
    let rect = egui::Rect::from_center_size(
        pos2(
            text.center().x,
            text.top() - theme::space::M - EMPTY_MARK_SIZE / 2.0,
        ),
        vec2(EMPTY_MARK_SIZE, EMPTY_MARK_SIZE),
    );
    widgets::mark::paint(painter, rect, look);
}

/// The mark's size above an empty arrangement.
const EMPTY_MARK_SIZE: f32 = 48.0;

/// The mark's size in the transport bar.
const MARK_SIZE: f32 = 22.0;

/// The panel frame of a banner under the transport: `app` ground with room
/// around the notice card inside it.
pub(crate) fn notice_panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::p().app)
        .inner_margin(egui::Margin::symmetric(
            theme::space::M as i8,
            theme::space::S as i8,
        ))
}

/// A notice (the Notices card, ADR-0052): `raised` ground with a 1 pt
/// `warning` hairline at 45%, radius `LARGE`, the warning icon, then
/// `add` on one row. Say what happened and what is safe; buttons are
/// `raised::button`, at most one `primary_button` for the main action.
pub(crate) fn notice_card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::p().raised)
        .stroke(Stroke::new(theme::stroke::HAIRLINE, theme::warning_edge()))
        .corner_radius(theme::radius::LARGE)
        .inner_margin(egui::Margin::symmetric(
            theme::space::M as i8,
            theme::space::S as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("\u{26A0}").color(theme::p().warning));
                add(ui);
            });
        });
}

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
        let editor = bench.editor();
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
                if editor {
                    self.bench_editor(view.top);
                }
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

    /// `--bench-editor`: keeps the stress project's dense clip open in the
    /// clip editor, its pitches scrolled to `top` (0 to 1) of their range.
    fn bench_editor(&mut self, top: f32) {
        // The roll has its own view (ADR-0048): the run drives it to the same
        // zoom and scroll as the arrangement, so it is measured zoomed.
        self.note_editor.time = self.nav.view;
        if self.note_editor.clip.is_none() {
            let dense = self
                .document
                .project()
                .tracks
                .last()
                .and_then(|t| t.clips.first())
                .map(|c| c.id);
            if let Some(id) = dense {
                self.open_note_editor(id);
                self.detail_size = 420.0;
            }
        }
        let rows = 128.0 * self.note_editor.row_height;
        self.note_editor.scroll = top * (rows - 300.0).max(0.0);
    }

    pub(crate) fn transport(&mut self, ui: &mut egui::Ui) {
        // The panel toggles keep their place at the right at any width or
        // scale, so their room is set aside first (as wide as they were
        // last frame); the transport takes the rest, and the status text
        // shortens to fit. They are drawn last, so Tab reaches them after
        // the transport, in reading order (slice 8: they came first).
        let full = ui.available_rect_before_wrap();
        let width_id = ui.id().with("panel toggles width");
        let width: f32 = ui.data(|d| d.get_temp(width_id)).unwrap_or(220.0);
        let split = (full.right() - width).max(full.left());
        let left = egui::Rect::from_min_max(full.min, egui::pos2(split, full.bottom()));
        let right = egui::Rect::from_min_max(egui::pos2(split, full.top()), full.max);
        let row = |rect| {
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center))
        };
        let midi = crate::transport::MidiView::gather(
            &self.midi,
            &self.settings.midi,
            self.live_keys
                .as_ref()
                .map_or(0, crate::live::LiveKeys::activity),
        );
        ui.scope_builder(row(left), |ui| {
            // Whatever does not fit is cut off here rather than drawn over
            // the panel toggles.
            ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
            self.transport_mark(ui);
            self.app_menu(ui);
            ui.separator();
            self.transport.keyboard_readout =
                self.keyboard.is_on().then(|| self.keyboard.readout());
            self.transport.ui(ui, &mut self.document, &self.keymap);
            if std::mem::take(&mut self.transport.keyboard_clicked) {
                let on = !self.keyboard.is_on();
                self.set_computer_keyboard(on);
            }
        });
        if let Some(info) = self.transport.info.take() {
            self.set_info(info);
        }
        let toggles = ui.scope_builder(row(right), |ui| {
            // The MIDI chip sits with the panel toggles, so it stays in view
            // however narrow the window: the transport is cut off first.
            self.transport.midi_chip(ui, &midi);
            ui.separator();
            let keys = self.keymap.keys(Command::OpenPalette);
            if icons::button(
                ui,
                Icon::Search,
                "Command palette",
                widgets::Tip {
                    keys: keys.as_ref(),
                    hint: None,
                },
                self.palette.is_open(),
                true,
                "",
            )
            .clicked()
            {
                self.toggle_palette();
            }
            widgets::toggle_button(ui, &mut self.show_browser, "Browser")
                .on_hover_text("Show or hide the browser");
            let detail = widgets::toggle_button(ui, &mut self.show_detail, "Detail");
            let keys = self.keymap.keys(Command::ToggleDetail);
            widgets::tip(detail, "Show or hide the detail panel", keys.as_ref(), None);
        });
        let used = toggles.response.rect.width();
        if (used - width).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(width_id, used));
            ui.ctx().request_repaint();
        }
    }

    /// The mark before the wordmark (ADR-0043): a meter of the master, its
    /// sun rising with the level. Recording turns it red from M9.
    fn transport_mark(&mut self, ui: &mut egui::Ui) {
        let master = self.document.project().master.id.0;
        let reading = self.arrangement.meters.reading(master);
        let look = widgets::mark::Look::level(reading.rms[0].max(reading.rms[1]), false);
        self.mark = look;
        let (_, rect) = ui.allocate_space(vec2(MARK_SIZE, MARK_SIZE));
        widgets::mark::paint(ui.painter(), rect, look);
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
        self.panes
            .anchor(ui, super::panes::Pane::Arrangement, whole);
        let response = ui.allocate_rect(whole, Sense::hover());
        // Layout A (owner, slice 5b): the locator strip and the bar ruler
        // on top, the time ruler at the bottom; track headers on the left
        // between their corners; the lanes in the middle.
        let header = self.arrangement.header_width();
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
        // Across the headers and the lanes: the pinned returns and master.
        let pinned = egui::Rect::from_x_y_ranges(headers.left()..=canvas.right(), canvas.y_range());
        self.arrangement
            .pinned_wheel(ui, pinned, self.document.project());
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
        self.arrangement.additive_solo = self.settings.mixing.additive_solo;
        self.lend_live_input();
        self.lend_cue_controls();
        // New, moved and deleted tracks glide (M4.5).
        self.arrangement.begin_frame(
            ui.ctx(),
            self.document.session(),
            self.document.project(),
            self.lanes.pending().len(),
        );
        self.arrangement
            .headers(ui, headers, &mut self.document, &self.lanes, &self.keymap);
        self.take_cue_controls();

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
        hover.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Lanes: click to move the playhead, drag clips, or drag below a clip's middle to select time",
            )
        });
        self.clip_editing(ui, &hover, canvas, &painter);
        widgets::Menu::context(&hover).show(|ui| self.lanes_menu(ui));
        // Where the tracks are scrolled: a bar that shows while scrolling
        // or near the right edge (M4.5, U2).
        let pending = self.lanes.pending().len();
        let (area, content) =
            self.arrangement
                .scroll_extent(ui.ctx(), self.document.project(), pending, canvas);
        let bar = widgets::ScrollBar {
            area,
            axis: widgets::Axis::Vertical,
            content,
            offset: self.arrangement.top,
        };
        if let Some(top) = widgets::scroll_bar(ui, ui.id().with("tracks scroll bar"), bar) {
            let all =
                self.arrangement
                    .height_of_content(ui.ctx(), self.document.project(), pending);
            self.arrangement.scroll_to(top, all, canvas.height());
        }
        self.arrangement.pinned_controls(ui, pinned, &self.document);
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
            let properties = self.keymap.text(Command::ClipProperties);
            hover.on_hover_ui_at_pointer(|ui| lanes::file_card(ui, item, properties.as_deref()));
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
            None => (
                DropInto::NewTracks {
                    before: None,
                    group: None,
                },
                canvas.top(),
            ),
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
        let playhead = self.transport.playhead().as_quarters();
        let head = motion::glide(
            &ctx,
            egui::Id::new("playhead"),
            playhead as f32,
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

        // Playhead: from the bar ruler through the time ruler. While
        // playing it is a beam (ADR-0043): a white core in the accent, a
        // halo, and light cast on the lanes it passes; stopped, it is unlit.
        let x = x_of(settled(head, playhead));
        if (full.left()..=full.right()).contains(&x) {
            let ys = egui::Rangef::new(ruler.top(), full.bottom());
            if state.playing {
                light::cast(&painter, light::BEAM, x, canvas.y_range(), palette.accent);
                light::vline(&painter, light::BEAM, x, ys, palette.accent, 1.0);
                light::cap(&painter, x, ruler.top(), palette.accent);
            } else {
                painter.vline(x, ys, Stroke::new(1.0, palette.accent));
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
            let text = painter.text(
                canvas.center(),
                egui::Align2::CENTER_CENTER,
                format!("{headline}\n\n{}", lines.join("\n")),
                egui::FontId::proportional(theme::size::TITLE),
                color,
            );
            // The mark rises over the files it can take (ADR-0043).
            let look = if supported > 0 {
                widgets::mark::Look::RISEN
            } else {
                widgets::mark::Look::WAITING
            };
            empty_mark(&painter, text, look);
        } else if self.lanes.is_empty(self.document.project()) {
            // Or choose them (M3 review H1: importing was mouse-drag only).
            let keys = self.keymap.keys(Command::ImportAudio);
            let state = widgets::EmptyState {
                // The mark waits, half risen, over an empty arrangement
                // (ADR-0043).
                icon: Some(|p, at, _| {
                    let rect =
                        egui::Rect::from_center_size(at, vec2(EMPTY_MARK_SIZE, EMPTY_MARK_SIZE));
                    widgets::mark::paint(p, rect, widgets::mark::Look::WAITING);
                }),
                icon_size: EMPTY_MARK_SIZE,
                title: "Drop audio files here",
                hint: "WAV, AIFF, FLAC, MP3 or Ogg Vorbis",
                action: Some(widgets::EmptyAction {
                    label: "Import audio\u{2026}",
                    keys: keys.as_ref(),
                    tip: "Choose audio files to add",
                }),
            };
            let clicked = widgets::empty_state(ui, canvas, &state);
            if clicked && self.availability(ui.ctx(), Command::ImportAudio).is_ok() {
                self.run(&ui.ctx().clone(), Command::ImportAudio);
            }
        }
        // Last, so it sits over the headers; on their side of the edge
        // only, so a drag at the lanes' left edge stays a clip or locator
        // drag.
        self.header_divider(
            ui,
            egui::Rect::from_x_y_ranges(corner.right() - 5.0..=corner.right(), whole.y_range()),
        );
        if let Some(info) = self.arrangement.info.take() {
            self.set_info(info);
        }
        if let Some(note) = self.arrangement.note.take() {
            self.toast = Some((note.into(), std::time::Instant::now()));
        }
        self.lanes
            .panel(ui.ctx(), pos2(canvas.left() + 8.0, canvas.bottom() - 8.0));
        self.toast(ui.ctx(), pos2(full.right() - 12.0, canvas.top() + 10.0));
    }

    /// The edge between the track headers and the lanes (MT-MIX-3 run 1):
    /// drag to widen the headers, double-click to go back to the default
    /// width. It glows in the accent while hovered or dragged, as a track's
    /// bottom edge does. Not an edit: saved with the project's view.
    fn header_divider(&mut self, ui: &mut egui::Ui, edge: egui::Rect) {
        let id = ui.id().with("header divider");
        let response = ui
            .interact(edge, id, Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Resize the track headers")
        });
        if response.hovered() {
            use super::info::{Held, Info, Mouse};
            self.arrangement.info = Some(
                Info::new("Track headers' edge")
                    .on(Held::NONE, Mouse::Drag, "widen or narrow")
                    .on(Held::NONE, Mouse::DoubleClick, "default width"),
            );
        }
        if response.drag_started() {
            ui.data_mut(|d| d.insert_temp(id, self.arrangement.header_width()));
        }
        if response.dragged() {
            let start: f32 = ui
                .data(|d| d.get_temp(id))
                .unwrap_or(arrangement::HEADER_WIDTH);
            let moved = response.total_drag_delta().map_or(0.0, |d| d.x);
            self.arrangement.set_header_width(start + moved);
        }
        if response.double_clicked() {
            self.arrangement.set_header_width(arrangement::HEADER_WIDTH);
        }
        let glow = motion::hover(
            ui.ctx(),
            id.with("glow"),
            response.hovered() || response.dragged(),
        );
        if glow > 0.0 {
            widgets::draw_edge_glow_vertical(ui.painter(), edge.right(), edge.y_range(), glow);
        }
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
        let shown = egui::Panel::top("device banner")
            .frame(notice_panel_frame())
            .show_collapsible(ui, &mut open, |ui| {
                notice_card(ui, |ui| {
                    ui.label(&message);
                    ui.add_enabled_ui(live, |ui| {
                        // The main action is the one primary: Try again when
                        // it is offered, else the settings.
                        let can_retry = retry && self.transport.audio.can_switch();
                        if can_retry && raised::primary_button(ui, "Try again").clicked() {
                            self.transport.audio.set_on(true);
                        }
                        let settings = if can_retry {
                            raised::button(ui, "Audio and MIDI settings…")
                        } else {
                            raised::primary_button(ui, "Audio and MIDI settings…")
                        };
                        if settings.clicked() {
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
