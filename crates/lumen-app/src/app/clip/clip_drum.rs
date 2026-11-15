use std::sync::Arc;

use eframe::egui::{self, CursorIcon, Rect, Sense, Stroke, Ui, pos2};
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, Id};

use crate::canvas::{self, GridLook, Surface};
use crate::drum_editor::geometry;
use crate::drum_editor::paint::lane_name;
use crate::drum_editor::{Lane, Reorder, View};
use crate::note_editor::NoteEditor;
use crate::note_editor::view::EditorView;
use crate::widgets;
use crate::{App, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Audition(usize),
    Mute(usize),
    Solo(usize),
    StartReorder(usize),
    Length(usize),
}

pub(crate) struct DrumFrame<'a> {
    pub ed: &'a mut NoteEditor,
    pub view: &'a EditorView,
    pub clip: &'a Arc<Clip>,
    pub lanes: &'a [Lane],
    pub map_id: Id,
    pub map_name: &'a str,
    pub sounds: usize,
    pub keys: Rect,
}

impl App {
    pub(crate) fn drum_view(&self, clip: Id) -> Option<View> {
        self.clip_drum_map(clip)?;
        let track = self.track_of_clip(clip)?;
        Some(self.note_editor.drum.view(track))
    }

    pub(crate) fn drum_lanes(&self, clip: Id) -> Option<Vec<Lane>> {
        let map = self.clip_drum_map(clip)?;
        let track = self.track_of_clip(clip)?;
        let (clip, _) = self.editor_clip(clip)?;
        let hide = self.note_editor.drum.hides_unused(track);
        Some(crate::drum_editor::lanes(
            map,
            crate::note_editor::notes_of(&clip),
            hide,
            crate::theme::is_light(),
        ))
    }

    pub(crate) fn set_drum_view(&mut self, view: View) -> bool {
        let mut ed = std::mem::take(&mut self.note_editor);
        let changed = self.set_drum_view_in(&mut ed, view);
        self.note_editor = ed;
        changed
    }

    pub(crate) fn set_drum_view_in(&self, ed: &mut NoteEditor, view: View) -> bool {
        let Some(clip) = ed.clip else {
            return false;
        };
        if self.clip_drum_map(clip).is_none() {
            return false;
        }
        let Some(track) = self.track_of_clip(clip) else {
            return false;
        };
        if ed.drum.view(track) == view {
            return false;
        }
        ed.drum.set_view(track, view);
        ed.framed_for = None;
        true
    }

    pub(crate) fn toggle_hide_unused_lanes(&mut self) -> bool {
        let Some(clip) = self.note_editor.clip else {
            return false;
        };
        let Some(track) = self.track_of_clip(clip) else {
            return false;
        };
        if self.drum_view(clip).is_none() {
            return false;
        }
        self.note_editor.drum.toggle_unused(track);
        self.note_editor.framed_for = None;
        true
    }

    pub(crate) fn hides_unused_lanes(&self) -> bool {
        let Some(clip) = self.note_editor.clip else {
            return false;
        };
        self.track_of_clip(clip)
            .is_some_and(|track| self.note_editor.drum.hides_unused(track))
    }

    pub(crate) fn drum_picked_lane(&self) -> Option<(Id, u8)> {
        let clip = self.note_editor.clip?;
        let track = self.track_of_clip(clip)?;
        if self.note_editor.drum.view(track) != View::Lanes {
            return None;
        }
        let map = self.clip_drum_map(clip)?;
        let (open, _) = self.editor_clip(clip)?;
        let notes = crate::note_editor::notes_of(&open);
        let pitch = crate::note_editor::ui::picked_lane(&self.note_editor, notes, map)?;
        Some((map.id, pitch))
    }

    pub(crate) fn drum_surface(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        surface: Surface,
        look: &GridLook,
        mut frame: DrumFrame<'_>,
    ) {
        let map_id = frame.map_id;
        let sounds = frame.sounds;
        let DrumFrame {
            ed, view, lanes, ..
        } = &mut frame;
        ed.drum.open_map(map_id);
        let picked = self.picked_rows(ed, lanes);
        let painter = ui.painter().clone();
        crate::drum_editor::paint::background(&painter, view, lanes, &picked);
        painter.add(canvas::roll_grid_shape(
            surface,
            view.body,
            ui.pixels_per_point(),
            view.time,
            look,
        ));
        if lanes.is_empty() {
            let (state, can_show) = crate::drum_editor::paint::empty_state(sounds);
            if widgets::empty_state(ui, view.body, &state) && can_show {
                self.toggle_hide_unused_lanes();
            }
        }
        self.drum_names(ui, ctx, frame);
    }

    fn picked_rows(&self, ed: &NoteEditor, lanes: &[Lane]) -> Vec<bool> {
        let mut rows = vec![false; lanes.len()];
        let Some(clip) = ed.clip.and_then(|id| self.editor_clip(id)).map(|(c, _)| c) else {
            return rows;
        };
        let notes = crate::note_editor::notes_of(&clip);
        for &i in ed.picked() {
            if let Some(note) = notes.get(i)
                && let Some(row) = lanes.iter().position(|lane| lane.input == note.pitch())
            {
                rows[row] = true;
            }
        }
        rows
    }

    fn drum_names(&mut self, ui: &mut Ui, ctx: &egui::Context, frame: DrumFrame<'_>) {
        let DrumFrame {
            ed,
            view,
            clip,
            lanes,
            map_id,
            map_name,
            keys,
            ..
        } = frame;
        let painter = ui.painter().clone();
        let p = theme::p();
        painter.rect_filled(keys, 0.0, p.header);
        let ruler_slot = Rect::from_min_max(
            pos2(
                keys.left(),
                keys.top() - crate::note_editor::view::RULER_HEIGHT,
            ),
            pos2(keys.right(), keys.top()),
        );
        painter.rect_filled(ruler_slot, 0.0, p.header);
        painter.hline(
            ruler_slot.x_range(),
            ruler_slot.bottom() - 0.5,
            Stroke::new(theme::stroke::HAIRLINE, p.border),
        );
        if ruler_slot.height() >= 12.0 {
            let font = eframe::egui::FontId::monospace(theme::size::SCALE);
            let text = widgets::fit(
                &painter,
                map_name,
                font.clone(),
                ruler_slot.width() - 2.0 * theme::space::M,
            );
            painter.text(
                pos2(ruler_slot.left() + theme::space::M, ruler_slot.center().y),
                eframe::egui::Align2::LEFT_CENTER,
                text,
                font,
                p.text_hint,
            );
        }
        painter.vline(
            keys.right() - 0.5,
            keys.y_range(),
            Stroke::new(theme::stroke::HAIRLINE, p.key_edge),
        );
        let picked = self.picked_rows(ed, lanes);
        let hover_row = ctx
            .input(|i| i.pointer.latest_pos())
            .filter(|p| view.body.contains(*p))
            .and_then(|p| geometry::row_at(view, p.y));
        let mut action = None;
        let id = ui.id().with(("drum names", clip.id.0));
        let dragging = ed.drum.reorder.filter(|r| r.map == map_id);
        for (row, lane) in lanes.iter().enumerate() {
            let row_rect = geometry::row_rect(view, row);
            if row_rect.bottom() < keys.top() || row_rect.top() > keys.bottom() {
                continue;
            }
            let rect = Rect::from_x_y_ranges(keys.x_range(), row_rect.y_range());
            let muted = lane.mute;
            let soloed = lane.solo;
            lane_name(
                &painter,
                rect,
                lane,
                picked.get(row).copied().unwrap_or(false),
                hover_row == Some(row),
                lane.silent,
                dragging.is_some_and(|r| r.from == lane.index),
            );
            let (mute, solo) = geometry::buttons(rect);
            let icon = geometry::icon_rect(rect);
            let name_rect = Rect::from_min_max(
                pos2(rect.left() + geometry::NAME_X, rect.top() + 2.0),
                pos2(mute.left() - theme::space::XS, rect.bottom() - 2.0),
            );
            let name = ui.interact(name_rect, id.with(("name", row)), Sense::click());
            name.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("{} lane, {} hits", lane.name, lane.hits),
                )
            });
            let name = name.on_hover_text(format!("Audition {}", lane.name));
            if name.clicked() {
                action = Some(Action::Audition(row));
            }
            widgets::Menu::context(&name).show(|ui| {
                if widgets::menu_item(ui, &format!("Audition {}", lane.name), None, true).clicked()
                {
                    action = Some(Action::Audition(row));
                }
                if lane.length.is_none() {
                    let shown = ed.drum.shows_length(map_id, lane.input);
                    if widgets::menu_toggle(ui, "Show the sound's length", None, shown).clicked() {
                        action = Some(Action::Length(row));
                    }
                } else {
                    ui.label("The sound's length is set in the map editor");
                }
            });
            let handle = ui.interact(icon, id.with(("move", row)), Sense::click_and_drag());
            handle.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("Move the {} lane", lane.name),
                )
            });
            let handle = handle.on_hover_text(format!("Drag to move the {} lane", lane.name));
            if handle.drag_started() {
                action = Some(Action::StartReorder(row));
            }
            if handle.clicked() {
                action = Some(Action::Audition(row));
            }
            if widgets::letter_toggle(
                ui,
                mute,
                id.with(("mute", row)),
                "M",
                muted,
                &format!("Mute {}", lane.name),
                theme::Tone::Mute,
            ) {
                action = Some(Action::Mute(row));
            }
            if widgets::letter_toggle(
                ui,
                solo,
                id.with(("solo", row)),
                "S",
                soloed,
                &format!("Solo {}", lane.name),
                theme::Tone::Accent,
            ) {
                action = Some(Action::Solo(row));
            }
        }
        if let Some(reorder) = dragging {
            let at = ctx.input(|i| i.pointer.latest_pos());
            if let Some(at) = at
                && !lanes.is_empty()
            {
                let row = geometry::row_at(view, at.y).unwrap_or_else(|| {
                    if at.y < view.body.top() {
                        0
                    } else {
                        lanes.len() - 1
                    }
                });
                let target = lanes[row].index;
                ed.drum.reorder = Some(Reorder {
                    map: map_id,
                    from: reorder.from,
                    to: target,
                });
                let y = geometry::row_rect(view, row).top();
                painter.hline(
                    keys.x_range(),
                    y,
                    Stroke::new(theme::stroke::EDGE, p.accent),
                );
            }
            if ctx.input(|i| !i.pointer.primary_down()) {
                let done = ed.drum.reorder.take().expect("a reorder");
                if done.from != done.to {
                    self.document.move_drum_sound(map_id, done.from, done.to);
                }
            }
            ctx.set_cursor_icon(CursorIcon::Grabbing);
            ctx.request_repaint();
        } else if let Some(at) = ctx.input(|i| i.pointer.latest_pos())
            && keys.contains(at)
        {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        if let Some(action) = action {
            self.drum_name_action(ed, clip, lanes, map_id, action);
        }
    }

    fn drum_name_action(
        &mut self,
        ed: &mut NoteEditor,
        clip: &Arc<Clip>,
        lanes: &[Lane],
        map: Id,
        action: Action,
    ) {
        match action {
            Action::Audition(row) => {
                if let Some(lane) = lanes.get(row) {
                    self.audition_note(clip.id, lane.input, 100);
                }
            }
            Action::Mute(row) => {
                if let Some(lane) = lanes.get(row) {
                    self.document.set_sound_mute(map, lane.input, !lane.mute);
                }
            }
            Action::Solo(row) => {
                if let Some(lane) = lanes.get(row) {
                    self.document.set_sound_solo(map, lane.input, !lane.solo);
                }
            }
            Action::StartReorder(row) => {
                if let Some(lane) = lanes.get(row) {
                    ed.drum.reorder = Some(Reorder {
                        map,
                        from: lane.index,
                        to: lane.index,
                    });
                }
            }
            Action::Length(row) => {
                if let Some(lane) = lanes.get(row) {
                    ed.drum.toggle_length(map, lane.input);
                }
            }
        }
    }

    pub(crate) fn drum_quantize_steps(
        &self,
        editor: &NoteEditor,
        clip: Id,
    ) -> Option<Vec<(u8, Ticks)>> {
        let track = self.track_of_clip(clip)?;
        if editor.drum.view(track) != View::Lanes {
            return None;
        }
        let map = self.clip_drum_map(clip)?;
        let (open, _) = self.editor_clip(clip)?;
        let meter = self.transport.timeline.meter.meter_at(open.start);
        let mut steps = Vec::new();
        for sound in &map.sounds {
            if let Some(quantize) = sound.quantize
                && let Some(step) =
                    lumen_core::grid::size_ticks(quantize.size, quantize.feel, meter)
            {
                steps.push((sound.input, step));
            }
        }
        (!steps.is_empty()).then_some(steps)
    }
}
