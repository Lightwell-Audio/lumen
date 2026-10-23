//! The Draw tool's press-and-drag on an empty MIDI lane (M7b slice E0): a
//! click makes a one-bar clip, a drag sets its length on the grid. Nothing
//! changes in the project until the pointer is released; Esc ends the
//! gesture first.

use eframe::egui::{self, Align2, FontId, Painter, Pos2, Rect, Stroke, pos2};
use lumen_core::meter::MeterMap;
use lumen_core::time::Ticks;
use lumen_project::model::{Id, TrackKind};

use super::grid::draw_snap_guide;
use crate::App;
use crate::arrangement::select::Gesture;
use crate::document::NewClip;
use crate::theme;

impl App {
    /// The gesture a press at `origin` starts with the Draw tool: only on
    /// an empty spot of a MIDI track's lane.
    pub(super) fn begin_draw(
        &self,
        ctx: &egui::Context,
        origin: Pos2,
        canvas: Rect,
        free: bool,
    ) -> Option<Gesture> {
        if self.hit(ctx, origin, canvas).is_some() {
            return None;
        }
        let track = self.lane_track(ctx, canvas, origin)?;
        let midi = self
            .document
            .project()
            .tracks
            .iter()
            .any(|t| t.id == track && t.kind == TrackKind::Midi);
        let at = self.snapped(self.ticks_at(canvas, origin.x), free);
        midi.then_some(Gesture::Draw {
            track,
            from: at,
            to: at,
        })
    }

    /// Follows the pointer to `p` and shows the clip that release would
    /// make: its outline on the lane, its length, and the snap guide.
    pub(super) fn aim_draw(
        &mut self,
        ctx: &egui::Context,
        p: Pos2,
        canvas: Rect,
        free: bool,
        painter: &Painter,
    ) {
        let Some(Gesture::Draw { track, from, .. }) = self.arrangement.gesture.clone() else {
            return;
        };
        let to = self.snapped(self.ticks_at(canvas, p.x), free);
        self.arrangement.gesture = Some(Gesture::Draw { track, from, to });
        let x = |t: Ticks| self.nav.view.x(canvas.left(), t.as_quarters());
        draw_snap_guide(painter, x(to), canvas);
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        let project = self.document.project();
        let pending = self.lanes.pending().len();
        let Some((top, bottom)) = self
            .arrangement
            .lane_span(ctx, project, pending, canvas, track)
        else {
            return;
        };
        let (start, end) = (from.min(to).max(Ticks::ZERO), from.max(to));
        let outline = Rect::from_min_max(
            pos2(x(start), top),
            pos2(x(end).max(x(start) + 1.0), bottom),
        )
        .intersect(canvas);
        let palette = theme::p();
        painter.rect_filled(
            outline,
            theme::radius::SMALL,
            palette.accent.gamma_multiply(0.18),
        );
        painter.rect_stroke(
            outline,
            theme::radius::SMALL,
            Stroke::new(theme::stroke::LIT, palette.accent),
            egui::StrokeKind::Inside,
        );
        if end > start {
            let label = length_label(&project.meter_map(), start, end);
            painter.text(
                pos2(outline.left() + theme::space::S, outline.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(theme::size::CAPTION),
                palette.text,
            );
        }
    }

    /// Release: a drag of some length makes a clip of that length; a press
    /// released where it began is a click, a bar.
    pub(super) fn finish_draw(&mut self, track: Id, from: Ticks, to: Ticks) {
        let place = if from == to {
            NewClip::Bar(from)
        } else {
            NewClip::Range(from, to)
        };
        self.put_midi_clip(track, place);
    }
}

/// How long a clip from `start` to `end` is, as a person counts it: whole
/// bars when both ends are bar lines ("1 bar", "4 bars"), else beats.
fn length_label(map: &MeterMap, start: Ticks, end: Ticks) -> String {
    let (a, b) = (map.bbt_at(start).bar, map.bbt_at(end).bar);
    if map.bar_start(a) == start && map.bar_start(b) == end && b > a {
        return match b - a {
            1 => "1 bar".to_owned(),
            n => format!("{n} bars"),
        };
    }
    let beats = (end - start).as_quarters();
    let text = format!("{beats:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if (beats - 1.0).abs() < 1e-9 {
        "1 beat".to_owned()
    } else {
        format!("{text} beats")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::meter::Meter;

    fn four_four() -> MeterMap {
        MeterMap::constant(Meter::FOUR_FOUR).expect("4/4 is a valid map")
    }

    #[test]
    fn whole_bars_are_counted_in_bars() {
        let map = four_four();
        let bar = map.bar_start(1) - map.bar_start(0);
        assert_eq!(length_label(&map, Ticks::ZERO, bar), "1 bar");
        assert_eq!(length_label(&map, bar, Ticks(bar.0 * 4)), "3 bars");
    }

    #[test]
    fn anything_else_is_counted_in_beats() {
        let map = four_four();
        assert_eq!(
            length_label(&map, Ticks::ZERO, Ticks::quarters(1)),
            "1 beat"
        );
        assert_eq!(
            length_label(&map, Ticks::quarters(1), Ticks::quarters(4)),
            "3 beats"
        );
        assert_eq!(
            length_label(&map, Ticks::ZERO, Ticks::quarters(5)),
            "5 beats"
        );
    }
}
