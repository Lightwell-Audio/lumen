//! One track's header (M3 slice 4a, groups in M4 slice 3, sends and
//! returns in slice 4; redrawn in M9 slice R1b, ADR-0101): the background
//! that selects and drags, the rails and level strip, the fold button and
//! name, the well of M S and arm, the volume and pan row, the send well
//! and input, a return's senders, and the edge that resizes. Split from `headers.rs` (M4 review M1); the column around the
//! rows stays there.

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, CursorIcon, Id as EguiId, Modifiers, Painter, Rect, Sense, Stroke, Ui,
    pos2, vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{Id, Track, TrackKind};
use lumen_project::solo::Heard;

use super::buttons::Style;
use super::headers::{
    CONTENT_RIGHT, CONTENT_X, FOLD, GAP, SILENCED, arm_button, columns, group_description,
    kind_name, name_center,
};
use super::nesting;
use super::{Arrangement, FOLD_BELOW, FOLDED, MAX_HEIGHT, snap_height};
use crate::commands::{Command, Keymap};
use crate::document::Document;
use crate::icons::draw_type_icon;
use crate::widgets::{self, draw_arrow, draw_edge_glow, gradient, raised};
use crate::{fonts, motion, theme};

/// The armed wash: this strong at the rail, fading across this share of
/// the header (the design system's `.is-armed`).
const ARMED_WASH: f32 = 0.10;
const ARMED_WASH_SPAN: f32 = 0.55;

/// The pieces of the M S ● well: their width, and the gap between them.
const PIECE_W: f32 = theme::size::WELL_PIECE_W;
const PIECE_GAP: f32 = 1.0;

/// The well holding `pieces` pieces, its right edge at `right` and centred
/// on `mid`: 60 pt for M S ●, 41 pt for a return's M S.
fn well_rect(right: f32, mid: f32, pieces: usize) -> Rect {
    let n = pieces as f32;
    let width = 2.0 * theme::space::XXS + n * PIECE_W + (n - 1.0) * PIECE_GAP;
    Rect::from_min_max(
        pos2(right - width, mid - theme::size::CONTROL_SMALL / 2.0),
        pos2(right, mid + theme::size::CONTROL_SMALL / 2.0),
    )
}

/// Where a header row is, and what it shows.
pub(super) struct HeaderRow<'a> {
    pub track: Arc<Track>,
    /// The track's index in the project.
    pub index: usize,
    pub rect: Rect,
    /// The header column.
    pub column: Rect,
    pub modifiers: Modifiers,
    pub hearing: &'a Heard,
}

impl Arrangement {
    /// A device card carried over a header: the track takes it when let go
    /// (`alt`: a copy). The result is left for the app to follow.
    pub(super) fn device_drop_target(
        &mut self,
        ctx: &egui::Context,
        painter: &Painter,
        doc: &mut Document,
        rect: Rect,
        to: Id,
        alt: bool,
    ) {
        let Some(card) = egui::DragAndDrop::payload::<crate::chain::CardDrag>(ctx) else {
            return;
        };
        if !ctx.pointer_latest_pos().is_some_and(|p| rect.contains(p)) {
            return;
        }
        painter.rect_stroke(
            rect.shrink(1.0),
            0.0,
            Stroke::new(theme::stroke::LIT, theme::p().accent),
            egui::StrokeKind::Inside,
        );
        if ctx.input(|i| i.pointer.any_released()) {
            let slot = crate::document::Slot::End;
            let result = if alt {
                doc.copy_device(card.track, card.device, to, slot)
            } else {
                doc.move_device(card.track, card.device, to, slot)
                    .map(|()| card.device)
            };
            egui::DragAndDrop::clear_payload(ctx);
            self.device_drop = Some(super::DeviceDrop { to, result });
        }
    }

    /// A device carried from the browser over a track's header or lane: the
    /// track lights, and when it is let go the app puts the device there.
    pub(super) fn browser_drop_target(
        &mut self,
        ctx: &egui::Context,
        painter: &Painter,
        rect: Rect,
        to: Id,
    ) {
        let Some(device) = egui::DragAndDrop::payload::<crate::chain::Dragged>(ctx) else {
            return;
        };
        if !ctx.pointer_latest_pos().is_some_and(|p| rect.contains(p)) {
            return;
        }
        painter.rect_stroke(
            rect.shrink(1.0),
            0.0,
            Stroke::new(theme::stroke::LIT, theme::p().accent),
            egui::StrokeKind::Inside,
        );
        if ctx.input(|i| i.pointer.any_released()) {
            self.browser_drop = Some((device.0.clone(), to));
            egui::DragAndDrop::clear_payload(ctx);
        }
    }

    /// Draws one track's header in its zone (`painter` is clipped to it).
    /// Returns the track's bottom edge and how strongly it glows, while it
    /// does, so the lane can continue the glow.
    pub(super) fn track_header(
        &mut self,
        ui: &mut Ui,
        painter: &Painter,
        doc: &mut Document,
        keymap: &Keymap,
        at: HeaderRow<'_>,
    ) -> Option<(Id, f32)> {
        let HeaderRow {
            track,
            index,
            rect,
            column,
            modifiers,
            hearing,
        } = at;
        let ctx = ui.ctx().clone();
        let palette = *theme::p();
        let painter = painter.clone();
        let row_height = rect.height();
        let id = EguiId::new(("track header", track.id));
        let selected = self.selected.contains(&track.id);
        let hears = hearing.of(track.id);
        let heard = hears.output;
        let group = track.kind == TrackKind::Group;
        let arm = nesting::arm_look(doc.project(), index, &self.selected);
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
        self.device_drop_target(&ctx, &painter, doc, rect, track.id, modifiers.alt);
        self.browser_drop_target(&ctx, &painter, rect, track.id);
        // Returns stay pinned in their own order (reordering them waits;
        // KNOWN_ISSUES).
        if background.drag_started() && track.kind != TrackKind::Return {
            self.start_drag(doc.project(), track.id, false);
        }
        widgets::Menu::context(&background).show(|ui| {
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
            // The armed wash: `record` at 10%, fading out across the
            // header's left 55% (the design system's `.is-armed`).
            gradient(
                &painter,
                Rect::from_min_size(
                    pos2(left, rect.top()),
                    vec2((rect.right() - left) * ARMED_WASH_SPAN, rect.height()),
                ),
                palette.record.gamma_multiply(ARMED_WASH),
                Color32::TRANSPARENT,
            );
        }

        // The name row: the fold button, the name, the monitor badge and
        // the well of M S ●. It sits in the middle of a folded row and
        // moves up to the first unfolded row as the track opens (ADR-0101).
        let mid = name_center(rect.top(), row_height);
        let ppp = ui.ctx().pixels_per_point();
        let open = if group {
            !collapsed
        } else {
            self.expanded.contains(&track.id)
        };
        let unfolded = motion::glide(&ctx, id.with("fold"), if open { 1.0 } else { 0.0 }, true);
        let note = self.output_notes.get(&track.id).cloned();
        let kind_words = match &note {
            Some(note) => format!("{} \u{b7} {}", kind_name(track.kind), note.text),
            None => kind_name(track.kind).to_owned(),
        };
        // The type icon is the fold button: it turns into the arrow under
        // the pointer (a group always shows its arrow).
        let fold_rect =
            Rect::from_min_size(pos2(left + CONTENT_X, mid - FOLD / 2.0), vec2(FOLD, FOLD))
                .round_to_pixels(ppp);
        let fold = ui
            .interact(fold_rect, id.with("fold button"), Sense::click())
            .on_hover_text(format!(
                "{kind_words} \u{b7} {}",
                match (group, open) {
                    (true, true) => "hide the group's tracks (Alt+click: every group's)",
                    (true, false) => "show the group's tracks (Alt+click: every group's)",
                    (false, true) => "fold the track",
                    (false, false) => "unfold the track (volume, pan and sends)",
                }
            ));
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
        let pointed = group || fold.has_focus() || ui.rect_contains_pointer(rect);
        let arrow = motion::hover(&ctx, id.with("fold shows arrow"), pointed);
        let ink = if fold.hovered() {
            palette.text
        } else if heard {
            palette.text_secondary
        } else {
            // Silenced: dimmed to 45% (the design system's `.is-dim`).
            palette.text_secondary.gamma_multiply(SILENCED)
        };
        if arrow < 1.0 {
            draw_type_icon(
                &painter,
                fold_rect.center(),
                track.kind,
                ink.gamma_multiply(1.0 - arrow),
            );
        }
        if arrow > 0.0 {
            draw_arrow(
                &painter,
                fold_rect.center(),
                unfolded,
                ink.gamma_multiply(arrow),
            );
        }
        raised::focus_ring(&painter, &fold, fold_rect);

        // M, S and arm: pieces of one sunk well at the right (a return's
        // has M and S only), snapped to whole pixels together.
        let pieces = if track.kind == TrackKind::Return {
            2
        } else {
            3
        };
        let trough = well_rect(rect.right() - CONTENT_RIGHT, mid, pieces).round_to_pixels(ppp);
        raised::paint_sunken(&painter, trough, theme::radius::SMALL);
        let piece = |n: usize| {
            Rect::from_min_size(
                pos2(
                    trough.left() + theme::space::XXS + n as f32 * (PIECE_W + PIECE_GAP),
                    trough.top() + theme::space::XXS,
                ),
                vec2(PIECE_W, theme::size::WELL_PIECE_H),
            )
            .round_to_pixels(ppp)
        };
        let (mute_rect, solo_rect, arm_rect) = (piece(0), piece(1), piece(2));

        let before = trough.left();
        // A track whose Monitor is not Auto shows a badge before that.
        let silent = note.as_ref().filter(|n| n.warn).map(|n| n.text.clone());
        let badge = (super::monitor_badge::shown(&track) || silent.is_some()).then(|| {
            Rect::from_center_size(
                pos2(before - GAP - theme::size::BADGE_W / 2.0, mid),
                vec2(theme::size::BADGE_W, theme::size::BADGE_H),
            )
            .round_to_pixels(ppp)
        });
        let mut name_rect = Rect::from_min_max(
            pos2(fold_rect.right() + GAP, mid - 10.0),
            pos2(badge.map_or(before, |b| b.left()) - GAP, mid + 10.0),
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
            .round_to_pixels(ppp);
            let fill = super::color_of(track.color);
            painter.rect_filled(badge, theme::radius::SMALL, fill);
            painter.text(
                badge.center(),
                Align2::CENTER_CENTER,
                &letter,
                fonts::medium(theme::size::CAPTION),
                theme::text_on(fill),
            );
            name_rect.min.x = badge.right() + GAP;
        }
        // The name is on top of the header's background: it gets the
        // same menu, so a right-click on it opens the track's menu too.
        let count = (group && collapsed).then_some(inside);
        if let Some(name) = self.name(ui, doc, &track, name_rect, (heard, count), (id, false)) {
            let keys = keymap.keys(Command::Rename);
            let name = widgets::tip(name, "Double-click to rename", keys.as_ref(), None);
            widgets::Menu::context(&name).show(|ui| {
                if !self.selected.contains(&track.id) {
                    self.selected = vec![track.id];
                }
                self.track_menu(ui, doc, track.id, keymap);
                self.renaming_in_strip = false;
            });
        }

        if let Some(badge) = badge {
            if let Some(why) = &silent {
                // The track plays a plugin's output and the plugin is not
                // running: it is silent, and this says why.
                painter.text(
                    badge.center(),
                    Align2::CENTER_CENTER,
                    "\u{25B2}",
                    fonts::medium(theme::size::CAPTION),
                    palette.warning,
                );
                let warn = ui
                    .interact(badge, id.with("output note"), Sense::hover())
                    .on_hover_text(why);
                warn.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, why.clone())
                });
            } else if let Some(next) = super::monitor_badge::badge(ui, badge, id, &track, look) {
                doc.set_monitor(&self.targets(track.id), next);
            }
        }
        self.mute_solo(
            ui,
            doc,
            &track,
            hears,
            (mute_rect, solo_rect),
            (id, look, "", Style::Well),
        );
        // Returns record nothing: no arm piece.
        if track.kind != TrackKind::Return
            && arm_button(ui, arm_rect, id, &track.name, arm, (look, true))
        {
            self.press_arm(ui, doc, track.id, arm.on());
        }

        // The level rises beside the rail, folded or not (ADR-0101).
        self.level_strip(ui, rect, left, &track, (id, heard));
        // Unfolded: volume and pan, then the sends and the input (or a
        // return's senders) on the third row.
        if row_height >= FOLDED + 22.0 {
            self.mixer_row(ui, doc, &track, (rect, left), (id, look));
            let (_, _, third) = columns(rect, left);
            if super::sends::has_sends(track.kind) {
                self.send_well(ui, doc, &track, third, selected);
            }
            // An unfolded return lists what sends to it (the owner's R2).
            if track.kind == TrackKind::Return {
                self.senders_rows(ui, doc, &track, (rect, third));
            }
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
            self.resizing = Some((track.id, row_height));
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
            draw_edge_glow(&painter, column.x_range(), rect.bottom(), glow);
        }
        (glow > 0.0).then_some((track.id, glow))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_well_holds_m_s_and_arm_or_a_returns_m_and_s() {
        let well = well_rect(196.0, 22.0, 3);
        assert_eq!(well.width(), 60.0);
        assert_eq!(well.right(), 196.0);
        assert_eq!((well.top(), well.bottom()), (12.0, 32.0));
        assert_eq!(well_rect(196.0, 22.0, 2).width(), 41.0);
    }
}
