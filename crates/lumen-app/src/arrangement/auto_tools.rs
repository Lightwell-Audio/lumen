//! The lane's tools and the marks a gesture leaves on screen (M7 slice A2):
//! the strip that picks the pointer, the pencil or a shape, the box being
//! dragged, the dot that bends a segment, and the live value beside a point
//! being moved or a stroke being drawn.

use eframe::egui::{self, Painter, Pos2, Rect, Stroke, Ui, pos2, vec2};
use lumen_core::automation::{Lane, Shape};
use lumen_core::time::Ticks;
use lumen_project::automation::{AutomationLane, Target};
use lumen_project::edit::automation::Wave;
use lumen_project::model::Id;

use super::Arrangement;
use super::auto_scale as scale;
use super::auto_state::{Gesture, Tool, wave_name};
use super::auto_view::Frame;
use super::strip_output::row_button;
use crate::note_editor::glyphs;
use crate::theme;
use crate::widgets::{self, Choice};

/// The Re-enable button's label.
const REENABLE: &str = "Re-enable";

/// A segment narrower than this on screen has no bend dot: it could not be
/// grabbed apart from its points.
const BEND_MIN_WIDTH: f32 = 3.0 * theme::size::AUTO_GRAB;

/// The waves a shape tool draws, in the order the menu lists them.
pub(crate) const WAVES: [Wave; 5] = [
    Wave::Ramp,
    Wave::Sine,
    Wave::Triangle,
    Wave::Saw,
    Wave::Square,
];

impl Arrangement {
    /// The filled amber button beside the chip of a lane taken over: it lets
    /// the lane play again (M7 slice A4a).
    pub(super) fn reenable_button(
        &mut self,
        ui: &mut Ui,
        doc: &mut crate::document::Document,
        rect: Rect,
        (track, target): (&lumen_project::model::Track, Target),
        lane: &str,
    ) {
        let palette = *theme::p();
        let id = egui::Id::new(("automation reenable", track.id));
        let response = ui
            .interact(rect, id, egui::Sense::click())
            .on_hover_text("Let the lane play again; the control follows it");
        let name = format!("Re-enable {lane} of {}", track.name);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        let fill = if response.hovered() {
            palette.warning.gamma_multiply(0.88)
        } else {
            palette.warning
        };
        let radius = f32::from(theme::radius::for_height(rect.height()));
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            REENABLE,
            crate::fonts::medium(theme::size::CAPTION),
            theme::text_on(fill),
        );
        crate::widgets::raised::focus_ring(ui.painter(), &response, rect);
        if response.clicked() {
            doc.reenable_lane(track.id, target);
        }
    }
}

impl Frame {
    /// The height (0 to 1) of the lane at screen `y`.
    pub fn height_at(&self, y: f32) -> f32 {
        let area = self.area();
        ((area.bottom() - y) / area.height().max(1.0)).clamp(0.0, 1.0)
    }

    /// The screen `y` of height `h` (0 to 1).
    pub fn y_at_height(&self, h: f32) -> f32 {
        let area = self.area();
        area.bottom() - h * area.height()
    }

    /// Where the dot that bends the segment leaving point `index` is, if the
    /// segment has one: it runs to a next point at another value, is not a
    /// step, and is wide enough on screen to grab.
    pub fn bend_dot(&self, target: Target, lane: &Lane, index: usize) -> Option<Pos2> {
        let points = lane.points();
        let (from, to) = (points.get(index)?, points.get(index + 1)?);
        if from.shape == Shape::Step || from.value == to.value {
            return None;
        }
        let (x0, x1) = (self.x(from.at), self.x(to.at));
        if x1 - x0 < BEND_MIN_WIDTH {
            return None;
        }
        let [u, v] = match from.shape {
            Shape::Curve(curve) => curve.handle().dot,
            _ => [0.5, 0.5],
        };
        Some(pos2(
            x0 + u * (x1 - x0),
            self.y(target, from.value + (to.value - from.value) * v),
        ))
    }
}

impl Arrangement {
    /// The tool strip, to the right of the lane's chip, and the wave chooser
    /// when a shape tool is in hand.
    pub(super) fn lane_tools(&mut self, ui: &mut Ui, frame: &Frame, chip: Rect, track: &str) {
        let area = Rect::from_min_size(
            pos2(chip.right() + theme::space::S, chip.top()),
            vec2(
                (frame.row.right() - chip.right() - 2.0 * theme::space::S).max(0.0),
                chip.height(),
            ),
        );
        if area.width() < 150.0 {
            return;
        }
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(area)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        row.set_clip_rect(area.intersect(ui.clip_rect()));
        let shape = Tool::Shape(self.auto.wave);
        let pointer_name = format!("Automation pointer tool of {track}");
        let pencil_name = format!("Automation pencil tool of {track}");
        let shape_name = format!("Automation shape tool of {track}");
        let choices = [
            Choice::new(Tool::Pointer, "Pointer")
                .icon(glyphs::pointer)
                .named(&pointer_name)
                .tip("Pick, move and add points (Shift drags a box)"),
            Choice::new(Tool::Pencil, "Pencil")
                .icon(glyphs::pencil)
                .named(&pencil_name)
                .tip("Draw the lane freehand (B)"),
            Choice::new(shape, "Shape")
                .icon(glyphs::curve)
                .named(&shape_name)
                .tip("Drag out a ramp, sine, triangle, saw or square"),
        ];
        if let Some(tool) = widgets::segmented(
            &mut row,
            egui::Id::new(("automation tools", track)),
            self.auto.tool,
            &choices,
        ) {
            self.auto.use_tool(tool);
        }
        if let Tool::Shape(wave) = self.auto.tool {
            row.add_space(theme::space::XS);
            let at = row.cursor().min;
            let chip = Rect::from_min_size(at, vec2(88.0, chip.height()));
            let name = format!("Automation shape of {track}: {}", wave_name(wave));
            let id = egui::Id::new(("automation shape", track));
            let response = row_button(
                &mut row,
                chip,
                id,
                "wave",
                wave_name(wave),
                &name,
                "Choose the shape the shape tool draws",
            );
            widgets::Menu::below(&response).show(|ui| {
                ui.set_min_width(140.0);
                for choice in WAVES {
                    if widgets::menu_toggle(ui, wave_name(choice), None, choice == wave).clicked() {
                        self.auto.use_tool(Tool::Shape(choice));
                        ui.close();
                    }
                }
            });
        }
    }

    /// What a gesture draws over the lane: the box being dragged and the
    /// dot that bends the one picked segment.
    pub(super) fn draw_marks(
        &self,
        painter: &Painter,
        frame: &Frame,
        track: Id,
        target: Target,
        entry: Option<&AutomationLane>,
    ) {
        let palette = *theme::p();
        if let Some(active) = &self.auto.active
            && active.track == track
            && active.target == target
            && let Gesture::Box(boxing) = &active.how
        {
            let rect = Rect::from_two_pos(boxing.from, boxing.to);
            painter.rect(
                rect,
                theme::radius::SMALL,
                palette.accent.gamma_multiply(0.12),
                Stroke::new(theme::stroke::HAIRLINE, palette.accent.gamma_multiply(0.7)),
                egui::StrokeKind::Inside,
            );
        }
        let Some(entry) = entry else {
            return;
        };
        let Some(one) = self.auto.picked_on(track, target).and_then(|p| p.single()) else {
            return;
        };
        if let Some(dot) = frame.bend_dot(target, &entry.lane, one.index)
            && frame.row.contains(dot)
        {
            let hot = self.auto.active.as_ref().is_some_and(|a| {
                matches!(&a.how, Gesture::Bend(b) if b.index == one.index) && a.track == track
            });
            let big = if hot { 1.0 } else { 0.0 };
            // Solid, where the points are hollow: it is a handle, not a point.
            painter.circle_filled(
                dot,
                theme::size::AUTO_BEND + 3.0 + big,
                palette.accent.gamma_multiply(0.22),
            );
            painter.circle_filled(dot, theme::size::AUTO_BEND + big, palette.accent);
        }
    }

    /// The value, and where in the song, beside the point being moved or the
    /// stroke being drawn, over everything.
    pub(super) fn lane_tip(
        &mut self,
        ctx: &egui::Context,
        frame: &Frame,
        track: Id,
        target: Target,
        entry: Option<&AutomationLane>,
        position: &dyn Fn(Ticks) -> String,
    ) {
        let Some(active) = self
            .auto
            .active
            .as_ref()
            .filter(|a| a.track == track && a.target == target)
        else {
            return;
        };
        let spot = match &active.how {
            Gesture::Move(moving) => entry
                .and_then(|e| e.lane.points().get(moving.anchor).copied())
                .map(|p| (p.at, p.value)),
            Gesture::Draw(drawing) => drawing.raw.last().copied(),
            Gesture::Wave(waving) => Some((waving.to, scale::value(target, waving.end))),
            Gesture::Box(_) | Gesture::Bend(_) => None,
        };
        let Some((at, value)) = spot else {
            return;
        };
        let text = format!("{}  \u{00B7}  {}", scale::text(target, value), position(at));
        // Above the point, or below it when that would cover the tool strip.
        let y = frame.y(target, value);
        let lift = if y - 14.0 < frame.area().top() {
            16.0
        } else {
            -14.0
        };
        let pos = pos2(frame.x(at) + 12.0, y + lift);
        let layer = egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new(("automation tip", track)),
        );
        widgets::draw_hint(&ctx.layer_painter(layer), pos, &text, ctx.content_rect());
        self.auto.tip = Some(text);
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::automation::Point;
    use lumen_core::curve::Curve;

    use super::*;
    use crate::timeview::TimeView;

    fn frame() -> Frame {
        Frame {
            row: Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 100.0)),
            canvas_left: 0.0,
            view: TimeView {
                left: 0.0,
                zoom: 40.0,
            },
        }
    }

    fn two(shape: Shape, a: f32, b: f32) -> Lane {
        Lane::new(vec![
            Point::new(Ticks::quarters(0), a).with_shape(shape),
            Point::new(Ticks::quarters(4), b),
        ])
        .unwrap()
    }

    #[test]
    fn a_segment_has_a_bend_dot_in_its_middle_until_it_is_bent() {
        let f = frame();
        let lane = two(Shape::Linear, -20.0, 0.0);
        let dot = f.bend_dot(Target::Volume, &lane, 0).expect("a dot");
        assert!((dot.x - f.x(Ticks::quarters(2))).abs() < 0.01);
        let mid = f.y(Target::Volume, -10.0);
        assert!((dot.y - mid).abs() < 0.01);
        // The dot sits on the curve it bent.
        let bent = Curve::from_handle(lumen_core::curve::Handle {
            dot: [0.3, 0.7],
            s: 0.0,
        });
        let lane = two(Shape::Curve(bent), -20.0, 0.0);
        let dot = f.bend_dot(Target::Volume, &lane, 0).unwrap();
        assert!((dot.x - f.x(Ticks::quarters(4)) * 0.3).abs() < 0.01);
        assert!((dot.y - f.y(Target::Volume, -20.0 + 20.0 * 0.7)).abs() < 0.01);
    }

    #[test]
    fn steps_flat_segments_and_narrow_ones_have_no_dot() {
        let f = frame();
        assert!(
            f.bend_dot(Target::Volume, &two(Shape::Step, -20.0, 0.0), 0)
                .is_none()
        );
        assert!(
            f.bend_dot(Target::Volume, &two(Shape::Linear, -6.0, -6.0), 0)
                .is_none()
        );
        assert!(
            f.bend_dot(Target::Volume, &two(Shape::Linear, -20.0, 0.0), 1)
                .is_none()
        );
        let narrow = Frame {
            view: TimeView {
                left: 0.0,
                zoom: 2.0,
            },
            ..f
        };
        assert!(
            narrow
                .bend_dot(Target::Volume, &two(Shape::Linear, -20.0, 0.0), 0)
                .is_none(),
            "8 points wide is too narrow to grab"
        );
    }

    #[test]
    fn a_height_and_the_screen_agree() {
        let f = frame();
        let area = f.area();
        assert!((f.height_at(area.bottom()) - 0.0).abs() < 1e-6);
        assert!((f.height_at(area.top()) - 1.0).abs() < 1e-6);
        let y = f.y_at_height(0.25);
        assert!((f.height_at(y) - 0.25).abs() < 1e-5);
        assert!((f.y(Target::Pan, 0.0) - f.y_at_height(0.5)).abs() < 1e-4);
    }
}
