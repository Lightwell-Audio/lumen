//! The position readout: its texts laid out once a frame, and the piece, whose
//! bars are a field to type a place.

use super::*;

impl Transport {
    /// The position's texts, laid out, and the width of its piece: the
    /// piece keeps that width while a place is typed (the interface
    /// audit's T1).
    pub(super) fn position_text(
        &self,
        ui: &Ui,
        state: TransportSnapshot,
        compact: bool,
    ) -> PositionText {
        let counting = state.count_in_total > 0;
        let shown = if counting {
            record::count_in_ticks(
                &self.timeline,
                state.marker,
                state.count_in_left,
                state.sample_rate,
            )
        } else {
            state.ticks
        };
        let bbt = if counting {
            record::count_in_text(&self.timeline, shown)
        } else {
            position::format_bbt(&self.timeline, state.ticks)
        };
        let time = record::time_text(
            self.time_format,
            self.timeline.tempo.seconds_at(shown),
            state.sample_rate,
        );
        let c = theme::p();
        let bars_colour = if counting { c.text_secondary } else { c.text };
        let bars = ui
            .painter()
            .layout_no_wrap(format!("{bbt:>7}"), value_font(), bars_colour);
        let clock = ui
            .painter()
            .layout_no_wrap(time.clone(), value_font(), c.text_secondary);
        let pad = well_pad(compact);
        let gap = if compact {
            theme::space::XS
        } else {
            theme::space::S
        };
        let width = 2.0 * pad + bars.size().x + gap + clock.size().x;
        PositionText {
            bbt,
            time,
            bars,
            clock,
            bars_colour,
            pad,
            gap,
            width,
        }
    }

    /// The position, a piece of the numeric well: bars and beats, then the
    /// time, both Martian Mono. Clicking the bars types a place to go to, in
    /// the same piece. While counting in, it counts up through the bars
    /// before 1 in the secondary colour (M5 slice 6d).
    pub(super) fn position_piece(&mut self, ui: &mut Ui, piece: egui::Rect, text: PositionText) {
        let radius = widgets::well::piece_radius(piece.height());
        if let Some(typed) = &mut self.editing {
            let mut field = ui.new_child(egui::UiBuilder::new().max_rect(piece));
            let edit = field.add_sized(
                piece.size(),
                egui::TextEdit::singleline(typed)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::symmetric(text.pad as i8, 0))
                    .vertical_align(egui::Align::Center)
                    .font(value_font()),
            );
            widgets::raised::paint_focus(ui.painter(), piece, radius);
            edit.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Go to position")
            });
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            let (enter, escape) =
                ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if escape {
                self.editing = None;
                self.error = None;
            } else if edit.lost_focus() {
                if enter {
                    match position::parse(typed, &self.timeline) {
                        Ok(at) => {
                            self.locate(at);
                            self.error = None;
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                self.editing = None;
            }
            return;
        }
        let c = theme::p();
        let well_id = ui.id().with("position well");
        let bars_rect = egui::Rect::from_min_size(
            egui::pos2(piece.left() + text.pad, piece.top()),
            egui::vec2(text.bars.size().x, piece.height()),
        );
        let clock_rect = egui::Rect::from_min_size(
            egui::pos2(bars_rect.right() + text.gap, piece.top()),
            egui::vec2(text.clock.size().x, piece.height()),
        );
        let field = ui.interact(bars_rect, well_id.with("bars"), egui::Sense::click());
        let field = widgets::tip(
            field,
            "Position",
            None,
            Some("Click to type a bar or a time"),
        );
        let bbt = text.bbt;
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Position {bbt}"))
        });
        let time_node = ui.interact(clock_rect, well_id.with("time"), egui::Sense::hover());
        let time = text.time;
        time_node.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &time));
        widgets::well::wash_when(ui, piece, well_id, field.hovered());
        let centre = |rect: egui::Rect, galley: &std::sync::Arc<egui::Galley>| {
            (rect.center() - galley.size() / 2.0).round()
        };
        let (at_bars, at_clock) = (
            centre(bars_rect, &text.bars),
            centre(clock_rect, &text.clock),
        );
        ui.painter().galley(at_bars, text.bars, text.bars_colour);
        ui.painter().galley(at_clock, text.clock, c.text_secondary);
        if field.has_focus() {
            widgets::raised::paint_focus(ui.painter(), piece, radius);
        }
        if field.clicked() {
            self.editing = Some(bbt);
        }
    }
}

/// The position readout's texts, laid out once a frame (`position_text`).
pub(super) struct PositionText {
    bbt: String,
    time: String,
    bars: std::sync::Arc<egui::Galley>,
    clock: std::sync::Arc<egui::Galley>,
    bars_colour: egui::Color32,
    /// The room before the bars and after the time, and between them.
    pad: f32,
    gap: f32,
    /// The piece's width.
    pub(super) width: f32,
}
