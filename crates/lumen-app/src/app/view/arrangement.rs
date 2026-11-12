//! The arrangement: the ruler strips, the grid, the lanes and their clips,
//! file drops, the playhead and start marker, and the empty state.

use std::path::PathBuf;

use eframe::egui::{self, Sense, Stroke, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_core::product::PRODUCT_NAME;
use lumen_project::model::TrackKind;

use crate::app::grid::draw_snap_guide;
use crate::app::rulers;
use crate::arrangement::FileDrop;
use crate::arrangement::auto_view::Scene;
use crate::commands::Command;
use crate::document::{DropInto, Places};
use crate::{App, canvas, lanes, light, media, motion, ruler, theme, widgets};

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

fn settled(glided: f32, exact: f64) -> f64 {
    if (f64::from(glided) - exact).abs() < 1e-3 {
        exact
    } else {
        f64::from(glided)
    }
}

impl App {
    pub(crate) fn arrangement(&mut self, ui: &mut egui::Ui) {
        let whole = ui.available_rect_before_wrap();
        self.panes
            .anchor(ui, super::super::panes::Pane::Arrangement, whole);
        let response = ui.allocate_rect(whole, Sense::hover());
        // Layout A (owner, slice 5b): the locator strip and the bar ruler
        // on top, the time ruler at the bottom; track headers on the left
        // between their corners; the lanes in the middle.
        let header = self.arrangement.header_width();
        let band = self.song_band_height(ui.ctx());
        let top = rulers::STRIP_HEIGHT + rulers::RULER_HEIGHT;
        let corner = egui::Rect::from_min_size(whole.min, vec2(header, top));
        let time_corner = egui::Rect::from_min_max(
            pos2(whole.left(), whole.bottom() - rulers::TIME_HEIGHT),
            pos2(whole.left() + header, whole.bottom()),
        );
        let headers = egui::Rect::from_min_max(
            pos2(whole.left(), corner.bottom() + band),
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
        let canvas =
            egui::Rect::from_min_max(pos2(full.left(), ruler.bottom() + band), time.right_top());

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
        let row = egui::Rect::from_min_max(
            pos2(corner.left(), corner.top() + rulers::STRIP_HEIGHT),
            corner.max,
        );
        // One well of grid, tool and tempo lanes (the interface audit's H1).
        let well = egui::Rect::from_min_max(
            pos2(row.left() + theme::space::S, row.center().y),
            pos2(row.right() - theme::space::S, row.center().y),
        )
        .expand2(vec2(0.0, theme::size::CONTROL_SMALL / 2.0))
        .round_to_pixels(ui.pixels_per_point());
        let widths = [
            theme::size::WELL_PIECE_W,
            self.grid_value_width(ui),
            0.0,
            theme::size::CONTROL,
        ];
        let pieces = widgets::well::paint(ui.painter(), well, &widths);
        self.grid_pieces(ui, pieces[0], pieces[1]);
        self.arrangement_tool_switch(ui, pieces[2]);
        self.song_lanes_button(ui, pieces[3]);
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
                "Lanes: click to move the playhead while stopped, drag clip headers to move, or drag clip bodies and empty lanes to select time",
            )
        });
        if !self.auto_editing(ui, &hover, canvas, &painter) {
            self.clip_editing(ui, &hover, canvas, &painter);
        }
        let pending = self.lanes.pending().len();
        self.lane_names();
        self.lane_texts(ui.ctx());
        self.arrangement.lane_overlay(
            ui,
            &painter,
            &mut self.document,
            Scene {
                canvas,
                view,
                pending,
                timeline: &self.transport.timeline,
            },
        );
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
            && let Some((item, marks)) = self.arrangement.item_at(
                ui.ctx(),
                pointer,
                canvas,
                self.document.project(),
                &self.lanes,
                view,
            )
        {
            let properties = self.keymap.text(Command::ClipProperties);
            hover.on_hover_ui_at_pointer(|ui| {
                lanes::file_card(ui, item, marks.words().as_deref(), properties.as_deref())
            });
        }

        // Dropped files land under the pointer (snapped; Alt places them
        // freely) when the system reports it, otherwise at the playhead, on
        // new tracks at the end. Over the lane of a track of their kind
        // (audio or MIDI, M5 slice 7) they land on it; near the line between
        // two tracks they make new tracks there (slice 6e). The lane or the
        // line, and a snap guide, show where while files are dragged over:
        // where MIDI files go when every file dragged is one.
        let carried: Vec<PathBuf> =
            egui::DragAndDrop::payload::<crate::browser::files::Dragged>(ui.ctx())
                .map(|d| d.0.clone())
                .unwrap_or_default();
        let (pointer, free, mut dragging, mut all_midi) = ui.input(|i| {
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
                !i.raw.hovered_files.is_empty()
                    && i.raw.hovered_files.iter().all(|f| {
                        f.path
                            .as_deref()
                            .is_some_and(crate::midi::files::is_midi_file)
                    }),
            )
        });
        if !carried.is_empty() {
            dragging = carried
                .iter()
                .map(|p| {
                    p.file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                })
                .collect();
            all_midi = carried.iter().all(|p| crate::midi::files::is_midi_file(p));
        }
        self.drop_target = match pointer {
            Some(p) => self.snapped(ruler::ticks_at(view.beat_at(canvas.left(), p.x)), free),
            None => self.transport.state().ticks,
        };
        let pending = self.lanes.pending().len();
        let place = |kind| match pointer {
            Some(p) => self.arrangement.file_drop(
                ui.ctx(),
                self.document.project(),
                pending,
                canvas,
                p.y,
                kind,
            ),
            None => (DropInto::END, canvas.top()),
        };
        let (audio, audio_line) = place(TrackKind::Audio);
        let (midi, midi_line) = place(TrackKind::Midi);
        self.drop_into = Places { audio, midi };
        let (into, line) = if all_midi {
            (midi, midi_line)
        } else {
            (audio, audio_line)
        };
        // A file from the browser let go over the lanes is added there.
        if !carried.is_empty() && pointer.is_some() && ui.input(|i| i.pointer.any_released()) {
            self.file_drop = Some(carried.clone());
            egui::DragAndDrop::clear_payload(ui.ctx());
        }
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
        self.song_band(ui, &at, band);

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
                Stroke::new(theme::stroke::HAIRLINE, palette.canvas),
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
                painter.vline(x, ys, Stroke::new(theme::stroke::HAIRLINE, palette.accent));
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
            let midi = |p: &PathBuf| crate::midi::files::is_midi_file(p);
            let supported = hovered
                .iter()
                .filter(|p| media::is_audio_file(p) || midi(p))
                .count();
            let color = if supported > 0 {
                theme::p().accent
            } else {
                theme::p().warning
            };
            painter.rect_stroke(
                canvas.shrink(2.0),
                4.0,
                Stroke::new(theme::stroke::EDGE, color),
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
                        None if midi(p) => format!("✓  {name}  (MIDI)"),
                        None => {
                            format!("✕  {name}  (not an audio or MIDI file {PRODUCT_NAME} reads)")
                        }
                    }
                })
                .collect();
            if hovered.len() > 8 {
                lines.push(format!("…and {} more", hovered.len() - 8));
            }
            let headline = match (supported, hovered.len()) {
                (0, _) => "Nothing here can be added".to_owned(),
                (n, total) if n == total => {
                    let kind = if hovered.iter().all(midi) {
                        "MIDI"
                    } else if hovered.iter().any(midi) {
                        "audio and MIDI"
                    } else {
                        "audio"
                    };
                    format!(
                        "Drop to add {n} {kind} file{}",
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
            let keys = self.keymap.keys(Command::Import);
            let state = widgets::EmptyState {
                // The mark waits, half risen, over an empty arrangement
                // (ADR-0043).
                icon: Some(|p, at, _| {
                    let rect =
                        egui::Rect::from_center_size(at, vec2(EMPTY_MARK_SIZE, EMPTY_MARK_SIZE));
                    widgets::mark::paint(p, rect, widgets::mark::Look::WAITING);
                }),
                icon_size: EMPTY_MARK_SIZE,
                title: "Drop audio or MIDI files here",
                hint: "WAV, AIFF, FLAC, MP3, Ogg Vorbis or MIDI",
                action: Some(widgets::EmptyAction {
                    label: "Import audio or MIDI\u{2026}",
                    keys: keys.as_ref(),
                    tip: "Choose audio or MIDI files to add",
                }),
            };
            // Above the pinned master's row, not behind it: in a short window
            // little room is left and the state gives up its icon and hint.
            let free = egui::Rect::from_min_size(
                canvas.min,
                vec2(
                    canvas.width(),
                    self.arrangement
                        .main_height(ui.ctx(), self.document.project(), canvas.height())
                        .min(canvas.height()),
                ),
            );
            let clicked = widgets::empty_state(ui, free, &state);
            if clicked && self.availability(ui.ctx(), Command::Import).is_ok() {
                self.run(&ui.ctx().clone(), Command::Import);
            }
        }
        // Last, so it sits over the headers; on their side of the edge
        // only, a point short of it, so a press on the lanes' first column
        // stays the lanes' (a clip or locator drag, or the point at the
        // start of a lane: KNOWN_ISSUES rows 37 and 54).
        self.header_divider(
            ui,
            egui::Rect::from_x_y_ranges(
                corner.right() - 6.0..=corner.right() - 1.0,
                whole.y_range(),
            ),
            corner.right(),
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
}
