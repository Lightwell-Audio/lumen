//! The track header column (M3 slice 4a; redrawn in M9 slice R1b,
//! ADR-0101): rail and level strip, the fold button, name, the well of M S
//! and arm; an unfolded track's rows and columns; resizing. Design: DESIGN
//! §7a, ADR-0025. Groups step their tracks in
//! (`nesting`, M4 slice 3); the menus are in `menus`; each track's row is
//! drawn by `header_row`.

use std::sync::Arc;

use eframe::egui::{self, Align2, FontId, Id as EguiId, Key, Rect, Sense, Stroke, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_project::automation::Target;
use lumen_project::model::{Id, TrackKind};

use super::control_menu::{control_menu_for, waiting_ring};
use super::follow;
use super::header_row::HeaderRow;
use super::menus::AddMenu;
use super::nesting::Arm;
use super::{Arrangement, FOLDED, RowKind, Zone};
use crate::commands::{Command, Keymap};
use crate::document::Document;
use crate::lanes::Lanes;
use crate::widgets::{self, SliderKind, draw_arrow, value_slider};

/// A header's content starts this far right of its rail's left edge (after
/// the rail and the level strip beside it, ADR-0101), and ends this far
/// inside the header's right edge.
pub(super) const CONTENT_X: f32 = 12.0;
pub(super) const CONTENT_RIGHT: f32 = 8.0;
/// The fold button (the type icon, an arrow under the pointer), and the
/// gap after it and between the parts of a row.
pub(super) const FOLD: f32 = 16.0;
pub(super) const GAP: f32 = 6.0;

/// An unfolded header's rows: 8 pt above the first, 6 pt between (and 8 pt
/// under the last: the owner's even spacing, ADR-0101).
const ROW_TOP: f32 = 8.0;
const ROW: f32 = theme::size::CONTROL_SMALL;

/// The centre of row `n` (from 0) of an unfolded header whose top is `top`.
pub(super) fn row_center(top: f32, n: usize) -> f32 {
    top + ROW_TOP + n as f32 * (ROW + GAP) + ROW / 2.0
}

/// Where the name row sits in a header `height` tall whose top is `top`: in
/// the middle of a folded row, and moving up to the first unfolded row's
/// centre as the track opens (the move follows the height as it glides).
pub(super) fn name_center(top: f32, height: f32) -> f32 {
    let open = ((height - FOLDED) / (super::UNFOLDED - FOLDED)).clamp(0.0, 1.0);
    let folded = top + FOLDED / 2.0;
    folded + (row_center(top, 0) - folded) * open
}

/// How much of its strength a silenced track's name and type icon keep
/// (the design system's `.is-dim`).
pub(super) const SILENCED: f32 = 0.45;
use crate::theme::Tone;
use crate::widgets::raised;
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
        let mut rows = self.rows(&ctx, doc.project(), lanes.pending().len());
        let content = self.content_height(&ctx, doc.project(), &rows);
        if self.keep_in_content(content, column.height()) {
            rows = self.rows(&ctx, doc.project(), lanes.pending().len());
        }
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
            self.empty_area(ui, &painter, doc, keymap, (below, main.max));
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
        let mut edges = Vec::new();
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
            // A row folding shut shows only what fits in it (M4.5).
            let zone = if row.clipped {
                zone.intersect(rect)
            } else {
                zone
            };
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
            self.track_header(
                ui,
                &painter,
                doc,
                keymap,
                HeaderRow {
                    track: Arc::clone(&track),
                    index,
                    rect,
                    column,
                    modifiers,
                    hearing: &hearing,
                },
            );
            edges.push((track, rect, zone));
        }
        // The resize edges go over every row, so the band below a row's line
        // is not taken by the next row's header.
        for (track, rect, zone) in edges {
            ui.set_clip_rect(zone.intersect(base_clip));
            let painter = base_painter.with_clip_rect(zone.intersect(base_painter.clip_rect()));
            if let Some(glow) = self.resize_edge(ui, &painter, &track, (rect, column)) {
                hot_edge = Some(glow);
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

    /// The empty area below the last track, `below`: double-click adds an
    /// audio track, right-click any kind, and the "+ Add track ▾" button
    /// (owner's choice, option B). The scrolling tracks end at `bottom`.
    fn empty_area(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        doc: &mut Document,
        keymap: &Keymap,
        (below, bottom): (Rect, f32),
    ) {
        let ctx = ui.ctx().clone();
        let palette = *theme::p();
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
        widgets::Menu::context(&empty)
            .show(|ui| self.add_menu(ui, doc, None, keymap, AddMenu::Context));

        // "+ Add track ▾": a menu of every track type with its shortcut,
        // and a hint that double-clicking here adds an audio track
        // (owner's choice, option B).
        let button =
            Rect::from_min_size(below.min + vec2(8.0, 8.0), vec2(below.width() - 16.0, 26.0));
        if button.bottom() < bottom {
            let response = ui
                .interact(button, ui.id().with("add track"), Sense::click())
                .on_hover_text("Add an audio, MIDI, group or return track");
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add track")
            });
            let hover = motion::hover(&ctx, ui.id().with("add track hover"), response.hovered());
            let open = egui::Popup::is_id_open(&ctx, egui::Popup::default_response_id(&response));
            // A raised menu button (ADR-0052): on while its menu is open.
            let face = raised::Face::new(theme::controls(false))
                .hover(hover)
                .pressed(response.is_pointer_button_down_on())
                .lit(if open { 1.0 } else { 0.0 }, Tone::On);
            let button_rect = button.round_to_pixels(ctx.pixels_per_point());
            raised::paint(
                painter,
                button_rect,
                theme::radius::for_height(button_rect.height()),
                &face,
            );
            raised::focus_ring(painter, &response, button_rect);
            painter.text(
                pos2(button.left() + 10.0, button.center().y),
                Align2::LEFT_CENTER,
                "+ Add track",
                FontId::proportional(theme::size::BODY),
                palette
                    .text
                    .lerp_to_gamma(theme::lit(Tone::On).text, face.lit),
            );
            draw_arrow(
                painter,
                pos2(button.right() - 12.0, button.center().y),
                1.0,
                palette.text_secondary,
            );
            widgets::Menu::below(&response).show(|ui| {
                ui.set_min_width(button.width());
                self.add_menu(
                    ui,
                    doc,
                    self.selected.last().copied(),
                    keymap,
                    AddMenu::Button,
                );
            });
        }
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
        // A track that listens keeps room for its light (the header's
        // name; the strip has none).
        let rect = if !in_strip && self.live.listens(track, heard) {
            Rect::from_min_max(
                rect.min,
                pos2(
                    (rect.right() - super::listen_light::ROOM).max(rect.left()),
                    rect.bottom(),
                ),
            )
        } else {
            rect
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
        // A frozen track keeps room for its badge (the strip has none).
        let frozen = track.frozen.is_some() && !in_strip;
        let (rect, badge_row) = if frozen {
            let room = super::frozen_badge::room(ui, rect.width());
            (
                Rect::from_min_max(
                    rect.min,
                    pos2((rect.right() - room).max(rect.left()), rect.bottom()),
                ),
                Some(rect),
            )
        } else {
            (rect, None)
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
        // A track that is silenced dims its name (the design system's
        // `.is-dim`: 45%).
        let name_ink = if heard {
            palette.text
        } else {
            palette.text.gamma_multiply(SILENCED)
        };
        let galley = painter.layout(track.name.clone(), font.clone(), name_ink, rect.width());
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
            name_ink,
        );
        if !in_strip {
            self.live.paint(ui, track, heard, drawn);
        }
        if let Some((count, _)) = count {
            let at = pos2(drawn.right() + 6.0, rect.center().y - count.size().y / 2.0);
            painter.galley(at, count, palette.text_hint);
        }
        if let Some(row) = badge_row
            && super::frozen_badge::badge(ui, row, id, track, self.selected.contains(&track.id))
        {
            self.selected = vec![track.id];
            self.track_command = Some(crate::commands::Command::UnfreezeTracks);
        }
        Some(response)
    }

    /// The second row of an unfolded track: volume, then pan in the right
    /// column; and on an audio track the In chip under pan (the owner's
    /// B·1, ADR-0101). `left` is where the header's rail starts. Both
    /// sliders show their value inside (owner request): volume fills from
    /// the left; pan fills from the centre and grows more saturated the
    /// harder it pans. Double-click resets either.
    pub(super) fn mixer_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &lumen_project::model::Track,
        (rect, left): (Rect, f32),
        (id, look): (EguiId, theme::Controls),
    ) {
        let second = row_center(rect.top(), 1);
        let (start, side, _) = columns(rect, left);
        let right = rect.right() - CONTENT_RIGHT;
        let volume_rect = Rect::from_min_max(
            pos2(start, second - ROW / 2.0),
            pos2(side - GAP, second + ROW / 2.0),
        );
        let pan_rect = Rect::from_min_max(
            pos2(side, second - ROW / 2.0),
            pos2(right, second + ROW / 2.0),
        );
        if track.kind == lumen_project::model::TrackKind::Audio {
            let third = row_center(rect.top(), 2);
            let chip = Rect::from_min_max(
                pos2(side, third - ROW / 2.0),
                pos2(right, third + ROW / 2.0),
            );
            self.input_chip(ui, doc, track, chip, id);
        }
        let gain = self.shown(doc, track, Target::Volume, track.gain_db);
        let db = widgets::value_slider_styled(
            ui,
            volume_rect,
            id.with("volume"),
            gain,
            SliderKind::Volume,
            &format!("Volume {}", track.name),
            widgets::SliderStyle { look, comet: false },
        );
        if db != gain {
            doc.set_volume(track.id, db);
        }
        if self.taken_over(doc, track, Target::Volume) {
            follow::ring(ui, volume_rect);
        }
        control_menu_for(
            ui,
            id.with("volume"),
            doc,
            &mut self.auto,
            track.id,
            Target::Volume,
        );
        waiting_ring(ui, &self.auto, volume_rect, track.id, Target::Volume);
        let shown_pan = self.shown(doc, track, Target::Pan, track.pan);
        let pan = value_slider(
            ui,
            pan_rect,
            id.with("pan"),
            shown_pan,
            SliderKind::Pan,
            &format!("Pan {}", track.name),
            look,
        );
        if pan != shown_pan {
            doc.set_pan(track.id, pan);
        }
        if self.taken_over(doc, track, Target::Pan) {
            follow::ring(ui, pan_rect);
        }
        control_menu_for(
            ui,
            id.with("pan"),
            doc,
            &mut self.auto,
            track.id,
            Target::Pan,
        );
        waiting_ring(ui, &self.auto, pan_rect, track.id, Target::Pan);
    }
}

/// An unfolded header's columns, for a header in `rect` whose rail starts at
/// `left`: where the left column starts, where the right column (pan, the
/// In chip) starts, and the third row's left column (the sends, a return's
/// senders), which ends a gap before the right column.
pub(super) fn columns(rect: Rect, left: f32) -> (f32, f32, Rect) {
    let start = left + CONTENT_X;
    let side = rect.right() - CONTENT_RIGHT - theme::size::HEADER_SIDE;
    let third = row_center(rect.top(), 2);
    let sends = Rect::from_min_max(
        pos2(start, third - ROW / 2.0),
        pos2((side - GAP).max(start), third + ROW / 2.0),
    );
    (start, side, sends)
}

/// "· 4": how many tracks a folded group hides.
fn painter_count(ui: &Ui, n: usize) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout_no_wrap(
        format!("\u{00B7} {n}"),
        FontId::proportional(theme::size::BODY),
        theme::p().text_hint,
    )
}

/// The record-arm button: a piece of a well (a header's, ADR-0101, or a
/// mixer strip's), drawn by [`arm_piece`]. At rest a quiet grey dot that
/// turns record red under the pointer; armed, a breathing `record` body and
/// ring (the owner's pulse, kept). A group's lights when everything inside
/// it is armed, and shows the dot with half a ring when only some is.
pub(super) fn arm_button(ui: &mut Ui, rect: Rect, id: EguiId, name: &str, arm: Arm) -> bool {
    let armed = arm.on();
    let response = ui
        .interact(rect, id.with("arm"), Sense::click())
        .on_hover_text(if arm == Arm::Listening {
            "Listening because selected \u{b7} click to arm for recording (Ctrl keeps the other tracks armed)"
        } else {
            "Arm for recording (Ctrl keeps the other tracks armed; MIDI tracks record now, audio recording arrives in Milestone 9)"
        });
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            armed,
            format!("Arm {name}"),
        )
    });
    let hover = motion::hover(ui.ctx(), id.with("arm hover"), response.hovered());
    // Snapped with M and S beside it, so the dot sits on their letters' line.
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    arm_piece(ui, rect, arm, hover);
    if response.has_focus() {
        raised::paint_focus(ui.painter(), rect, crate::widgets::well::PIECE_RADIUS);
    }
    response.clicked()
}

/// The arm piece: armed, a `record` body around 25% with a solid dot, and a
/// ring, the body and a halo round the dot that breathe together, all
/// inside the piece (a glow beyond it would spill over S); some of a group
/// armed, the body, the dot and half a ring, still; listening because
/// selected, a hollow record-red ring; at rest a quiet grey dot. Every
/// circle shares the piece's centre (owner: the ring must sit centred on
/// the dot); unlit pieces take the Ember wash under the pointer.
fn arm_piece(ui: &Ui, rect: Rect, arm: Arm, hover: f32) {
    let palette = theme::p();
    let p = ui.painter();
    let center = rect.center();
    let r = f32::from(crate::widgets::well::PIECE_RADIUS);
    let ring = rect.height() / 2.0 - PIECE_RING_INSET;
    match arm {
        Arm::All => {
            // The body breathes with the ring and a soft halo swells round
            // the dot (the owner: the pulse keeps its personality).
            let breath = breath(ui);
            let swell = PIECE_BODY_LOW + (1.0 - PIECE_BODY_LOW) * 2.0 * breath;
            p.rect_filled(rect, r, palette.record.gamma_multiply(ARM_BODY * swell));
            p.circle_filled(
                center,
                PIECE_DOT + PIECE_HALO * breath,
                palette.record.gamma_multiply(PIECE_HALO_ALPHA * breath),
            );
            p.circle_stroke(
                center,
                ring - 0.5 + 0.5 * breath,
                Stroke::new(
                    theme::stroke::LIT,
                    palette.record.gamma_multiply(0.45 + 0.3 * breath),
                ),
            );
            p.circle_filled(center, PIECE_DOT, palette.record);
        }
        Arm::Partly => {
            p.rect_filled(rect, r, palette.record.gamma_multiply(ARM_BODY));
            p.add(egui::Shape::line(
                half_ring(center, ring),
                Stroke::new(theme::stroke::LIT, palette.record.gamma_multiply(0.6)),
            ));
            p.circle_filled(center, PIECE_DOT, palette.record);
        }
        Arm::Listening => {
            p.rect_filled(rect, r, palette.ember.gamma_multiply(hover));
            p.circle_stroke(
                center,
                PIECE_DOT,
                Stroke::new(theme::stroke::LIT, palette.record),
            );
        }
        Arm::Off => {
            p.rect_filled(rect, r, palette.ember.gamma_multiply(hover));
            let rest = palette.text_secondary.gamma_multiply(ARM_REST_DOT);
            p.circle_filled(center, PIECE_DOT, rest.lerp_to_gamma(palette.record, hover));
        }
    }
}

/// How far the armed ring breathes, 0 to 1. Reduce motion holds it still;
/// frames are requested only while a track is armed.
fn breath(ui: &Ui) -> f32 {
    if motion::reduced() {
        0.5
    } else {
        let t = ui.input(|i| i.time) as f32;
        ui.ctx().request_repaint_after(motion::BREATH_FRAME);
        0.5 - 0.5 * (t * std::f32::consts::TAU / ARM_BREATH).cos()
    }
}

/// The dot of an arm piece (the mock's 8 pt), and how far inside the
/// piece's height its ring sits.
const PIECE_DOT: f32 = 4.0;
const PIECE_RING_INSET: f32 = 2.0;
/// An armed piece's body at the breath's low point, as a share of
/// `ARM_BODY` (it rises as far above at the high point); the halo round
/// its dot at full breath: how far it reaches past the dot, how strong.
const PIECE_BODY_LOW: f32 = 0.7;
const PIECE_HALO: f32 = 1.5;
const PIECE_HALO_ALPHA: f32 = 0.35;

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
const ARM_BREATH: f32 = motion::BREATH;

/// How much `record` tints an armed button's body.
const ARM_BODY: f32 = 0.25;
/// How strong the resting dot's grey is.
const ARM_REST_DOT: f32 = 0.55;

/// What a screen reader says for a header; a group adds how many tracks
/// it holds and whether they are hidden: "Track Drums, Group track, 3
/// tracks inside, folded".
pub(super) fn group_description(
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
pub(super) fn kind_name(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Audio => "Audio track",
        TrackKind::Midi => "MIDI track",
        TrackKind::Group => "Group track",
        TrackKind::Return => "Return track",
        TrackKind::Master => "Master track",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unfolded_track_has_even_margins_around_its_three_rows() {
        // 8 above, three 20 pt rows (the name; volume and pan; the sends
        // and the input) 6 apart, 8 below: four steps.
        assert_eq!(
            ROW_TOP + 3.0 * ROW + 2.0 * GAP + 8.0,
            super::super::UNFOLDED
        );
        assert_eq!(row_center(0.0, 0), 18.0);
        assert_eq!(row_center(0.0, 1), 44.0);
        assert_eq!(row_center(0.0, 2), 70.0);
        // The last row ends 8 pt above the track's bottom.
        assert_eq!(
            super::super::UNFOLDED - (row_center(0.0, 2) + ROW / 2.0),
            8.0
        );
    }

    #[test]
    fn the_name_row_moves_up_as_the_track_opens() {
        assert_eq!(name_center(100.0, FOLDED), 122.0);
        assert_eq!(name_center(100.0, super::super::UNFOLDED), 118.0);
        assert_eq!(name_center(100.0, 300.0), 118.0);
        let half = name_center(100.0, (FOLDED + super::super::UNFOLDED) / 2.0);
        assert!(half > 118.0 && half < 122.0);
    }

    #[test]
    fn the_columns_line_up_at_every_width() {
        for width in [204.0, 280.0, 480.0] {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 88.0));
            let (start, side, sends) = columns(rect, 0.0);
            assert_eq!(start, 12.0);
            assert_eq!(side, width - 8.0 - 60.0);
            assert_eq!(sends.left(), start);
            assert_eq!(sends.right(), side - GAP);
            assert_eq!(sends.center().y, 70.0);
        }
    }
}
