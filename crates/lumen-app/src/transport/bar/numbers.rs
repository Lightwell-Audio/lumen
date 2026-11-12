//! The numeric well: tempo and its Tap piece, the time signature, and the
//! position at the end.

use super::*;

impl Transport {
    /// Tempo, meter and position as the pieces of one 30 pt numeric well
    /// (the interface audit's TB1), hairlines between them. The values are
    /// Martian Mono, units in `text_hint`; the piece under the pointer
    /// takes the Ember wash.
    pub(super) fn numbers(
        &mut self,
        ui: &mut Ui,
        doc: &mut crate::document::Document,
        keymap: &Keymap,
        state: TransportSnapshot,
        plan: fit::Plan,
    ) {
        let compact = plan.compact;
        // The tempo and the time signature in force at the playhead: with
        // changes along the song, the fields show and edit the segment the
        // playhead is in (M7 slice A6, the owner's choice).
        let mut bpm = lumen_project::edit::maps::tempo_in(&doc.project().tempo, state.ticks);
        let pad = well_pad(compact);
        let unit = (!compact).then_some("BPM");
        let value_w = fit::text_width(ui, "999.00", value_font());
        let unit_w = unit.map_or(0.0, |unit| {
            theme::space::S + fit::text_width(ui, unit, unit_font())
        });
        let position = self.position_text(ui, state, compact);
        // The Tap piece, when it fits, follows the tempo (the owner's TT A).
        let tap = plan.tap.then(|| tap_width(ui));
        let widths: Vec<f32> = [Some(2.0 * pad + value_w + unit_w), tap]
            .into_iter()
            .flatten()
            .chain([widgets::signature::WIDTH, position.width])
            .collect();
        let (well, _) = ui.allocate_exact_size(
            egui::vec2(widgets::well::width(&widths), theme::size::CONTROL_LARGE),
            egui::Sense::hover(),
        );
        let well = well.round_to_pixels(ui.pixels_per_point());
        let pieces = widgets::well::paint(ui.painter(), well, &widths);
        widgets::well::dividers(ui.painter(), &pieces);

        // Tempo: the drag value fills the value's part of its piece, with no
        // fill or outline of its own: the well is its ground.
        let tempo_piece = pieces[0];
        widgets::well::wash(ui, tempo_piece, ui.id().with("tempo well"));
        let value_rect = egui::Rect::from_min_max(
            egui::pos2(tempo_piece.left() + pad, tempo_piece.top()),
            egui::pos2(tempo_piece.left() + pad + value_w, tempo_piece.bottom()),
        );
        let mut field = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(value_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        field.style_mut().drag_value_text_style = egui::TextStyle::Monospace;
        field.spacing_mut().button_padding.x = 0.0;
        field.set_clip_rect(value_rect.intersect(ui.clip_rect()));
        field.style_mut().visuals.text_edit_bg_color = Some(theme::p().well);
        for state in {
            let w = &mut field.style_mut().visuals.widgets;
            [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open]
        } {
            state.bg_fill = egui::Color32::TRANSPARENT;
            state.weak_bg_fill = egui::Color32::TRANSPARENT;
            state.bg_stroke = egui::Stroke::NONE;
            state.corner_radius =
                egui::CornerRadius::same(widgets::well::piece_radius(value_rect.height()));
        }
        let tempo = field.add_sized(
            value_rect.size(),
            egui::DragValue::new(&mut bpm)
                .range(20.0..=999.0)
                // Typed values apply on Enter or leaving the field: live,
                // "1" on the way to "125" played at 20 BPM and sent the
                // playhead past the audio (owner, after slice 4a).
                .update_while_editing(false)
                .speed(0.1)
                .fixed_decimals(2),
        );
        let tempo = widgets::tip(
            tempo,
            "Tempo",
            None,
            Some("20 to 999 BPM. Drag, or click to type."),
        );
        tempo.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, "Tempo"));
        if let Some(unit) = unit {
            let galley =
                ui.painter()
                    .layout_no_wrap(unit.to_owned(), unit_font(), theme::p().text_hint);
            let at = egui::pos2(
                value_rect.right() + theme::space::S,
                tempo_piece.center().y - galley.size().y / 2.0,
            );
            ui.painter()
                .galley(at.round(), galley, theme::p().text_hint);
        }
        if tempo.changed() {
            // A drag is one undo step (merged until the pointer is released).
            doc.set_tempo_at(state.ticks, bpm, Some("tempo"));
        }

        let mut next = 1;
        if let Some(width) = tap {
            self.tap_piece(ui, pieces[next], keymap);
            self.fit.spent(width + widgets::well::PIECE_GAP);
            next += 1;
        }

        // Time signature: one field (the interface audit's S7).
        let here = self.timeline.meter.meter_at(state.ticks);
        let current = (here.numerator, here.denominator);
        if let Some((beats, unit)) = widgets::time_signature(
            ui,
            pieces[next],
            ui.id().with("time signature"),
            current,
            crate::document::MAX_BEATS_PER_BAR,
        ) {
            doc.set_time_signature_at(state.ticks, beats, unit, Some("meter"));
        }

        self.position_piece(ui, pieces[next + 1], position);
        if let Some(error) = &self.error {
            let warn = ui
                .label(RichText::new("⚠").color(theme::p().warning))
                .on_hover_text(error.as_str());
            let name = format!("Position error: {error}");
            warn.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
        }
    }

    /// The Tap piece (M9 R7, the owner's TT A): "Tap" until a run starts,
    /// then the run's taps counted, so its state is read, not only seen.
    /// A click is the Tap tempo command, timed by the app.
    fn tap_piece(&mut self, ui: &mut Ui, piece: egui::Rect, keymap: &Keymap) {
        let id = ui.id().with("tap tempo");
        let response = ui.interact(piece, id, egui::Sense::click());
        let keys = keymap.keys(Run::TapTempo);
        crate::testing::coverage::drawn(ui.ctx(), "Tap tempo", Run::TapTempo);
        let response = widgets::tip(
            response,
            "Tap tempo",
            keys.as_ref(),
            Some("Tap in time: the tempo follows from the third tap"),
        );
        // The count is the button's value, so a screen reader hears it too.
        let taps = self.taps;
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Tap tempo");
            info.value = (taps > 0).then_some(taps as f64);
            info
        });
        widgets::well::wash_when(ui, piece, id, response.hovered());
        let c = theme::p();
        let (text, font, colour) = if self.taps == 0 {
            ("Tap".to_owned(), unit_font(), c.text_secondary)
        } else {
            (self.taps.to_string(), value_font(), c.accent)
        };
        let galley = ui.painter().layout_no_wrap(text, font, colour);
        let at = (piece.center() - galley.size() / 2.0).round();
        ui.painter().galley(at, galley, colour);
        if response.has_focus() {
            let radius = widgets::well::piece_radius(piece.height());
            widgets::raised::paint_focus(ui.painter(), piece, radius);
        }
        if response.clicked() {
            self.requests.push(Run::TapTempo);
        }
    }
}
