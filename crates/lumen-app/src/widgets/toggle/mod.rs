//! On/off and small choices (the interface audit's T3, the owner's A):
//! a switch for anything on or off, and a segmented control for two to
//! five choices. They replace egui's checkboxes and radio buttons, the only
//! controls that still looked like egui; longer lists are menu rows
//! (`menu_toggle`). Their look is the design system's Segmented card
//! (ADR-0052): sunk troughs, raised neutral thumbs, Ember under the
//! pointer, a hairline for what is on.

use eframe::egui::{self, FontId, Sense, Ui, vec2};
use eframe::emath::GuiRounding as _;

use crate::theme::Tone;
use crate::{motion, theme};

use super::raised;

mod menu;
mod segmented;
mod switch;
mod tabs;

pub use menu::{choice_row, menu_caption, menu_header};
pub use segmented::{Choice, segmented, segmented_full, segmented_tool, segmented_tool_width};
pub use switch::{SWITCH_WIDTH, switch, switch_noted};
pub use tabs::{Tab, TabsLayout, tab_divider, tabs_with};

/// The width [`toggle_button`] takes for `label`.
pub fn toggle_button_width(ui: &Ui, label: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme::size::BODY),
        egui::Color32::PLACEHOLDER,
    );
    galley.size().x + 2.0 * theme::space::S
}

/// A button that stays lit while `on` (Browser, Detail): raised like every
/// button when off, `accent_muted` with accent letters and a 38% accent
/// hairline when on (the Button card, ADR-0052; the interface audit's T4:
/// off still reads as a button).
pub fn toggle_button(ui: &mut Ui, on: &mut bool, label: &str) -> egui::Response {
    let palette = theme::p();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme::size::BODY),
        palette.text,
    );
    let pad = theme::space::S;
    let size = vec2(galley.size().x + 2.0 * pad, theme::size::CONTROL);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let state = *on;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, state, label)
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let lit = motion::hover(ui.ctx(), response.id.with("lit"), state);
    let face = raised::face_of(ui, &response).lit(lit, Tone::On);
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    let body = raised::paint(
        ui.painter(),
        rect,
        theme::radius::for_height(rect.height()),
        &face,
    );
    raised::centre_galley(ui.painter(), body, galley, face.text());
    raised::focus_ring(ui.painter(), &response, rect);
    response
}
/// The left and right edges at position `at` along `spans` (each a tab's or
/// segment's left and right): `at` 0 is the first span, 1 the second, and in
/// between each edge is blended. Clamped to the list; empty gives zeros.
fn slide_between(spans: &[(f32, f32)], at: f32) -> (f32, f32) {
    let Some(last) = spans.len().checked_sub(1) else {
        return (0.0, 0.0);
    };
    let at = at.clamp(0.0, last as f32);
    let i = (at.floor() as usize).min(last);
    let j = (i + 1).min(last);
    let t = at - i as f32;
    let (a, b) = (spans[i], spans[j]);
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Id as EguiId, Rect, pos2};

    /// A switch flipped while it is scrolled out of view is already in its new
    /// place when it comes back (the outputs menu: "All on", then scroll up,
    /// showed the knobs gliding across).
    #[test]
    fn a_switch_changed_out_of_view_does_not_glide_when_it_returns() {
        let ctx = context();
        let mut id = None;
        let mut frame = |on: bool, visible: bool| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                if !visible {
                    ui.set_clip_rect(Rect::from_min_size(pos2(-100.0, -100.0), vec2(1.0, 1.0)));
                }
                let mut on = on;
                id = Some(switch(ui, &mut on, "Out", true).id);
            });
            output.textures_delta.clear();
        };
        frame(false, true);
        frame(true, false);
        frame(true, true);
        let knob = id.expect("a switch was drawn").with("knob");
        assert_eq!(
            motion::glide(&ctx, knob, 1.0, true),
            1.0,
            "on at once, not gliding from off"
        );
    }

    #[test]
    fn sliding_lands_on_each_span_and_blends_between() {
        let spans = [(0.0, 10.0), (14.0, 44.0), (48.0, 60.0)];
        assert_eq!(slide_between(&spans, 0.0), (0.0, 10.0));
        assert_eq!(slide_between(&spans, 1.0), (14.0, 44.0));
        assert_eq!(slide_between(&spans, 2.0), (48.0, 60.0));
        assert_eq!(slide_between(&spans, 0.5), (7.0, 27.0));
        assert_eq!(slide_between(&spans, 1.5), (31.0, 52.0));
    }

    #[test]
    fn sliding_is_clamped_to_the_list() {
        let spans = [(0.0, 10.0), (14.0, 44.0)];
        assert_eq!(slide_between(&spans, -3.0), (0.0, 10.0));
        assert_eq!(slide_between(&spans, 7.0), (14.0, 44.0));
    }

    /// One frame at `t` seconds of `draw` in a strip whose left edge is
    /// at `x`: every shape it painted.
    fn frame(
        ctx: &egui::Context,
        t: f64,
        x: f32,
        draw: &mut dyn FnMut(&mut Ui),
    ) -> Vec<egui::Shape> {
        let input = egui::RawInput {
            time: Some(t),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let area = Rect::from_min_size(pos2(x, 20.0), vec2(400.0, 100.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| draw(ui));
        });
        output.textures_delta.clear();
        output.shapes.into_iter().map(|s| s.shape).collect()
    }

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        // Fonts take effect from the next frame.
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        ctx
    }

    /// The left edge of the 2 pt accent underline among `shapes`.
    fn underline_left(shapes: &[egui::Shape]) -> f32 {
        shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Rect(r)
                    if r.fill == theme::p().accent && (r.rect.height() - 2.0).abs() < 1e-3 =>
                {
                    Some(r.rect.left())
                }
                _ => None,
            })
            .expect("the underline was painted")
    }

    /// The left edge of the thumb (the only gradient mesh) among `shapes`.
    fn thumb_left(shapes: &[egui::Shape]) -> f32 {
        shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Mesh(m) => Some(m.calc_bounds().left()),
                _ => None,
            })
            .expect("the thumb was painted")
    }

    #[test]
    fn a_moving_tab_strip_carries_the_underline_rigidly() {
        let ctx = context();
        let tab = |label| Tab {
            label,
            note: None,
            disabled_because: None,
            tip: "",
        };
        let list = [tab("Clip"), tab("Mixer")];
        let mut draw = |ui: &mut Ui| {
            tabs_with(ui, EguiId::new("tabs"), 1, &list, TabsLayout::default());
        };
        let first = underline_left(&frame(&ctx, 0.0, 0.0, &mut draw));
        let still = underline_left(&frame(&ctx, 0.016, 0.0, &mut draw));
        assert_eq!(first, still);
        // The strip moves 37 pt (a panel resizes): so does the underline,
        // at once, with no glide of its own.
        let moved = underline_left(&frame(&ctx, 0.032, 37.0, &mut draw));
        assert!((moved - still - 37.0).abs() < 1e-3, "{still} -> {moved}");
    }

    #[test]
    fn a_moving_segmented_control_carries_the_thumb_rigidly() {
        let ctx = context();
        let list = [Choice::new(0, "Pointer"), Choice::new(1, "Draw")];
        let mut draw = |ui: &mut Ui| {
            segmented(ui, EguiId::new("tool"), 1, &list);
        };
        let first = thumb_left(&frame(&ctx, 0.0, 0.0, &mut draw));
        let still = thumb_left(&frame(&ctx, 0.016, 0.0, &mut draw));
        assert_eq!(first, still);
        let moved = thumb_left(&frame(&ctx, 0.032, 37.0, &mut draw));
        assert!((moved - still - 37.0).abs() < 1e-3, "{still} -> {moved}");
    }

    #[test]
    fn icons_sit_on_the_labels_optical_middle() {
        let ctx = context();
        let dot: fn(&egui::Painter, egui::Pos2, egui::Color32) = |painter, at, colour| {
            painter.circle_filled(at, 1.0, colour);
        };
        let list = [
            Choice::new(0, "Pointer").icon(dot),
            Choice::new(1, "Draw").icon(dot),
        ];
        let shapes = frame(&ctx, 0.0, 0.0, &mut |ui| {
            segmented_tool(ui, EguiId::new("tool"), 0, &list, None);
        });
        let dots: Vec<f32> = shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Circle(circle) if circle.radius == 1.0 => Some(circle.center.y),
                _ => None,
            })
            .collect();
        let labels: Vec<f32> = shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => {
                    Some(text.pos.y + crate::fonts::optical_middle(&text.galley))
                }
                _ => None,
            })
            .collect();
        assert_eq!(dots.len(), 2);
        assert_eq!(labels.len(), 2, "no keycap is drawn in the control (KC C)");
        for (dot, label) in dots.iter().zip(&labels) {
            assert!(
                (dot - label).abs() <= 0.5,
                "icon at {dot}, label's middle at {label}"
            );
        }
    }

    /// The colour each label is drawn in, by its text.
    fn label_colours(shapes: &[egui::Shape]) -> Vec<(String, Option<egui::Color32>)> {
        shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => {
                    Some((text.galley.text().to_owned(), text.override_text_color))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_tool_switch_is_grey_for_pointer_and_lit_for_draw() {
        // The owner's TL C: the resting tool is the neutral thumb, a drawing
        // tool is filled in the accent.
        let ctx = context();
        let list = [Choice::new(0, "Pointer"), Choice::new(1, "Draw")];
        let resting = frame(&ctx, 0.0, 0.0, &mut |ui| {
            segmented_tool(ui, EguiId::new("tool"), 0, &list, None);
        });
        let palette = theme::p();
        let on_accent = theme::lit(Tone::Accent).text;
        let colours = label_colours(&resting);
        assert!(
            colours.contains(&("Pointer".into(), Some(palette.text))),
            "{colours:?}"
        );
        assert!(
            !colours.iter().any(|(_, c)| *c == Some(on_accent)),
            "{colours:?}"
        );
        let drawing = frame(&ctx, 0.0, 0.0, &mut |ui| {
            segmented_tool(ui, EguiId::new("tool"), 1, &list, Some(Tone::Accent));
        });
        let colours = label_colours(&drawing);
        assert!(
            colours.contains(&("Draw".into(), Some(on_accent))),
            "{colours:?}"
        );
    }

    #[test]
    fn one_span_and_none() {
        assert_eq!(slide_between(&[(5.0, 9.0)], 0.0), (5.0, 9.0));
        assert_eq!(slide_between(&[(5.0, 9.0)], 3.0), (5.0, 9.0));
        assert_eq!(slide_between(&[], 1.0), (0.0, 0.0));
    }
}
