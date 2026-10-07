//! The arrangement's tracks (M3 slice 4a): the header column beside the
//! timeline, and the lanes with their clips.
//!
//! Design (owner's choice from rendered options, 2026-10-07):
//! - **Headers:** a colour rail, the name, and mute, solo and a round
//!   record-arm button always visible.
//! - **Clips:** dark, with a track-coloured title bar and a bright waveform
//!   (the waveform comes in slice 4c).
//! - **Hints:** an armed track's lane has a faint red wash and a soloed
//!   track's a faint blue one; tracks that cannot be heard (muted, or not
//!   soloed while another track is) have their clips dimmed.
//!
//! Tracks fold and unfold, as in Ableton. Folded is a compact row; unfolded
//! is taller, with volume and pan in the header (automation lanes join it in
//! M7). Dragging a track's bottom edge resizes it: a folded track dragged
//! taller unfolds, and an unfolded one dragged small folds. Heights glide
//! (motion: GLIDE) and are saved with the project, but are not undo steps.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Id as EguiId, Key, Modifiers, Painter, Pos2, Rect,
    Sense, Stroke, Ui, pos2, vec2,
};
use lumen_project::model::{ClipContent, Color, Id, Project, TrackKind};

use crate::document::{Document, audible};
use crate::lanes::Lanes;
use crate::media::{Item, State};
use crate::{fonts, motion, theme};

/// Width of the header column.
pub const HEADER_WIDTH: f32 = 184.0;
/// A folded track.
pub const FOLDED: f32 = 44.0;
/// An unfolded track, until resized.
pub const UNFOLDED: f32 = 96.0;
/// Heights a track can be dragged to.
const MAX_HEIGHT: f32 = 320.0;
/// Dragging an unfolded track below this folds it.
const FOLD_BELOW: f32 = FOLDED + 16.0;
/// The clip title bar.
const TITLE: f32 = 15.0;
const BUTTON: f32 = 19.0;

/// One row of the arrangement, in content coordinates.
#[derive(Debug, Clone, Copy)]
struct Row {
    kind: RowKind,
    y: f32,
    height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RowKind {
    /// Index into the project's tracks.
    Track(usize),
    /// Index into the files loading for import.
    Pending(usize),
}

/// The arrangement's view state: vertical scroll, heights, folding,
/// selection and the track being renamed.
#[derive(Debug, Default)]
pub struct Arrangement {
    /// Vertical scroll, in points.
    pub top: f32,
    /// Unfolded heights of tracks that were resized.
    pub heights: BTreeMap<Id, f32>,
    /// Unfolded tracks.
    pub expanded: BTreeSet<Id>,
    /// Selected tracks, in the order they were selected.
    pub selected: Vec<Id>,
    /// Where a Shift-click range starts.
    anchor: Option<Id>,
    /// The track being renamed, with the text so far.
    renaming: Option<(Id, String)>,
    /// The track being resized and its height when the drag began.
    resizing: Option<(Id, f32)>,
}

fn color_of(c: Color) -> Color32 {
    Color32::from_rgb((c.0 >> 16) as u8, (c.0 >> 8) as u8, c.0 as u8)
}

fn color_from(c: Color32) -> Color {
    Color(u32::from(c.r()) << 16 | u32::from(c.g()) << 8 | u32::from(c.b()))
}

/// "−3.0 dB", "−∞ dB".
fn db_text(db: f32) -> String {
    if db <= -59.9 {
        "\u{2212}\u{221E} dB".into()
    } else if db.abs() < 0.05 {
        "0.0 dB".into()
    } else {
        format!("{db:+.1} dB").replace('-', "\u{2212}")
    }
}

/// "C", "L 30", "R 100".
fn pan_text(pan: f32) -> String {
    let n = (pan * 100.0).round() as i32;
    match n {
        0 => "C".into(),
        n if n < 0 => format!("L {}", -n),
        n => format!("R {n}"),
    }
}

impl Arrangement {
    /// The height a track is drawn at (before the glide).
    fn target_height(&self, id: Id) -> f32 {
        if self.expanded.contains(&id) {
            self.heights.get(&id).copied().unwrap_or(UNFOLDED)
        } else {
            FOLDED
        }
    }

    /// Every row with its position; heights glide to their targets.
    fn rows(&self, ctx: &egui::Context, project: &Project, pending: usize) -> Vec<Row> {
        let mut rows = Vec::with_capacity(project.tracks.len() + pending);
        let mut y = 0.0;
        for (i, track) in project.tracks.iter().enumerate() {
            let height = motion::glide(
                ctx,
                EguiId::new(("track height", track.id)),
                self.target_height(track.id),
                true,
            );
            rows.push(Row {
                kind: RowKind::Track(i),
                y,
                height,
            });
            y += height;
        }
        for i in 0..pending {
            rows.push(Row {
                kind: RowKind::Pending(i),
                y,
                height: FOLDED,
            });
            y += FOLDED;
        }
        rows
    }

    fn content_height(rows: &[Row]) -> f32 {
        rows.last().map_or(0.0, |r| r.y + r.height)
    }

    /// Forgets selected, resized or unfolded tracks that no longer exist.
    fn prune(&mut self, project: &Project) {
        let exists = |id: &Id| project.tracks.iter().any(|t| t.id == *id);
        self.selected.retain(exists);
        self.heights.retain(|id, _| exists(id));
        self.expanded.retain(exists);
        if self.renaming.as_ref().is_some_and(|(id, _)| !exists(id)) {
            self.renaming = None;
        }
    }

    /// Scrolls vertically by `delta` points, within the content.
    pub fn scroll(&mut self, delta: f32, content: f32, visible: f32) {
        self.top = (self.top - delta).clamp(0.0, (content - visible + FOLDED).max(0.0));
    }

    /// Height of everything, for scrolling.
    pub fn height_of_content(&self, ctx: &egui::Context, project: &Project, pending: usize) -> f32 {
        Self::content_height(&self.rows(ctx, project, pending))
    }

    /// Selects `id` the way a click does: alone, toggled with Ctrl, or as a
    /// range from the last click with Shift.
    fn click_select(&mut self, project: &Project, id: Id, modifiers: Modifiers) {
        if modifiers.command {
            if let Some(i) = self.selected.iter().position(|s| *s == id) {
                self.selected.remove(i);
            } else {
                self.selected.push(id);
            }
            self.anchor = Some(id);
        } else if modifiers.shift
            && let Some(anchor) = self.anchor
        {
            let index = |id: Id| project.tracks.iter().position(|t| t.id == id);
            if let (Some(a), Some(b)) = (index(anchor), index(id)) {
                let (lo, hi) = (a.min(b), a.max(b));
                self.selected = project.tracks[lo..=hi].iter().map(|t| t.id).collect();
            }
        } else {
            self.selected = vec![id];
            self.anchor = Some(id);
        }
    }

    /// The tracks a header button acts on: the whole selection when the
    /// clicked track is part of it, otherwise just that track.
    fn targets(&self, id: Id) -> Vec<Id> {
        if self.selected.len() > 1 && self.selected.contains(&id) {
            self.selected.clone()
        } else {
            vec![id]
        }
    }

    /// Moves the selected tracks one place up (`-1`) or down (`1`).
    fn nudge(&mut self, doc: &mut Document, by: isize) {
        let tracks = &doc.project().tracks;
        let positions: Vec<usize> = tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| self.selected.contains(&t.id))
            .map(|(i, _)| i)
            .collect();
        let (Some(&first), Some(&last)) = (positions.first(), positions.last()) else {
            return;
        };
        let target = if by < 0 {
            first.checked_sub(1)
        } else {
            (last + 1 < tracks.len()).then_some(first + 1)
        };
        if let Some(index) = target {
            let ids: Vec<Id> = positions.iter().map(|&i| tracks[i].id).collect();
            doc.move_tracks(&ids, index);
        }
    }

    /// Track shortcuts (not while typing in a field): Ctrl+T adds an audio
    /// track and Ctrl+Shift+T a MIDI track, after the selection; Ctrl+D
    /// duplicates the selected tracks; Delete removes them; F2 renames;
    /// Alt+Up and Alt+Down move them.
    pub fn shortcuts(&mut self, ctx: &egui::Context, doc: &mut Document) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |modifiers, key| {
            ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(modifiers, key)))
        };
        let after = self.selected.last().copied();
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::T) {
            self.selected = vec![doc.add_track(TrackKind::Midi, after)];
        } else if pressed(Modifiers::COMMAND, Key::T) {
            self.selected = vec![doc.add_track(TrackKind::Audio, after)];
        } else if pressed(Modifiers::COMMAND, Key::D) && !self.selected.is_empty() {
            self.selected = doc.duplicate_tracks(&self.selected);
        } else if (pressed(Modifiers::NONE, Key::Delete)
            || pressed(Modifiers::NONE, Key::Backspace))
            && !self.selected.is_empty()
        {
            doc.delete_tracks(&self.selected);
            self.selected.clear();
        } else if pressed(Modifiers::ALT, Key::ArrowUp) {
            self.nudge(doc, -1);
        } else if pressed(Modifiers::ALT, Key::ArrowDown) {
            self.nudge(doc, 1);
        } else if pressed(Modifiers::NONE, Key::F2)
            && let Some(id) = self.selected.last().copied()
            && let Some(track) = doc.project().tracks.iter().find(|t| t.id == id)
        {
            self.renaming = Some((id, track.name.clone()));
        }
    }

    /// The header column in `column`, whose top is level with the lanes'
    /// top (below the ruler).
    pub fn headers(&mut self, ui: &mut Ui, column: Rect, doc: &mut Document, lanes: &Lanes) {
        self.prune(doc.project());
        let ctx = ui.ctx().clone();
        let palette = *theme::p();
        let painter = ui.painter_at(column);
        painter.rect_filled(column, 0.0, palette.header);
        let rows = self.rows(&ctx, doc.project(), lanes.pending().len());
        let content = Self::content_height(&rows);

        // Below the last track: double-click or right-click to add one.
        let below = Rect::from_min_max(
            pos2(
                column.left(),
                (column.top() + content - self.top).max(column.top()),
            ),
            column.max,
        );
        if below.height() > 0.0 {
            let empty = ui.interact(below, ui.id().with("headers below"), Sense::click());
            if empty.double_clicked() {
                self.selected = vec![doc.add_track(TrackKind::Audio, None)];
            } else if empty.clicked() {
                self.selected.clear();
            }
            empty.context_menu(|ui| self.add_menu(ui, doc, None));
            if doc.project().tracks.is_empty() && lanes.pending().is_empty() {
                painter.text(
                    pos2(column.center().x, column.top() + 24.0),
                    Align2::CENTER_TOP,
                    "Double-click or press Ctrl+T\nto add a track",
                    FontId::proportional(theme::size::CAPTION),
                    palette.text_hint,
                );
            }
        }

        let modifiers = ui.input(|i| i.modifiers);
        let audible = audible(doc.project());
        for row in rows {
            let rect = Rect::from_min_size(
                pos2(column.left(), column.top() + row.y - self.top),
                vec2(column.width(), row.height),
            );
            if rect.bottom() < column.top() || rect.top() > column.bottom() {
                continue;
            }
            let RowKind::Track(index) = row.kind else {
                // A file still loading: a quiet placeholder header.
                let name = lanes
                    .pending()
                    .get(match row.kind {
                        RowKind::Pending(i) => i,
                        RowKind::Track(_) => unreachable!(),
                    })
                    .and_then(|(id, _)| lanes.library.get(*id))
                    .map_or_else(String::new, |i| i.name.clone());
                painter.text(
                    pos2(rect.left() + 12.0, rect.top() + FOLDED / 2.0),
                    Align2::LEFT_CENTER,
                    format!("{name} \u{00B7} loading"),
                    FontId::proportional(theme::size::BODY),
                    palette.text_hint,
                );
                painter.hline(
                    column.x_range(),
                    rect.bottom() - 0.5,
                    Stroke::new(1.0, palette.border),
                );
                continue;
            };
            let track = doc.project().tracks[index].clone();
            let id = EguiId::new(("track header", track.id));
            let selected = self.selected.contains(&track.id);
            let heard = audible(&track);

            // Background: click to select, right-click for the menu.
            let background = ui.interact(rect, id, Sense::click());
            if background.clicked() {
                self.click_select(doc.project(), track.id, modifiers);
            }
            background.context_menu(|ui| {
                if !self.selected.contains(&track.id) {
                    self.selected = vec![track.id];
                }
                self.track_menu(ui, doc, track.id);
            });
            let fill = if selected {
                palette.header.lerp_to_gamma(palette.accent_muted, 0.75)
            } else {
                palette.header
            };
            painter.rect_filled(rect, 0.0, fill);
            painter.rect_filled(
                Rect::from_min_size(rect.min, vec2(4.0, rect.height())),
                0.0,
                color_of(track.color),
            );

            // Top row: fold arrow, name, M S ●.
            let mid = rect.top() + FOLDED / 2.0;
            let unfolded = motion::glide(
                &ctx,
                id.with("fold"),
                if self.expanded.contains(&track.id) {
                    1.0
                } else {
                    0.0
                },
                true,
            );
            let arrow = Rect::from_center_size(pos2(rect.left() + 16.0, mid), vec2(16.0, 18.0));
            let fold = ui
                .interact(arrow, id.with("fold button"), Sense::click())
                .on_hover_text(if self.expanded.contains(&track.id) {
                    "Fold the track"
                } else {
                    "Unfold the track (volume and pan; more detail in the lane)"
                });
            fold.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("Fold {}", track.name),
                )
            });
            if fold.clicked() {
                self.toggle_fold(track.id);
            }
            draw_arrow(
                &painter,
                arrow.center(),
                unfolded,
                if fold.hovered() {
                    palette.text
                } else {
                    palette.text_secondary
                },
            );

            let mut x = rect.right() - 8.0 - BUTTON;
            let arm_rect = Rect::from_min_size(pos2(x, mid - BUTTON / 2.0), vec2(BUTTON, BUTTON));
            x -= BUTTON + 4.0;
            let solo_rect = Rect::from_min_size(pos2(x, mid - BUTTON / 2.0), vec2(BUTTON, BUTTON));
            x -= BUTTON + 4.0;
            let mute_rect = Rect::from_min_size(pos2(x, mid - BUTTON / 2.0), vec2(BUTTON, BUTTON));

            let name_rect = Rect::from_min_max(
                pos2(rect.left() + 26.0, mid - 10.0),
                pos2(mute_rect.left() - 6.0, mid + 10.0),
            );
            self.name(ui, &painter, doc, &track, name_rect, heard, id);

            let toggle =
                |ui: &mut Ui, r: Rect, what: &str, label: &str, on: bool, on_fill: Color32| {
                    let response =
                        ui.interact(r, id.with(what), Sense::click())
                            .on_hover_text(match what {
                                "mute" => "Mute (0)",
                                _ => "Solo: click for this track only, Ctrl+click to add it",
                            });
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Checkbox,
                            true,
                            on,
                            format!("{} {}", label, track.name),
                        )
                    });
                    let hover =
                        motion::hover(ui.ctx(), id.with(what).with("hover"), response.hovered());
                    let fill = if on {
                        on_fill
                    } else {
                        palette.control.lerp_to_gamma(palette.control_hover, hover)
                    };
                    let p = ui.painter();
                    p.rect_filled(r, theme::radius::SMALL, fill);
                    p.text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        &label[..1],
                        fonts::medium(theme::size::CAPTION),
                        if on {
                            on_text(on_fill)
                        } else {
                            palette.text_secondary
                        },
                    );
                    response.clicked()
                };
            if toggle(
                ui,
                mute_rect,
                "mute",
                "Mute",
                track.mute,
                palette.start_marker,
            ) {
                doc.set_mute(&self.targets(track.id), !track.mute);
            }
            if toggle(ui, solo_rect, "solo", "Solo", track.solo, palette.accent) {
                doc.set_solo(&self.targets(track.id), !track.solo, !modifiers.command);
            }
            if arm_button(ui, arm_rect, id, &track.name, track.armed) {
                doc.set_armed(&self.targets(track.id), !track.armed);
            }

            // Unfolded: volume and pan on a second row.
            if row.height >= FOLDED + 22.0 {
                self.mixer_row(ui, doc, &track, rect, id);
            }

            // Bottom edge: drag to resize.
            let edge = Rect::from_min_max(
                pos2(rect.left(), rect.bottom() - 3.0),
                pos2(rect.right(), rect.bottom() + 3.0),
            );
            let resize = ui
                .interact(edge, id.with("resize"), Sense::drag())
                .on_hover_cursor(CursorIcon::ResizeVertical);
            if resize.drag_started() {
                self.resizing = Some((track.id, row.height));
            }
            if resize.dragged()
                && let Some((rid, start)) = self.resizing
                && rid == track.id
            {
                let total = resize.total_drag_delta().map_or(0.0, |d| d.y);
                let wanted = (start + total).clamp(FOLDED, MAX_HEIGHT);
                if wanted < FOLD_BELOW {
                    self.expanded.remove(&track.id);
                } else {
                    self.expanded.insert(track.id);
                    self.heights.insert(track.id, wanted);
                }
                // Follow the pointer exactly while dragging.
                ctx.data_mut(|d| d.remove::<f32>(EguiId::new(("track height", track.id))));
            }
            if resize.drag_stopped() {
                self.resizing = None;
            }
            painter.hline(
                column.x_range(),
                rect.bottom() - 0.5,
                Stroke::new(1.0, palette.border),
            );
        }
        painter.vline(
            column.right() - 0.5,
            column.y_range(),
            Stroke::new(1.0, palette.border),
        );
    }

    pub fn toggle_fold(&mut self, id: Id) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
    }

    /// The name, or the rename field. Double-click to rename; Enter or
    /// clicking away keeps the new name, Esc cancels.
    #[allow(clippy::too_many_arguments)]
    fn name(
        &mut self,
        ui: &mut Ui,
        painter: &Painter,
        doc: &mut Document,
        track: &lumen_project::model::Track,
        rect: Rect,
        heard: bool,
        id: EguiId,
    ) {
        let palette = theme::p();
        if let Some((renaming, text)) = &mut self.renaming
            && *renaming == track.id
        {
            let edit_id = id.with("rename");
            let response = ui.put(
                rect,
                egui::TextEdit::singleline(text)
                    .id(edit_id)
                    .font(FontId::proportional(theme::size::BODY))
                    .margin(vec2(4.0, 2.0)),
            );
            if !response.has_focus() && !response.lost_focus() {
                response.request_focus();
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                self.renaming = None;
            } else if response.lost_focus() {
                let name = text.clone();
                self.renaming = None;
                doc.rename_track(track.id, &name);
            }
            return;
        }
        let response = ui
            .interact(rect, id.with("name"), Sense::click())
            .on_hover_text("Double-click to rename (F2)");
        if response.double_clicked() {
            self.renaming = Some((track.id, track.name.clone()));
        } else if response.clicked() {
            let modifiers = ui.input(|i| i.modifiers);
            self.click_select(doc.project(), track.id, modifiers);
        }
        let galley = painter.layout(
            track.name.clone(),
            FontId::proportional(theme::size::BODY),
            if heard {
                palette.text
            } else {
                palette.text_hint
            },
            rect.width(),
        );
        // One line, shortened with "…" if too long.
        let text = if galley.rows.len() > 1 {
            let mut name = track.name.clone();
            while !name.is_empty() {
                name.pop();
                let g = painter.layout_no_wrap(
                    format!("{name}\u{2026}"),
                    FontId::proportional(theme::size::BODY),
                    palette.text,
                );
                if g.size().x <= rect.width() {
                    break;
                }
            }
            format!("{name}\u{2026}")
        } else {
            track.name.clone()
        };
        painter.text(
            pos2(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(theme::size::BODY),
            if heard {
                palette.text
            } else {
                palette.text_hint
            },
        );
    }

    /// Volume and pan, for an unfolded track. Double-click resets either.
    fn mixer_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &lumen_project::model::Track,
        rect: Rect,
        id: EguiId,
    ) {
        let palette = theme::p();
        let y = rect.top() + FOLDED + 8.0;
        let left = rect.left() + 26.0;
        let label = |x: f32, text: &str| {
            ui.painter().text(
                pos2(x, y),
                Align2::LEFT_CENTER,
                text,
                FontId::proportional(theme::size::CAPTION),
                palette.text_secondary,
            );
        };
        label(left, "Vol");
        label(left + 90.0, "Pan");
        let mut db = track.gain_db;
        let volume = ui
            .put(
                Rect::from_min_size(pos2(left + 22.0, y - 9.0), vec2(62.0, 18.0)),
                egui::DragValue::new(&mut db)
                    .range(-60.0..=6.0)
                    .speed(0.1)
                    .custom_formatter(|v, _| db_text(v as f32))
                    .custom_parser(|s| {
                        s.trim()
                            .trim_end_matches("dB")
                            .trim()
                            .replace('\u{2212}', "-")
                            .parse()
                            .ok()
                    }),
            )
            .on_hover_text("Track volume: drag, or click and type. Double-click for 0 dB");
        volume.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::DragValue,
                true,
                format!("Volume {}", track.name),
            )
        });
        if volume.double_clicked() {
            db = 0.0;
        }
        if db != track.gain_db {
            doc.apply(
                "Change volume",
                Some(&format!("volume {:x}", track.id.0)),
                |p| {
                    if let Some(t) = p.tracks.iter_mut().find(|t| t.id == track.id) {
                        std::sync::Arc::make_mut(t).gain_db = db;
                    }
                },
            );
        }
        let mut pan = track.pan;
        let pan_response = ui
            .put(
                Rect::from_min_size(pos2(left + 114.0, y - 9.0), vec2(36.0, 18.0)),
                egui::DragValue::new(&mut pan)
                    .range(-1.0..=1.0)
                    .speed(0.01)
                    .custom_formatter(|v, _| pan_text(v as f32))
                    .custom_parser(|s| {
                        let s = s.trim().to_ascii_uppercase();
                        if s == "C" {
                            return Some(0.0);
                        }
                        let (side, n) = s.split_at(1);
                        let n: f64 = n.trim().parse().ok()?;
                        match side {
                            "L" => Some(-n / 100.0),
                            "R" => Some(n / 100.0),
                            _ => s.parse::<f64>().ok().map(|v| v / 100.0),
                        }
                    }),
            )
            .on_hover_text("Pan: drag, or type L 30 or R 30. Double-click to centre");
        pan_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::DragValue,
                true,
                format!("Pan {}", track.name),
            )
        });
        if pan_response.double_clicked() {
            pan = 0.0;
        }
        if pan != track.pan {
            doc.apply("Change pan", Some(&format!("pan {:x}", track.id.0)), |p| {
                if let Some(t) = p.tracks.iter_mut().find(|t| t.id == track.id) {
                    std::sync::Arc::make_mut(t).pan = pan;
                }
            });
        }
        let _ = id;
    }

    /// Right-click menu on a track.
    fn track_menu(&mut self, ui: &mut Ui, doc: &mut Document, id: Id) {
        let targets = self.targets(id);
        let many = targets.len() > 1;
        if ui
            .add(egui::Button::new("Rename").shortcut_text("F2"))
            .clicked()
        {
            if let Some(track) = doc.project().tracks.iter().find(|t| t.id == id) {
                self.renaming = Some((id, track.name.clone()));
            }
            ui.close();
        }
        if ui
            .add(
                egui::Button::new(if many {
                    "Duplicate tracks"
                } else {
                    "Duplicate"
                })
                .shortcut_text("Ctrl+D"),
            )
            .clicked()
        {
            self.selected = doc.duplicate_tracks(&targets);
            ui.close();
        }
        if ui
            .add(
                egui::Button::new(if many { "Delete tracks" } else { "Delete" })
                    .shortcut_text("Del"),
            )
            .clicked()
        {
            doc.delete_tracks(&targets);
            self.selected.clear();
            ui.close();
        }
        if ui
            .add(egui::Button::new("Move up").shortcut_text("Alt+\u{2191}"))
            .clicked()
        {
            self.nudge(doc, -1);
            ui.close();
        }
        if ui
            .add(egui::Button::new("Move down").shortcut_text("Alt+\u{2193}"))
            .clicked()
        {
            self.nudge(doc, 1);
            ui.close();
        }
        if ui
            .button(if self.expanded.contains(&id) {
                "Fold"
            } else {
                "Unfold"
            })
            .clicked()
        {
            self.toggle_fold(id);
            ui.close();
        }
        ui.menu_button("Colour", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(6.0 * 26.0);
                for c in theme::CLIP_COLORS {
                    let (r, response) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                    ui.painter().rect_filled(r, theme::radius::SMALL, c);
                    if response.hovered() {
                        ui.painter().rect_stroke(
                            r,
                            theme::radius::SMALL,
                            Stroke::new(1.5, theme::p().text),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if response.clicked() {
                        doc.recolor_tracks(&targets, color_from(c));
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        self.add_menu(ui, doc, Some(id));
    }

    /// "Add … track" entries, adding after `after`.
    fn add_menu(&mut self, ui: &mut Ui, doc: &mut Document, after: Option<Id>) {
        for (kind, text, keys) in [
            (TrackKind::Audio, "Add audio track", "Ctrl+T"),
            (TrackKind::Midi, "Add MIDI track", "Ctrl+Shift+T"),
            (TrackKind::Group, "Add group track", ""),
            (TrackKind::Return, "Add return track", ""),
        ] {
            if ui
                .add(egui::Button::new(text).shortcut_text(keys))
                .clicked()
            {
                self.selected = vec![doc.add_track(kind, after)];
                ui.close();
            }
        }
    }

    /// The lanes and clips in `canvas`; `x_of` maps quarter notes to x.
    pub fn lanes(
        &self,
        ctx: &egui::Context,
        painter: &Painter,
        canvas: Rect,
        project: &Project,
        lanes: &mut Lanes,
        x_of: impl Fn(f32) -> f32,
    ) {
        let palette = *theme::p();
        let audible = audible(project);
        let rows = self.rows(ctx, project, lanes.pending().len());
        for row in rows {
            let lane = Rect::from_min_size(
                pos2(canvas.left(), canvas.top() + row.y - self.top),
                vec2(canvas.width(), row.height),
            );
            if lane.bottom() < canvas.top() || lane.top() > canvas.bottom() {
                continue;
            }
            match row.kind {
                RowKind::Track(index) => {
                    let track = &project.tracks[index];
                    if track.armed {
                        painter.rect_filled(lane, 0.0, palette.record.gamma_multiply(0.07));
                    } else if track.solo {
                        painter.rect_filled(lane, 0.0, palette.accent.gamma_multiply(0.06));
                    }
                    let heard = audible(track);
                    for clip in &track.clips {
                        let x0 = x_of(clip.start.as_quarters() as f32);
                        let x1 = x_of((clip.start + clip.length).as_quarters() as f32);
                        if x1 < canvas.left() || x0 > canvas.right() {
                            continue;
                        }
                        let rect = Rect::from_x_y_ranges(
                            x0..=x1.max(x0 + 2.0),
                            lane.top() + 3.0..=lane.bottom() - 3.0,
                        );
                        let (item, key) = match &clip.content {
                            ClipContent::Audio { media, .. } => {
                                (lanes.library.get(media.0).map(item_state), media.0)
                            }
                            ClipContent::Midi { .. } => (Some(ClipState::Ready), clip.id.0),
                        };
                        let shown = lanes.shown(key, ctx);
                        let progress = match &clip.content {
                            ClipContent::Audio { media, .. } => lanes.library.progress(media.0),
                            ClipContent::Midi { .. } => None,
                        };
                        draw_clip(
                            painter,
                            rect,
                            lane,
                            &clip.name,
                            color_of(track.color),
                            item.unwrap_or(ClipState::Missing),
                            progress,
                            if heard { shown } else { shown * 0.42 },
                        );
                    }
                }
                RowKind::Pending(i) => {
                    let Some(&(id, at)) = lanes.pending().get(i) else {
                        continue;
                    };
                    let x0 = x_of(at.as_quarters() as f32);
                    let rect = Rect::from_x_y_ranges(
                        x0..=x0 + 160.0,
                        lane.top() + 3.0..=lane.bottom() - 3.0,
                    );
                    let name = lanes
                        .library
                        .get(id)
                        .map_or_else(String::new, |i| i.name.clone());
                    let state = lanes.library.get(id).map_or(ClipState::Loading, item_state);
                    let progress = lanes.library.progress(id);
                    let shown = lanes.shown(id, ctx);
                    let color =
                        theme::CLIP_COLORS[(project.tracks.len() + i) % theme::CLIP_COLORS.len()];
                    draw_clip(painter, rect, lane, &name, color, state, progress, shown);
                }
            }
            painter.hline(
                canvas.x_range(),
                lane.bottom() - 0.5,
                Stroke::new(1.0, palette.border.gamma_multiply(0.6)),
            );
        }
    }

    /// The file of the audio clip under `pointer`, for the hover card.
    pub fn item_at<'a>(
        &self,
        ctx: &egui::Context,
        pointer: Pos2,
        canvas: Rect,
        project: &Project,
        lanes: &'a Lanes,
        x_of: impl Fn(f32) -> f32,
    ) -> Option<&'a Item> {
        let rows = self.rows(ctx, project, lanes.pending().len());
        let row = rows.iter().find(|r| {
            let y = canvas.top() + r.y - self.top;
            (y..y + r.height).contains(&pointer.y)
        })?;
        match row.kind {
            RowKind::Track(index) => project.tracks[index].clips.iter().find_map(|clip| {
                let x0 = x_of(clip.start.as_quarters() as f32);
                let x1 = x_of((clip.start + clip.length).as_quarters() as f32);
                match &clip.content {
                    ClipContent::Audio { media, .. } if (x0..=x1).contains(&pointer.x) => {
                        lanes.library.get(media.0)
                    }
                    _ => None,
                }
            }),
            RowKind::Pending(i) => {
                let &(id, at) = lanes.pending().get(i)?;
                let x0 = x_of(at.as_quarters() as f32);
                (x0..=x0 + 160.0)
                    .contains(&pointer.x)
                    .then(|| lanes.library.get(id))
                    .flatten()
            }
        }
    }

    /// The saved view state, written into a project being saved.
    pub fn save_into(&self, view: &mut lumen_project::model::ViewState) {
        view.top = self.top;
        view.track_heights = self.heights.clone();
        view.expanded = self.expanded.clone();
    }

    /// Restores the view state of an opened project.
    pub fn restore(view: &lumen_project::model::ViewState) -> Self {
        Self {
            top: view.top.max(0.0),
            heights: view
                .track_heights
                .iter()
                .map(|(id, h)| (*id, h.clamp(FOLD_BELOW, MAX_HEIGHT)))
                .collect(),
            expanded: view.expanded.clone(),
            ..Self::default()
        }
    }
}

/// What a clip's audio is doing, for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ClipState {
    Ready,
    Loading,
    Failed,
    Missing,
}

fn item_state(item: &Item) -> ClipState {
    match item.state {
        State::Ready(_) => ClipState::Ready,
        State::Loading => ClipState::Loading,
        State::Failed(_) => ClipState::Failed,
    }
}

/// A clip: dark body, track-coloured border and title bar with the name.
/// `alpha` fades it in and dims it when the track cannot be heard.
#[allow(clippy::too_many_arguments)]
fn draw_clip(
    painter: &Painter,
    rect: Rect,
    lane: Rect,
    name: &str,
    color: Color32,
    state: ClipState,
    progress: Option<f32>,
    alpha: f32,
) {
    let palette = theme::p();
    let radius = theme::radius::SMALL;
    let (bar, label) = match state {
        ClipState::Ready => (color, name.to_owned()),
        ClipState::Loading => (
            color.gamma_multiply(0.6),
            format!(
                "{name} \u{00B7} loading {:.0}%",
                progress.unwrap_or(0.0) * 100.0
            ),
        ),
        ClipState::Failed => (palette.record, format!("{name} \u{00B7} could not load")),
        ClipState::Missing => (palette.record, format!("{name} \u{00B7} file missing")),
    };
    painter.rect_filled(rect, radius, palette.clip_body.gamma_multiply(alpha));
    if let (ClipState::Loading, Some(p)) = (state, progress) {
        let done = Rect::from_min_size(rect.min, vec2(rect.width() * p, rect.height()));
        painter.rect_filled(done, radius, color.gamma_multiply(0.25 * alpha));
    }
    let title = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE.min(rect.height())));
    painter.rect_filled(
        title,
        egui::CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        },
        bar.gamma_multiply(alpha),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, color.gamma_multiply(0.6 * alpha)),
        egui::StrokeKind::Inside,
    );
    let visible = title.intersect(lane);
    if visible.width() > 8.0 {
        painter
            .with_clip_rect(visible.shrink2(vec2(4.0, 0.0)))
            .text(
                pos2(visible.left() + 5.0, title.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(theme::size::CAPTION),
                // Dimmed clips keep a readable name in both themes.
                if alpha < 0.9 {
                    palette.text_secondary.gamma_multiply(alpha.max(0.6))
                } else {
                    theme::CLIP_TEXT.gamma_multiply(alpha)
                },
            );
    }
}

/// Text on a lit button: near-black on light fills, white on dark ones.
fn on_text(fill: Color32) -> Color32 {
    let luma =
        0.2126 * f32::from(fill.r()) + 0.7152 * f32::from(fill.g()) + 0.0722 * f32::from(fill.b());
    if luma > 140.0 {
        Color32::from_rgb(0x0B, 0x0D, 0x10)
    } else {
        Color32::WHITE
    }
}

/// The fold arrow: pointing right when folded, down when unfolded (`t`
/// glides from 0 to 1).
fn draw_arrow(painter: &Painter, center: Pos2, t: f32, color: Color32) {
    let angle = t * std::f32::consts::FRAC_PI_2;
    let (s, c) = angle.sin_cos();
    let rotate = |x: f32, y: f32| center + vec2(x * c - y * s, x * s + y * c);
    painter.add(egui::Shape::convex_polygon(
        vec![rotate(-2.5, -4.0), rotate(3.5, 0.0), rotate(-2.5, 4.0)],
        color,
        Stroke::NONE,
    ));
}

/// The record-arm button: round, with a red dot that lights when armed.
fn arm_button(ui: &mut Ui, rect: Rect, id: EguiId, name: &str, armed: bool) -> bool {
    let palette = theme::p();
    let response = ui
        .interact(rect, id.with("arm"), Sense::click())
        .on_hover_text("Arm for recording (recording arrives in Milestone 9)");
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            armed,
            format!("Arm {name}"),
        )
    });
    let hover = motion::hover(ui.ctx(), id.with("arm hover"), response.hovered());
    let p = ui.painter();
    let center = rect.center();
    let back = if armed {
        palette.record.gamma_multiply(0.25)
    } else {
        palette.control.lerp_to_gamma(palette.control_hover, hover)
    };
    p.circle_filled(center, rect.width() / 2.0, back);
    let dot = if armed {
        palette.record
    } else {
        palette.record.gamma_multiply(0.45 + 0.25 * hover)
    };
    p.circle_filled(center, 4.5, dot);
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_naturally() {
        assert_eq!(db_text(0.0), "0.0 dB");
        assert_eq!(db_text(-3.0), "\u{2212}3.0 dB");
        assert_eq!(db_text(6.0), "+6.0 dB");
        assert_eq!(db_text(-60.0), "\u{2212}\u{221E} dB");
        assert_eq!(pan_text(0.0), "C");
        assert_eq!(pan_text(-0.3), "L 30");
        assert_eq!(pan_text(1.0), "R 100");
    }

    #[test]
    fn heights_fold_and_unfold() {
        let mut a = Arrangement::default();
        let id = Id(7);
        assert_eq!(a.target_height(id), FOLDED);
        a.toggle_fold(id);
        assert_eq!(a.target_height(id), UNFOLDED);
        a.heights.insert(id, 180.0);
        assert_eq!(a.target_height(id), 180.0);
        a.toggle_fold(id);
        assert_eq!(
            a.target_height(id),
            FOLDED,
            "folding keeps the unfolded height for later"
        );
        a.toggle_fold(id);
        assert_eq!(a.target_height(id), 180.0);
    }

    #[test]
    fn tracks_move_with_alt_and_the_arrow_keys() {
        let mut doc = Document::new(crate::empty_project());
        let ids: Vec<Id> = (0..3)
            .map(|_| doc.add_track(TrackKind::Audio, None))
            .collect();
        let mut a = Arrangement {
            selected: vec![ids[2]],
            ..Arrangement::default()
        };
        let order =
            |doc: &Document| -> Vec<Id> { doc.project().tracks.iter().map(|t| t.id).collect() };
        a.nudge(&mut doc, -1);
        assert_eq!(order(&doc), [ids[0], ids[2], ids[1]]);
        a.nudge(&mut doc, -1);
        a.nudge(&mut doc, -1);
        assert_eq!(order(&doc), [ids[2], ids[0], ids[1]], "stops at the top");
        a.nudge(&mut doc, 1);
        assert_eq!(order(&doc), [ids[0], ids[2], ids[1]]);
    }

    #[test]
    fn view_state_round_trips() {
        let mut a = Arrangement {
            top: 120.0,
            ..Arrangement::default()
        };
        a.expanded.insert(Id(3));
        a.heights.insert(Id(3), 150.0);
        let mut view = lumen_project::model::ViewState::default();
        a.save_into(&mut view);
        let b = Arrangement::restore(&view);
        assert_eq!(
            (b.top, b.expanded, b.heights),
            (a.top, a.expanded, a.heights)
        );
    }

    #[test]
    fn selection_follows_clicks() {
        let mut doc = Document::new(crate::empty_project());
        let ids: Vec<Id> = (0..4)
            .map(|_| doc.add_track(TrackKind::Audio, None))
            .collect();
        let mut a = Arrangement::default();
        let project = doc.project();
        a.click_select(project, ids[1], Modifiers::NONE);
        assert_eq!(a.selected, [ids[1]]);
        a.click_select(project, ids[3], Modifiers::SHIFT);
        assert_eq!(a.selected, [ids[1], ids[2], ids[3]]);
        a.click_select(project, ids[2], Modifiers::COMMAND);
        assert_eq!(a.selected, [ids[1], ids[3]]);
        assert_eq!(
            a.targets(ids[3]),
            [ids[1], ids[3]],
            "buttons act on the selection"
        );
        assert_eq!(
            a.targets(ids[0]),
            [ids[0]],
            "or on a track outside it alone"
        );
    }
}
