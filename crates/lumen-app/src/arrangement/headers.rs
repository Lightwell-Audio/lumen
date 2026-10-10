//! The track header column (M3 slice 4a): rail, fold arrow, type icon,
//! name, M S and the record-arm button; the unfolded volume and pan row;
//! resizing. Design: DESIGN §7a, ADR-0025. Groups step their tracks in
//! (`nesting`, M4 slice 3); the menus are in `menus`.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Id as EguiId, Key, Rect, Sense, Stroke, Ui, pos2,
    vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{Id, TrackKind};

use super::nesting::{self, Arm};
use super::{
    Arrangement, BUTTON, FOLD_BELOW, FOLDED, MAX_HEIGHT, RowKind, STEP, Zone, snap_height,
};
use crate::commands::{Command, Keymap};
use crate::document::Document;
use crate::icons::draw_type_icon;
use crate::lanes::Lanes;
use crate::widgets::{self, SliderKind, draw_arrow, draw_edge_glow, gradient, value_slider};

/// A header's content: the arrow, type icon and name, from its left (after
/// the rail and any indent).
const ARROW_X: f32 = 16.0;
const ICON_X: f32 = 34.0;
const NAME_X: f32 = 48.0;
use crate::{fonts, motion, theme};

impl Arrangement {
    /// The header column in `column`, whose top is level with the lanes'
    /// top (below the ruler).
    pub fn headers(
        &mut self,
        ui: &mut Ui,
        column: Rect,
        doc: &mut Document,
        lanes: &Lanes,
        keymap: &Keymap,
    ) {
        self.prune(doc.project());
        let ctx = ui.ctx().clone();
        let palette = *theme::p();
        let painter = ui.painter_at(column);
        painter.rect_filled(column, 0.0, palette.header);
        self.view_height = column.height();
        // A send and its return light together: what was hovered last frame.
        self.hot_return = self.hot_return_next.take();
        // A send being set in a return's list keeps its row (S1).
        self.held_send = self.held_next.take();
        let rows = self.rows(&ctx, doc.project(), lanes.pending().len());
        let content = self.content_height(&ctx, doc.project(), &rows);
        // The scrolling tracks end where the pinned returns and master
        // begin (slice 4).
        let main = self.zone_span(&ctx, doc.project(), Zone::Main, column.y_range());

        // Below the last track: double-click or right-click to add one.
        let below = Rect::from_min_max(
            pos2(
                column.left(),
                (column.top() + Self::main_bottom(&rows) - self.top).max(column.top()),
            ),
            pos2(column.right(), main.max),
        );
        if below.height() > 0.0 {
            let empty = ui.interact(below, ui.id().with("headers below"), Sense::click());
            empty.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Other,
                    true,
                    "Empty track area: double-click to add an audio track",
                )
            });
            if empty.hovered() {
                use crate::app::info::{Held, Info, Mouse};
                self.info = Some(
                    Info::new("Empty track area")
                        .on(Held::NONE, Mouse::DoubleClick, "add an audio track")
                        .on(Held::NONE, Mouse::RightClick, "add another kind")
                        .key(Command::AddAudioTrack, "audio track")
                        .key(Command::AddMidiTrack, "MIDI track"),
                );
            }
            if empty.double_clicked() {
                self.selected = vec![doc.add_track(TrackKind::Audio, None)];
            } else if empty.clicked() {
                self.selected.clear();
            }
            empty.context_menu(|ui| self.add_menu(ui, doc, None, keymap));

            // "+ Add track ▾": a menu of every track type with its shortcut,
            // and a hint that double-clicking here adds an audio track
            // (owner's choice, option B).
            let button = Rect::from_min_size(
                below.min + vec2(8.0, 8.0),
                vec2(column.width() - 16.0, 26.0),
            );
            if button.bottom() < main.max {
                let response = ui
                    .interact(button, ui.id().with("add track"), Sense::click())
                    .on_hover_text("Add an audio, MIDI, group or return track");
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add track")
                });
                let hover =
                    motion::hover(&ctx, ui.id().with("add track hover"), response.hovered());
                let open =
                    egui::Popup::is_id_open(&ctx, egui::Popup::default_response_id(&response));
                painter.rect_filled(
                    button,
                    theme::radius::MEDIUM,
                    palette.control.lerp_to_gamma(palette.control_hover, hover),
                );
                if open || hover > 0.0 {
                    painter.rect_stroke(
                        button,
                        theme::radius::MEDIUM,
                        Stroke::new(
                            1.0,
                            palette
                                .accent
                                .gamma_multiply(if open { 1.0 } else { hover }),
                        ),
                        egui::StrokeKind::Inside,
                    );
                }
                painter.text(
                    pos2(button.left() + 10.0, button.center().y),
                    Align2::LEFT_CENTER,
                    "+ Add track",
                    FontId::proportional(theme::size::BODY),
                    palette.text,
                );
                draw_arrow(
                    &painter,
                    pos2(button.right() - 12.0, button.center().y),
                    1.0,
                    palette.text_secondary,
                );
                egui::Popup::menu(&response)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.set_min_width(button.width());
                        self.add_menu(ui, doc, self.selected.last().copied(), keymap);
                    });
            }
        }

        let modifiers = ui.input(|i| i.modifiers);
        let hearing = lumen_project::solo::hearing(doc.project());
        let mut hot_edge = None;
        // The track each row was laid out for. A header's menu can delete or
        // move tracks partway through drawing them: rows whose track is no
        // longer at its place are skipped for the rest of the frame (the
        // next frame lays them out afresh). Deleting a track from its menu
        // crashed Lumen before (owner, after MT-MIX-3 run 1).
        let laid_out: Vec<Option<Id>> = rows
            .iter()
            .map(|r| match r.kind {
                RowKind::Track(i) => doc.project().tracks.get(i).map(|t| t.id),
                RowKind::Pending(_) | RowKind::Master => None,
            })
            .collect();
        let still = |doc: &Document, index: usize, row: usize| {
            doc.project()
                .tracks
                .get(index)
                .filter(|t| Some(t.id) == laid_out[row])
                .cloned()
        };
        // Each row draws and takes the pointer only inside its zone: the
        // scrolling tracks pass under the pinned returns and master.
        let base_clip = ui.clip_rect();
        let base_painter = painter.clone();
        for (n, &row) in rows.iter().enumerate() {
            let zone = Rect::from_x_y_ranges(
                column.x_range(),
                self.zone_span(&ctx, doc.project(), row.zone, column.y_range()),
            );
            let rect = Rect::from_min_size(
                pos2(column.left(), column.top() + row.y - self.top),
                vec2(column.width(), row.height),
            );
            if rect.bottom() < zone.top() || rect.top() > zone.bottom() {
                continue;
            }
            ui.set_clip_rect(zone.intersect(base_clip));
            let painter = base_painter.with_clip_rect(zone.intersect(base_painter.clip_rect()));
            if row.kind == RowKind::Master {
                self.master_header(ui, doc, rect);
                continue;
            }
            let RowKind::Track(index) = row.kind else {
                // A file still loading: a quiet placeholder header.
                let name = lanes
                    .pending()
                    .get(match row.kind {
                        RowKind::Pending(i) => i,
                        RowKind::Track(_) | RowKind::Master => unreachable!(),
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
            let Some(track) = still(doc, index, n) else {
                continue;
            };
            let id = EguiId::new(("track header", track.id));
            let selected = self.selected.contains(&track.id);
            let hears = hearing.of(track.id);
            let heard = hears.output;
            let group = track.kind == TrackKind::Group;
            let arm = nesting::arm_state(doc.project(), index);
            let inside = lumen_project::groups::span(&doc.project().tracks, index).len() - 1;
            let collapsed = self.collapsed.contains(&track.id);

            // Background: click to select, right-click for the menu.
            // Click to select; drag to move (the selection, or this track).
            let background = ui.interact(rect, id, Sense::click_and_drag());
            background.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    true,
                    selected,
                    group_description(&track, inside, collapsed),
                )
            });
            if background.clicked() {
                self.click_select(doc.project(), track.id, modifiers);
            }
            // Returns stay pinned in their own order (reordering them waits;
            // KNOWN_ISSUES).
            if background.drag_started() && track.kind != TrackKind::Return {
                self.start_drag(doc.project(), track.id);
            }
            background.context_menu(|ui| {
                if !self.selected.contains(&track.id) {
                    self.selected = vec![track.id];
                }
                self.track_menu(ui, doc, track.id, keymap);
                self.renaming_in_strip = false;
            });
            let fill = if selected {
                palette.selected_row
            } else {
                palette.header
            };
            let look = theme::controls(selected);
            painter.rect_filled(rect, 0.0, fill);
            // The rails: the groups it is in, then its own, the track
            // colour, or red with a glow into the header while armed.
            let left = nesting::draw_rails(&painter, rect, doc.project(), &track, arm.on());
            if arm.on() {
                gradient(
                    &painter,
                    Rect::from_min_size(pos2(left, rect.top()), vec2(28.0, rect.height())),
                    palette.record.gamma_multiply(0.30),
                    Color32::TRANSPARENT,
                );
            }

            // Top row: fold arrow, name, M S ●. A group's arrow hides or
            // shows its tracks (Alt+click: every group's); a track's
            // unfolds it.
            let mid = rect.top() + FOLDED / 2.0;
            let open = if group {
                !collapsed
            } else {
                self.expanded.contains(&track.id)
            };
            let unfolded = motion::glide(&ctx, id.with("fold"), if open { 1.0 } else { 0.0 }, true);
            let arrow = Rect::from_center_size(pos2(left + ARROW_X, mid), vec2(16.0, 18.0));
            let fold = ui
                .interact(arrow, id.with("fold button"), Sense::click())
                .on_hover_text(match (group, open) {
                    (true, true) => "Hide the group's tracks (Alt+click: every group's)",
                    (true, false) => "Show the group's tracks (Alt+click: every group's)",
                    (false, true) => "Fold the track",
                    (false, false) => "Unfold the track (volume and pan; more detail in the lane)",
                });
            fold.widget_info(|| {
                if group {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Checkbox,
                        true,
                        collapsed,
                        format!("Fold group {}", track.name),
                    )
                } else {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        format!("Fold {}", track.name),
                    )
                }
            });
            if fold.clicked() {
                if !group {
                    self.toggle_fold(track.id);
                } else if modifiers.alt {
                    self.collapse_all(doc.project(), !collapsed);
                } else {
                    self.toggle_collapsed(doc.project(), track.id);
                }
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

            // M, S and arm sit evenly spaced, snapped to whole pixels
            // together: egui snaps rectangles but not circles, so an
            // unsnapped arm button sat up to half a pixel off M and S, and
            // its ring off its dot (owner, after slice 4a). The armed glow
            // reaches `ARM_GLOW` beyond the button, inside the gap.
            let ppp = ui.ctx().pixels_per_point();
            let button = |x: f32| {
                Rect::from_min_size(pos2(x, mid - BUTTON / 2.0), vec2(BUTTON, BUTTON))
                    .round_to_pixels(ppp)
            };
            let arm_rect = button(rect.right() - 10.0 - BUTTON);
            let solo_rect = button(arm_rect.left() - BUTTON_GAP - BUTTON);
            let mute_rect = button(solo_rect.left() - BUTTON_GAP - BUTTON);

            // The track type, then the name, with room after the arrow.
            draw_type_icon(
                &painter,
                pos2(left + ICON_X, mid),
                track.kind,
                if heard {
                    palette.text_secondary
                } else {
                    palette.text_hint
                },
            );
            let kind_hover = Rect::from_center_size(pos2(left + ICON_X, mid), vec2(16.0, 16.0));
            let kind = ui
                .interact(kind_hover, id.with("kind"), Sense::hover())
                .on_hover_text(kind_name(track.kind));
            kind.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, kind_name(track.kind))
            });
            let mut name_rect = Rect::from_min_max(
                pos2(left + NAME_X, mid - 10.0),
                pos2(mute_rect.left() - 6.0, mid + 10.0),
            );
            // A return's letter (A, B, C), in its colour: its sends carry
            // the same letter and colour (slice 4).
            if track.kind == TrackKind::Return
                && let Some(letter) = super::pinned::letter_of(doc.project(), track.id)
            {
                let badge = Rect::from_min_size(
                    pos2(name_rect.left(), mid - 8.0),
                    vec2(16.0_f32.max(9.0 * letter.len() as f32 + 7.0), 16.0),
                )
                .round_to_pixels(ui.ctx().pixels_per_point());
                let fill = super::color_of(track.color);
                painter.rect_filled(badge, theme::radius::SMALL, fill);
                painter.text(
                    badge.center(),
                    Align2::CENTER_CENTER,
                    &letter,
                    fonts::medium(theme::size::CAPTION),
                    theme::text_on(fill),
                );
                name_rect.min.x = badge.right() + 6.0;
            }
            // The name is on top of the header's background: it gets the
            // same menu, so a right-click on it opens the track's menu too.
            let count = (group && collapsed).then_some(inside);
            if let Some(name) = self.name(ui, doc, &track, name_rect, (heard, count), (id, false)) {
                let keys = keymap.keys(Command::Rename);
                let name = widgets::tip(name, "Double-click to rename", keys.as_ref(), None);
                name.context_menu(|ui| {
                    if !self.selected.contains(&track.id) {
                        self.selected = vec![track.id];
                    }
                    self.track_menu(ui, doc, track.id, keymap);
                    self.renaming_in_strip = false;
                });
            }

            self.mute_solo(
                ui,
                doc,
                &track,
                hears,
                (mute_rect, solo_rect),
                (id, look, ""),
            );
            // Returns record nothing: no arm button.
            if track.kind != TrackKind::Return
                && arm_button(ui, arm_rect, id, &track.name, arm, look)
            {
                doc.set_armed(&self.targets(track.id), !arm.on());
            }

            // Unfolded: volume and pan on a second row, then the sends.
            if row.height >= FOLDED + 22.0 {
                self.mixer_row(ui, doc, &track, rect, id, look);
            } else {
                // Folded: the level as a hairline (the owner's H2).
                let span = (rect.left() + 30.0, rect.right() - 8.0);
                self.hairline_meter(ui, rect, span, &track, id);
            }
            if row.height >= super::sends::SENDS_FROM && super::sends::has_sends(track.kind) {
                self.sends_row(ui, doc, &track, rect, selected);
            }
            // An unfolded return lists what sends to it (the owner's R2).
            if track.kind == TrackKind::Return && row.height >= FOLDED + 2.0 * STEP {
                self.senders_rows(ui, doc, &track, rect);
            }
            // A return lights with the sends that feed it, and the other
            // way round (the owner's K1).
            if track.kind == TrackKind::Return {
                if background.hovered() {
                    self.hot_return_next = Some(track.id);
                }
                let hot = motion::hover(&ctx, id.with("linked"), self.hot_return == Some(track.id));
                if hot > 0.0 {
                    let color = super::color_of(track.color);
                    painter.rect_filled(rect, 0.0, color.gamma_multiply(0.10 * hot));
                    painter.rect_stroke(
                        rect.shrink(0.5),
                        0.0,
                        Stroke::new(1.0, color.gamma_multiply(0.7 * hot)),
                        egui::StrokeKind::Inside,
                    );
                }
            }

            // Bottom edge: drag to resize.
            let edge = Rect::from_min_max(
                pos2(rect.left(), rect.bottom() - 3.0),
                pos2(rect.right(), rect.bottom() + 3.0),
            );
            let resize = ui
                .interact(edge, id.with("resize"), Sense::drag())
                .on_hover_cursor(CursorIcon::ResizeVertical);
            resize.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Other,
                    true,
                    format!("Resize {}", track.name),
                )
            });
            if resize.drag_started() {
                self.resizing = Some((track.id, row.height));
            }
            if resize.dragged()
                && let Some((rid, start)) = self.resizing
                && rid == track.id
            {
                let total = resize.total_drag_delta().map_or(0.0, |d| d.y);
                let wanted = snap_height((start + total).clamp(FOLDED, MAX_HEIGHT));
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
            // The handle glows while hovered or dragged (owner request).
            let hot = resize.hovered() || self.resizing.is_some_and(|(r, _)| r == track.id);
            let glow = motion::hover(&ctx, id.with("edge glow"), hot);
            if glow > 0.0 {
                hot_edge = Some((track.id, glow));
                draw_edge_glow(&painter, column.x_range(), rect.bottom(), glow);
            }
        }
        ui.set_clip_rect(base_clip);
        self.hot_edge = hot_edge;
        painter.vline(
            column.right() - 0.5,
            column.y_range(),
            Stroke::new(1.0, palette.border),
        );
        // Dragged tracks are dimmed; the line shows where they will land.
        for (n, row) in rows.iter().enumerate() {
            if let RowKind::Track(index) = row.kind
                && still(doc, index, n).is_some_and(|t| self.is_dragged(t.id))
            {
                let rect = Rect::from_min_size(
                    pos2(column.left(), column.top() + row.y - self.top),
                    vec2(column.width(), row.height),
                );
                painter.rect_filled(rect, 0.0, palette.header.gamma_multiply(0.6));
            }
        }
        // A drag follows rows laid out this frame: none if tracks changed.
        let unchanged = rows.iter().enumerate().all(|(n, row)| match row.kind {
            RowKind::Track(index) => still(doc, index, n).is_some(),
            RowKind::Pending(_) | RowKind::Master => true,
        });
        if unchanged {
            self.follow_drag(ui, column, &rows, content, doc);
        }
        self.draw_drop_line(&painter, column, true);
    }

    /// The name, or the rename field. Double-click to rename; Enter or
    /// clicking away keeps the new name, Esc cancels. The name's response,
    /// for its tooltip and menu.
    pub(super) fn name(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &lumen_project::model::Track,
        rect: Rect,
        (heard, count): (bool, Option<usize>),
        (id, in_strip): (EguiId, bool),
    ) -> Option<egui::Response> {
        let palette = theme::p();
        // A group's name is a step bolder (the owner's H1 specimen).
        let font = if track.kind == TrackKind::Group {
            fonts::medium(theme::size::BODY)
        } else {
            FontId::proportional(theme::size::BODY)
        };
        // A folded group says how many tracks it hides, after its name.
        let (rect, count) = match count {
            Some(n) => {
                let count = painter_count(ui, n);
                let width = count.size().x + 6.0;
                (
                    Rect::from_min_max(
                        rect.min,
                        pos2((rect.right() - width).max(rect.left()), rect.bottom()),
                    ),
                    Some((count, width)),
                )
            }
            None => (rect, None),
        };
        if let Some((renaming, text)) = &mut self.renaming
            && *renaming == track.id
            && self.renaming_in_strip == in_strip
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
            return None;
        }
        let response = ui.interact(rect, id.with("name"), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!(
                    "{}{}: double-click to rename",
                    track.name,
                    if in_strip { " in the mixer" } else { "" }
                ),
            )
        });
        if response.double_clicked() {
            self.renaming = Some((track.id, track.name.clone()));
            self.renaming_in_strip = in_strip;
        } else if response.clicked() {
            let modifiers = ui.input(|i| i.modifiers);
            self.click_select(doc.project(), track.id, modifiers);
        }
        let painter = ui.painter().clone();
        let galley = painter.layout(
            track.name.clone(),
            font.clone(),
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
                let g =
                    painter.layout_no_wrap(format!("{name}\u{2026}"), font.clone(), palette.text);
                if g.size().x <= rect.width() {
                    break;
                }
            }
            format!("{name}\u{2026}")
        } else {
            track.name.clone()
        };
        let drawn = painter.text(
            pos2(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            text,
            font,
            if heard {
                palette.text
            } else {
                palette.text_hint
            },
        );
        if let Some((count, _)) = count {
            let at = pos2(drawn.right() + 6.0, rect.center().y - count.size().y / 2.0);
            painter.galley(at, count, palette.text_hint);
        }
        Some(response)
    }

    /// Volume and pan, for an unfolded track. Double-click resets either.
    pub(super) fn mixer_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &lumen_project::model::Track,
        rect: Rect,
        id: EguiId,
        look: theme::Controls,
    ) {
        // One row of sliders that show their value inside (owner request):
        // volume fills from the left; pan fills from the centre and grows
        // more saturated the harder it pans.
        let center_y = rect.top() + FOLDED + STEP / 2.0;
        let left = rect.left() + 30.0;
        let right = rect.right() - 8.0;
        let pan_rect = Rect::from_min_max(
            pos2(right - 54.0, center_y - 10.0),
            pos2(right, center_y + 10.0),
        );
        let volume_rect = Rect::from_min_max(
            pos2(left, center_y - 10.0),
            pos2(pan_rect.left() - 6.0, center_y + 10.0),
        );
        let db = value_slider(
            ui,
            volume_rect,
            id.with("volume"),
            track.gain_db,
            SliderKind::Volume,
            &format!("Volume {}", track.name),
            look,
        );
        if db != track.gain_db {
            doc.set_volume(track.id, db);
        }
        // The level under the value that sets it (the owner's H2).
        self.slider_meter(ui, volume_rect, track, id);
        let pan = value_slider(
            ui,
            pan_rect,
            id.with("pan"),
            track.pan,
            SliderKind::Pan,
            &format!("Pan {}", track.name),
            look,
        );
        if pan != track.pan {
            doc.set_pan(track.id, pan);
        }
    }
}

/// "· 4": how many tracks a folded group hides.
fn painter_count(ui: &Ui, n: usize) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout_no_wrap(
        format!("\u{00B7} {n}"),
        FontId::proportional(theme::size::BODY),
        theme::p().text_hint,
    )
}

/// The record-arm button: round, with a red dot that lights when armed. A
/// group's lights when everything inside it is armed, and shows half a ring
/// when only some is.
pub(super) fn arm_button(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    name: &str,
    arm: Arm,
    look: theme::Controls,
) -> bool {
    let armed = arm.on();
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
    // Every circle shares this exact centre (owner: the ring must sit
    // centred on the dot).
    let center = rect.center();
    let radius = rect.width() / 2.0;
    if armed {
        // The one deliberate loop in Lumen (owner's choice): while armed, a
        // ring breathes slowly around the button. Reduce motion holds it
        // still; frames are requested only while a track is armed.
        let breath = if motion::reduced() {
            0.5
        } else {
            let t = ui.input(|i| i.time) as f32;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
            0.5 - 0.5 * (t * std::f32::consts::TAU / ARM_BREATH).cos()
        };
        for (spread, alpha) in [(ARM_GLOW, 0.10), (ARM_GLOW * 0.7, 0.18)] {
            p.circle_filled(
                center,
                radius + spread * (0.6 + 0.4 * breath),
                palette.record.gamma_multiply(alpha * (0.6 + 0.4 * breath)),
            );
        }
        p.circle_stroke(
            center,
            radius + 1.0 + breath,
            Stroke::new(1.5, palette.record.gamma_multiply(0.45 + 0.3 * breath)),
        );
        p.circle_filled(center, radius, palette.record.gamma_multiply(0.25));
        p.circle_filled(center, 4.5, palette.record);
    } else {
        p.circle_filled(center, radius, look.fill.lerp_to_gamma(look.hover, hover));
        if arm == Arm::Partly {
            // Some of the group armed: a lit dot and half a ring, still.
            p.add(egui::Shape::line(
                half_ring(center, radius + 1.0),
                Stroke::new(1.5, palette.record.gamma_multiply(0.6)),
            ));
            p.circle_filled(center, 4.5, palette.record);
        } else {
            p.circle_filled(
                center,
                4.5,
                palette.record.gamma_multiply(0.45 + 0.25 * hover),
            );
        }
    }
    response.clicked()
}

/// The right half of a ring around `center`.
fn half_ring(center: egui::Pos2, radius: f32) -> Vec<egui::Pos2> {
    use std::f32::consts::PI;
    (0..=16)
        .map(|i| {
            let a = -PI / 2.0 + PI * i as f32 / 16.0;
            pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

/// Seconds per breath of the armed ring.
const ARM_BREATH: f32 = 2.4;

/// How far the armed glow reaches beyond the button, at its widest.
const ARM_GLOW: f32 = 4.0;

/// The space between M, S and the arm button; wider than the glow, so it
/// never touches S.
const BUTTON_GAP: f32 = 8.0;

/// What a screen reader says for a header; a group adds how many tracks
/// it holds and whether they are hidden: "Track Drums, Group track, 3
/// tracks inside, folded".
fn group_description(
    track: &lumen_project::model::Track,
    inside: usize,
    collapsed: bool,
) -> String {
    let mut text = track_description(track);
    if track.kind == TrackKind::Group {
        text.push_str(&format!(
            ", {inside} track{} inside",
            if inside == 1 { "" } else { "s" }
        ));
        if collapsed {
            text.push_str(", folded");
        }
    }
    text
}

/// What a screen reader says for a track header: "Track Kick, Audio
/// track, muted, armed for recording".
pub(super) fn track_description(track: &lumen_project::model::Track) -> String {
    let mut parts = vec![
        format!("Track {}", track.name),
        kind_name(track.kind).to_owned(),
    ];
    if track.mute {
        parts.push("muted".into());
    }
    if track.solo {
        parts.push("soloed".into());
    }
    if track.armed {
        parts.push("armed for recording".into());
    }
    parts.join(", ")
}

/// The track type, for the header icon's tooltip.
fn kind_name(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Audio => "Audio track",
        TrackKind::Midi => "MIDI track",
        TrackKind::Group => "Group track",
        TrackKind::Return => "Return track",
        TrackKind::Master => "Master track",
    }
}
