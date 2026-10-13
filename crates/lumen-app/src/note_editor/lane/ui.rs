//! The lane on the screen (M5 slice 4b): its header (the lane tabs and the
//! tools), the pointer, and committing a gesture to the document. See
//! `gesture` for what a drag means and `paint` for how it looks.

use std::borrow::Cow;
use std::sync::Arc;

use eframe::egui::{
    self, Align, Color32, CursorIcon, Layout, PointerButton, Pos2, Rect, Sense, Stroke, Ui,
    UiBuilder, pos2, vec2,
};
use lumen_core::grid::Line as GridLine;
use lumen_core::time::Ticks;
use lumen_project::edit::events as ev;
use lumen_project::model::{Clip, Note};

use super::geometry::{HIT, LaneKind, LaneView};
use super::gesture::{Env, Kind, LaneGesture, LaneTool, Resolved, boxed_stems};
use super::paint::{self, BendHandle, LaneHover, LaneScene, bend_dot};
use super::{LaneState, events_of};
use crate::document::{Drag, Line, Shift};
use crate::note_editor::glyphs;
use crate::note_editor::view::EditorView;
use crate::note_editor::{NoteEditor, notes_of};
use crate::widgets::{self, Choice, Pill};
use crate::{App, theme};

/// The height of the lane's header: the tabs and the tools.
pub(crate) const HEADER: f32 = theme::size::CONTROL + 2.0 * theme::space::XS;
/// The plotting area's shortest height (points).
const MIN_PLOT: f32 = 48.0;
/// ...and tallest.
const MAX_PLOT: f32 = 150.0;
/// The roll above the lane keeps at least this much height.
const ROLL_MIN: f32 = 140.0;

/// The plotting area's height for an editor `height` points tall.
fn plot_height(height: f32) -> f32 {
    (height * 0.28).clamp(MIN_PLOT, MAX_PLOT)
}

/// The height to keep at the bottom of the editor for the lane, header
/// included: none when the lane is switched off or the editor is too short
/// to spare it. `roll` is the height the toolbar, ruler and notes would
/// otherwise have.
pub(crate) fn reserved(roll: f32, state: &LaneState) -> f32 {
    if state.hidden {
        return 0.0;
    }
    let total = HEADER + plot_height(roll);
    if roll - total < ROLL_MIN { 0.0 } else { total }
}

/// The controllers the "+ CC" menu offers beside the ones with tabs.
const COMMON: [u8; 10] = [2, 7, 10, 11, 64, 71, 72, 73, 74, 91];

/// The lane tabs, in order.
fn kinds(state: &LaneState) -> Vec<LaneKind> {
    let mut kinds = vec![
        LaneKind::Velocity,
        LaneKind::Controller(1),
        LaneKind::PitchBend,
        LaneKind::Pressure,
    ];
    for &n in &state.extra {
        if !kinds.contains(&LaneKind::Controller(n)) {
            kinds.push(LaneKind::Controller(n));
        }
    }
    kinds
}

/// A value as the lane's chip and readouts say it.
fn value_text(kind: LaneKind, value: i32) -> String {
    match kind {
        LaneKind::PitchBend => format!("{} {value:+}", kind.name()),
        _ => format!("{} {value}", kind.name()),
    }
}

/// What the lane needs from the editor for one frame.
pub(in crate::note_editor) struct LaneInput<'a> {
    pub clip: &'a Arc<Clip>,
    pub colour: Color32,
    pub view: &'a EditorView,
    /// The lane's whole area: header, scale and plot.
    pub area: Rect,
    pub lines: &'a [GridLine],
    /// The pointer, when it is over the editor and nothing covers it.
    pub over: Option<Pos2>,
    pub editable: bool,
}

/// A gesture's result so far, and whether the bend handle is being dragged.
struct Live {
    resolved: Resolved,
    bending: bool,
}

impl App {
    /// The editor's keys are live and points of a controller lane are
    /// picked, the last place pressed being the lane: Delete means them.
    pub(crate) fn lane_keys_picked(&self) -> bool {
        let ed = &self.note_editor;
        ed.live && ed.lane.focus && ed.lane.kind != LaneKind::Velocity && !ed.lane.picked.is_empty()
    }

    /// Deletes the picked points of the lane: one undo step.
    pub(crate) fn lane_delete(&mut self) {
        let Some(id) = self.note_editor.clip else {
            return;
        };
        let picked = std::mem::take(&mut self.note_editor.lane.picked);
        self.document.delete_lane_points(id, &picked);
    }

    /// Draws the lane in `inp.area` and runs its pointer.
    pub(in crate::note_editor) fn draw_lane(
        &mut self,
        ui: &mut Ui,
        ed: &mut NoteEditor,
        inp: &LaneInput<'_>,
    ) {
        let ctx = ui.ctx().clone();
        let clip = inp.clip;
        let area = inp.area;
        let header = Rect::from_min_max(area.min, pos2(area.right(), area.top() + HEADER));
        let column = Rect::from_min_max(
            pos2(area.left(), header.bottom()),
            pos2(inp.view.body.left(), area.bottom()),
        );
        let plot = Rect::from_min_max(pos2(inp.view.body.left(), header.bottom()), area.max);
        let palette = theme::p();
        let painter = ui.painter().clone();
        painter.rect_filled(header, 0.0, palette.raised);
        painter.hline(
            header.x_range(),
            header.top(),
            Stroke::new(1.0, palette.border_strong),
        );
        painter.hline(
            header.x_range(),
            header.bottom(),
            Stroke::new(1.0, palette.border),
        );
        self.lane_header(ui, header, ed);
        ed.lane.sync(clip);
        let lview = LaneView {
            body: plot,
            time: inp.view.time,
            clip_start: inp.view.clip_start,
            window: inp.view.window,
            kind: ed.lane.kind,
        };
        ed.last_lane = Some(lview);
        let id = ui.id().with(("lane", clip.id.0));
        let response = ui.interact(plot, id, Sense::click_and_drag());
        let name = lview.kind.name();
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                format!("{name} lane: draw values"),
            )
        });
        let over = inp.over.filter(|p| plot.contains(*p));
        let live = if inp.editable {
            self.lane_pointer(&ctx, ed, &lview, inp, over)
        } else {
            Live {
                resolved: Resolved::Nothing,
                bending: false,
            }
        };
        // A bend in progress shows the new curve with its dot.
        let live_bend = match (&live.resolved, live.bending) {
            (Resolved::Span { from, to, curve }, true) => Some(BendHandle {
                from: *from,
                to: *to,
                curve: *curve,
            }),
            _ => ed.lane.bend,
        };
        let notes = notes_of(clip);
        let events = events_of(clip);
        let Some(cache) = ed.lane.cache.as_ref() else {
            return;
        };
        let hover = match over {
            Some(p) if ed.lane.gesture.is_none() && inp.editable => match lview.kind {
                LaneKind::Velocity => lview
                    .head_at(notes, &cache.stems, ed.picked(), p)
                    .and_then(|note| {
                        let at = notes.get(note)?.start();
                        let stem = cache.stems.binary_search_by_key(&at, |(t, _)| *t).ok()?;
                        Some(LaneHover::Head { stem, note })
                    })
                    .unwrap_or(LaneHover::None),
                _ => lview
                    .point_at(events, p)
                    .and_then(|i| cache.points.binary_search_by_key(&i, |q| q.index).ok())
                    .map_or(LaneHover::None, LaneHover::Point),
            },
            _ => LaneHover::None,
        };
        paint::paint_scale(&painter, &lview, column);
        let scene = LaneScene {
            notes,
            stems: &cache.stems,
            picked_notes: ed.picked(),
            points: &cache.points,
            picked_points: &ed.lane.picked,
            colour: inp.colour,
            clip_length: clip.length,
            lines: inp.lines,
            hover,
            preview: &live.resolved,
            bend: live_bend.as_ref(),
        };
        paint::paint_lane(&painter, &lview, &scene);
        if let (Some(chip), Some(p)) = (
            self.lane_chip(&lview, &live.resolved, grabbed_head(ed)),
            ctx.input(|i| i.pointer.latest_pos()),
        ) {
            widgets::draw_hint(&painter, p + vec2(14.0, -16.0), &chip, plot);
        }
        if inp.editable
            && let Some(p) = over.or(ctx
                .input(|i| i.pointer.latest_pos())
                .filter(|_| ed.lane.gesture.is_some()))
        {
            ctx.set_cursor_icon(match hover {
                LaneHover::None => CursorIcon::Crosshair,
                _ => CursorIcon::Grab,
            });
            let _ = p;
        }
        if ed.lane.gesture.is_some() {
            ctx.request_repaint();
        }
    }

    /// The chip beside the pointer while a gesture runs.
    fn lane_chip(
        &self,
        view: &LaneView,
        resolved: &Resolved,
        grabbed: Option<usize>,
    ) -> Option<String> {
        let kind = view.kind;
        match resolved {
            // Dragging heads shows the one pressed; a stroke, the last set.
            Resolved::Velocities(changes) => changes
                .iter()
                .rfind(|&&(note, _)| grabbed.is_none_or(|g| g == note))
                .or(changes.last())
                .map(|&(_, v)| value_text(LaneKind::Velocity, i32::from(v))),
            Resolved::Points(points) => points.last().map(|&(_, v)| value_text(kind, v)),
            Resolved::Span { from, to, .. } => Some(format!(
                "{} to {}",
                value_text(kind, from.1),
                value_text(kind, to.1)
                    .trim_start_matches(&kind.name())
                    .trim()
            )),
            Resolved::Ramp { from, to, .. } => Some(format!("Velocity {} to {}", from.1, to.1)),
            Resolved::Shift { value, .. } if *value != 0 => Some(format!("{value:+}")),
            _ => None,
        }
    }

    /// The lane tabs, "+ CC", and the tools.
    fn lane_header(&mut self, ui: &mut Ui, header: Rect, ed: &mut NoteEditor) {
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(header.shrink2(vec2(theme::space::S, theme::space::XS)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.set_clip_rect(header.intersect(ui.clip_rect()));
        let all = kinds(&ed.lane);
        let names: Vec<String> = all.iter().map(|k| k.name()).collect();
        let tips: Vec<String> = names.iter().map(|n| format!("Edit {n}")).collect();
        let tabs: Vec<Pill<'_>> = names
            .iter()
            .zip(&tips)
            .map(|(label, tip)| Pill { label, tip })
            .collect();
        let chosen = all.iter().position(|k| *k == ed.lane.kind).unwrap_or(0);
        let tab_id = ui.id().with("lane tabs");
        if let Some(i) = widgets::pill_tabs(&mut row, tab_id, chosen, &tabs) {
            ed.lane.choose(all[i]);
        }
        let plus = widgets::pill_button(&mut row, tab_id.with("plus"), "+ CC");
        let plus = widgets::tip(plus, "Show another controller in the lane", None, None);
        widgets::Menu::below(&plus).show(|ui| {
            ui.set_min_width(140.0);
            for n in COMMON {
                let kind = LaneKind::Controller(n);
                if all.contains(&kind) {
                    continue;
                }
                if widgets::menu_item(ui, &kind.name(), None, true).clicked() {
                    ed.lane.extra.push(n);
                    ed.lane.choose(kind);
                }
            }
        });
        row.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let choices = [
                Choice::new(LaneTool::Pencil, "Pencil")
                    .icon(glyphs::pencil)
                    .named("Lane pencil tool")
                    .tip("Draw values freehand"),
                Choice::new(LaneTool::Line, "Line")
                    .icon(glyphs::line)
                    .named("Lane line tool")
                    .tip("Drag a straight line from one value to another"),
                Choice::new(LaneTool::Curve, "Curve")
                    .icon(glyphs::curve)
                    .named("Lane curve tool")
                    .tip("Drag a line, then drag its dot to bend it"),
            ];
            if let Some(tool) =
                widgets::segmented(ui, ui.id().with("lane tool"), ed.lane.tool, &choices)
            {
                ed.lane.use_tool(tool);
            }
        });
    }

    /// Presses, drags and releases in the lane. Returns the gesture's
    /// result so far, for drawing.
    fn lane_pointer(
        &mut self,
        ctx: &egui::Context,
        ed: &mut NoteEditor,
        view: &LaneView,
        inp: &LaneInput<'_>,
        over: Option<Pos2>,
    ) -> Live {
        let nothing = Live {
            resolved: Resolved::Nothing,
            bending: false,
        };
        let (pressed, down, latest) = ctx.input(|i| {
            (
                i.pointer.button_pressed(PointerButton::Primary),
                i.pointer.primary_down(),
                i.pointer.latest_pos(),
            )
        });
        let modifiers = ctx.input(|i| i.modifiers);
        let clip = inp.clip;
        let notes = notes_of(clip);
        let events = events_of(clip);
        if ed.lane.gesture.is_none()
            && pressed
            && let Some(p) = over
            && !egui::Popup::is_any_open(ctx)
        {
            ed.lane.focus = true;
            let kind = start_kind(ed, view, notes, events, p, modifiers.shift);
            if let Kind::Heads { grabbed, .. } = &kind
                && !ed.picked().contains(grabbed)
            {
                // A head pressed outside the picks picks it alone.
                self.select_notes(ed, vec![*grabbed]);
            }
            ed.lane.gesture = Some(LaneGesture::new(kind, p));
        }
        let Some(gesture) = &mut ed.lane.gesture else {
            return nothing;
        };
        let at = latest.unwrap_or(gesture.origin);
        gesture.feed(at);
        let zoom = view.time.zoom;
        let cell = self.grid_cell_at_zoom(clip.start, zoom);
        let step = if self.grid.snap && !modifiers.alt {
            cell.1 - cell.0
        } else {
            Ticks::ZERO
        };
        let origin = clip.start;
        let snap =
            |local: Ticks| self.snapped_at_zoom(origin + local, modifiers.alt, zoom) - origin;
        let Some(cache) = ed.lane.cache.as_ref() else {
            return nothing;
        };
        let env = Env {
            view,
            notes,
            events,
            step,
            snap: &snap,
            length: lumen_project::edit::content_length(clip),
            stems: Cow::Borrowed(&cache.stems),
        };
        let resolved = gesture.resolve(at, &env);
        let bending = matches!(gesture.kind, Kind::Bend { .. });
        if down {
            return Live { resolved, bending };
        }
        // Released.
        let Some(gesture) = ed.lane.gesture.take() else {
            return nothing;
        };
        self.commit_lane(ed, inp, view, &gesture, resolved, modifiers.shift);
        nothing
    }

    /// Makes a finished gesture's result in the document: one undo step.
    fn commit_lane(
        &mut self,
        ed: &mut NoteEditor,
        inp: &LaneInput<'_>,
        view: &LaneView,
        gesture: &LaneGesture,
        resolved: Resolved,
        shift: bool,
    ) {
        let clip = inp.clip;
        let id = clip.id;
        let lane = view.kind.lane();
        let mut picked = ed.lane.picked.clone();
        let mut bend = None;
        match (resolved, lane) {
            (Resolved::Velocities(changes), _) if !changes.is_empty() => {
                let indices = self.document.set_velocities_each(None, id, &changes, None);
                match &gesture.kind {
                    // Dragging heads keeps the picks, now of the new clip.
                    Kind::Heads { group, .. } => self.select_notes(ed, group.clone()),
                    _ => self.select_notes(ed, indices),
                }
            }
            (Resolved::Points(points), Some(lane)) if !points.is_empty() => {
                picked = self
                    .document
                    .draw_lane_points(None, id, lane, &points, None);
            }
            (Resolved::Span { from, to, curve }, Some(lane)) => {
                picked = self.document.draw_lane_span(
                    Drag::default(),
                    id,
                    lane,
                    Line {
                        from,
                        to,
                        curve: &curve,
                    },
                    ev::FINE,
                );
                if ed.lane.tool == LaneTool::Curve || matches!(gesture.kind, Kind::Bend { .. }) {
                    bend = Some(BendHandle { from, to, curve });
                }
            }
            (
                Resolved::Ramp {
                    from,
                    to,
                    notes,
                    curve,
                },
                _,
            ) if !notes.is_empty() => {
                self.document.ramp_velocities(
                    Drag::default(),
                    id,
                    &notes,
                    Line {
                        from,
                        to,
                        curve: &curve,
                    },
                );
                self.select_notes(ed, notes);
            }
            (Resolved::Shift { time, value }, Some(lane)) => {
                if let Kind::Move { picked: moving, .. } = &gesture.kind {
                    picked = self.document.move_lane_points(
                        Drag::default(),
                        id,
                        lane,
                        moving,
                        Shift { time, value },
                    );
                }
            }
            (Resolved::Box(rect), lane) => {
                let Kind::Box { base } = &gesture.kind else {
                    return;
                };
                let mut chosen = if shift { base.clone() } else { Vec::new() };
                if lane.is_some() {
                    chosen.extend(view.points_in(events_of(clip), rect));
                    chosen.sort_unstable();
                    chosen.dedup();
                    picked = chosen;
                } else if let Some(cache) = &ed.lane.cache {
                    chosen.extend(boxed_stems(view, notes_of(clip), &cache.stems, rect));
                    chosen.sort_unstable();
                    chosen.dedup();
                    self.select_notes(ed, chosen);
                }
            }
            _ => {
                // A click on one of several picked points picks it alone.
                if !gesture.dragging
                    && let Kind::Move { grabbed, .. } = &gesture.kind
                {
                    picked = vec![*grabbed];
                }
            }
        }
        if let Some((fresh, _)) = self.editor_clip(id) {
            ed.lane.rebind(&fresh, picked, bend);
        }
    }
}

/// The note whose head the running gesture drags, if it drags heads.
fn grabbed_head(ed: &NoteEditor) -> Option<usize> {
    match ed.lane.gesture.as_ref()?.kind {
        Kind::Heads { grabbed, .. } => Some(grabbed),
        _ => None,
    }
}

/// What a press at `p` in the lane starts. In the velocity lane a press on a
/// head drags the heads: the picked notes' when it is one of them, else that
/// note's alone.
fn start_kind(
    ed: &mut NoteEditor,
    view: &LaneView,
    notes: &[Note],
    events: &[lumen_project::model::ClipEvent],
    p: Pos2,
    shift: bool,
) -> Kind {
    if let Some(b) = ed.lane.bend
        && (bend_dot(view, &b) - p).length() <= HIT
    {
        return Kind::Bend {
            from: b.from,
            to: b.to,
        };
    }
    ed.lane.bend = None;
    if shift {
        let base = if view.kind == LaneKind::Velocity {
            ed.picked().to_vec()
        } else {
            ed.lane.picked.clone()
        };
        return Kind::Box { base };
    }
    if view.kind == LaneKind::Velocity
        && let Some(cache) = &ed.lane.cache
        && let Some(grabbed) = view.head_at(notes, &cache.stems, ed.picked(), p)
    {
        let picked = ed.picked();
        let group = if picked.contains(&grabbed) {
            picked.to_vec()
        } else {
            vec![grabbed]
        };
        return Kind::Heads { grabbed, group };
    }
    if view.kind != LaneKind::Velocity
        && let Some(i) = view.point_at(events, p)
    {
        if !ed.lane.picked.contains(&i) {
            ed.lane.picked = vec![i];
        }
        return Kind::Move {
            grabbed: i,
            picked: ed.lane.picked.clone(),
        };
    }
    match ed.lane.tool {
        LaneTool::Pencil => Kind::Pencil,
        LaneTool::Line | LaneTool::Curve => Kind::Line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lane_takes_a_share_of_a_tall_editor_and_none_of_a_short_one() {
        let state = LaneState::default();
        let tall = reserved(520.0, &state);
        assert!(tall > HEADER + MIN_PLOT && tall < 520.0 - ROLL_MIN);
        assert_eq!(reserved(200.0, &state), 0.0);
    }

    #[test]
    fn a_hidden_lane_takes_nothing() {
        let state = LaneState {
            hidden: true,
            ..LaneState::default()
        };
        assert_eq!(reserved(600.0, &state), 0.0);
    }

    #[test]
    fn the_tabs_list_the_four_lanes_then_the_added_controllers_once() {
        let state = LaneState {
            extra: vec![7, 1, 7],
            ..LaneState::default()
        };
        assert_eq!(
            kinds(&state),
            [
                LaneKind::Velocity,
                LaneKind::Controller(1),
                LaneKind::PitchBend,
                LaneKind::Pressure,
                LaneKind::Controller(7),
            ]
        );
    }

    #[test]
    fn bend_values_show_their_sign() {
        assert_eq!(value_text(LaneKind::PitchBend, 120), "Pitch bend +120");
        assert_eq!(value_text(LaneKind::Controller(1), 64), "Mod wheel 64");
    }
}
