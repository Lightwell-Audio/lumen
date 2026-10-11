//! One track's header (M3 slice 4a, groups in M4 slice 3, sends and
//! returns in slice 4): the background that selects and drags, the rails,
//! the fold arrow, type icon and name, M S and arm, the volume and pan row
//! or the hairline meter, sends, a return's senders, and the edge that
//! resizes. Split from `headers.rs` (M4 review M1); the column around the
//! rows stays there.

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, CursorIcon, Id as EguiId, Modifiers, Painter, Rect, Sense, Stroke, Ui,
    pos2, vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{Id, Track, TrackKind};
use lumen_project::solo::Heard;

use super::headers::{
    ARROW_X, BUTTON_GAP, ICON_X, NAME_X, arm_button, group_description, kind_name,
};
use super::nesting;
use super::{Arrangement, BUTTON, FOLD_BELOW, FOLDED, MAX_HEIGHT, STEP, snap_height};
use crate::commands::{Command, Keymap};
use crate::document::Document;
use crate::icons::draw_type_icon;
use crate::widgets::{self, draw_arrow, draw_edge_glow, gradient};
use crate::{fonts, motion, theme};

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
        // A MIDI track whose Monitor is not Auto shows a badge before M.
        let badge = super::monitor_badge::shown(&track).then(|| {
            Rect::from_center_size(
                pos2(
                    mute_rect.left() - BUTTON_GAP - theme::size::BADGE_W / 2.0,
                    mid,
                ),
                vec2(theme::size::BADGE_W, theme::size::BADGE_H),
            )
            .round_to_pixels(ppp)
        });
        let mut name_rect = Rect::from_min_max(
            pos2(left + NAME_X, mid - 10.0),
            pos2(
                badge.map_or(mute_rect.left(), |b| b.left()) - 6.0,
                mid + 10.0,
            ),
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
            widgets::Menu::context(&name).show(|ui| {
                if !self.selected.contains(&track.id) {
                    self.selected = vec![track.id];
                }
                self.track_menu(ui, doc, track.id, keymap);
                self.renaming_in_strip = false;
            });
        }

        if let Some(badge) = badge
            && let Some(next) = super::monitor_badge::badge(ui, badge, id, &track, look, selected)
        {
            doc.set_monitor(&self.targets(track.id), next);
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
        if track.kind != TrackKind::Return && arm_button(ui, arm_rect, id, &track.name, arm, look) {
            doc.set_armed(&self.targets(track.id), !arm.on());
        }

        // Unfolded: volume and pan on a second row, then the sends.
        if row_height >= FOLDED + 22.0 {
            self.mixer_row(ui, doc, &track, rect, id, look);
        } else {
            // Folded: the level as a hairline (the owner's H2).
            let span = (rect.left() + 30.0, rect.right() - 8.0);
            self.hairline_meter(ui, rect, span, &track, id);
        }
        if row_height >= super::sends::SENDS_FROM && super::sends::has_sends(track.kind) {
            self.sends_row(ui, doc, &track, rect, selected);
        }
        // An unfolded return lists what sends to it (the owner's R2).
        if track.kind == TrackKind::Return && row_height >= FOLDED + 2.0 * STEP {
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
