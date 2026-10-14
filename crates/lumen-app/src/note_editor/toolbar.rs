//! The clip header strip (M5 slice 5, the owner's approved mock-up): the
//! clip's colour bar, name and "N bars · N notes"; the Pointer/Draw tools; the
//! grid chip; and the Lane and Pop out icon buttons at the right. Every
//! control is a raised control (the design system, ADR-0052): a toggle that
//! is on is `Tone::On`, and only the chosen tool is filled accent. Icons
//! and letters share one pixel-snapped centre line.

use std::sync::Arc;

use eframe::egui::{
    self, Align, Color32, FontId, Layout, Painter, Pos2, Rect, Response, Sense, Ui, UiBuilder,
    pos2, vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_core::time::Ticks;
use lumen_project::model::Clip;

use super::{NoteEditor, Tool, glyphs, notes_of};
use crate::commands::Command;
use crate::icons::{self, Icon};
use crate::theme::Tone;
use crate::widgets::{self, Choice, raised};
use crate::{App, fonts, motion, theme};

/// The strip's height: roomy enough for the name over its caption.
pub(super) const HEADER: f32 = 40.0;
/// The clip's colour bar.
const BAR: egui::Vec2 = vec2(8.0, 22.0);
/// An icon beside a label in the grid chip.
const CHIP_ICON: f32 = 13.0;

impl App {
    /// The strip: colour bar, name and counts, tools, grid, Lane, Pop out.
    pub(super) fn editor_toolbar(
        &mut self,
        ui: &mut Ui,
        toolbar: Rect,
        ed: &mut NoteEditor,
        clip: &Arc<Clip>,
        colour: Color32,
        editable: bool,
    ) {
        let palette = theme::p();
        let painter = ui.painter();
        painter.rect_filled(toolbar, 0.0, palette.raised);
        painter.hline(
            toolbar.x_range(),
            toolbar.bottom() - 0.5,
            egui::Stroke::new(1.0, palette.border),
        );
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(toolbar.shrink2(vec2(theme::space::M, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.set_clip_rect(toolbar.intersect(ui.clip_rect()));
        row.spacing_mut().item_spacing.x = theme::space::M;

        self.clip_title(&mut row, clip, colour, editable);
        separator(&mut row);

        let keymap = &self.keymap;
        let draw_key = keymap.text(Command::DrawTool);
        let choices = [
            Choice::new(Tool::Pointer, "Pointer")
                .icon(glyphs::pointer)
                .named("Pointer tool")
                .tip("Select, move and resize notes"),
            Choice::new(Tool::Draw, "Draw")
                .icon(glyphs::pencil)
                .hint(draw_key.as_deref())
                .named("Draw tool")
                .tip("Click to add a note, drag to set its length, click a note to remove it"),
        ];
        let id = ui.id().with("note tool");
        if let Some(tool) =
            widgets::segmented_styled(&mut row, id, ed.tool, &choices, Some(Tone::Accent))
        {
            ed.tool = tool;
        }
        self.grid_chip(&mut row, ed);
        if editable {
            self.quantize_button(&mut row, ed, clip);
        }

        row.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = theme::space::S;
            let keys = self.keymap.keys(Command::PopOutEditor);
            let (name, tip, glyph): (&str, &str, fn(&Painter, Pos2, Color32)) = if ed.popped {
                (
                    "Dock",
                    "Put the editor back in the detail panel",
                    glyphs::dock,
                )
            } else {
                (
                    "Pop out",
                    "Open the editor in its own window",
                    glyphs::pop_out,
                )
            };
            let response = raised::icon_button(ui, name, glyph);
            if widgets::tip(response, tip, keys.as_ref(), None).clicked() {
                ed.popped = !ed.popped;
            }
            let mut shown = !ed.lane.hidden;
            let response = icon_toggle(ui, "Lane", shown, glyphs::lane);
            if widgets::tip(
                response,
                "Show the velocity and controller lane under the notes",
                self.keymap.keys(Command::ToggleLane).as_ref(),
                None,
            )
            .clicked()
            {
                shown = !shown;
            }
            ed.lane.hidden = !shown;
        });
    }

    /// The colour bar, the clip's name and its counts.
    fn clip_title(&self, row: &mut Ui, clip: &Clip, colour: Color32, editable: bool) {
        let palette = theme::p();
        let ppp = row.pixels_per_point();
        let (bar, _) = row.allocate_exact_size(BAR, Sense::hover());
        let bar = bar.round_to_pixels(ppp);
        row.painter().rect_filled(bar, 3.0, colour);
        // The light top edge a lit thing has, as on the notes.
        row.painter().rect_filled(
            Rect::from_min_size(bar.min, vec2(bar.width(), bar.height() / 2.0)),
            3.0,
            Color32::WHITE.gamma_multiply(0.18),
        );
        let name = row.painter().layout_no_wrap(
            clip.name.clone(),
            fonts::medium(theme::size::BODY),
            palette.text,
        );
        let counts = self.clip_counts(clip, editable);
        let caption = row.painter().layout_no_wrap(
            counts,
            FontId::monospace(theme::size::SCALE),
            palette.text_hint,
        );
        // Cut a long name so the strip never grows (the title shortens).
        let width = name.size().x.max(caption.size().x).min(220.0);
        let height = name.size().y + caption.size().y + 1.0;
        let (rect, response) = row.allocate_exact_size(vec2(width, height), Sense::hover());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("Clip {}", clip.name))
        });
        let painter = row.painter().with_clip_rect(rect);
        let top = rect.top().round();
        painter.galley_with_override_text_color(pos2(rect.left(), top), name.clone(), palette.text);
        painter.galley_with_override_text_color(
            pos2(rect.left(), top + name.size().y.round() + 1.0),
            caption,
            palette.text_hint,
        );
    }

    /// "8 bars · 17 notes", with "· locked" when the clip cannot be edited.
    fn clip_counts(&self, clip: &Clip, editable: bool) -> String {
        let meter = &self.transport.timeline.meter;
        let first = meter.bbt_at(clip.start);
        let end = meter.bbt_at(clip.start + clip.length);
        let bars =
            (end.bar - first.bar + i64::from(end.beat > 0 || end.ticks > Ticks::ZERO)).max(1);
        let notes = notes_of(clip).len();
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        let mut text = format!(
            "{bars} bar{} \u{00B7} {notes} note{}",
            plural(bars as usize),
            plural(notes)
        );
        if !editable {
            text.push_str(" \u{00B7} locked");
        }
        text
    }

    /// The grid chip: a magnet (lit while snapping), "Grid", the value in
    /// the mono face and an arrow; a click opens the grid menu.
    fn grid_chip(&mut self, row: &mut Ui, ed: &NoteEditor) {
        let palette = theme::p();
        let label = self.grid_label_at(ed.time.left, ed.time.zoom);
        let word = row.painter().layout_no_wrap(
            "Grid".to_owned(),
            FontId::proportional(theme::size::BODY),
            palette.text_secondary,
        );
        let value = row.painter().layout_no_wrap(
            label.clone(),
            FontId::monospace(theme::size::CAPTION),
            palette.text,
        );
        let gap = theme::space::S;
        let arrow = 9.0;
        let width = theme::space::S * 2.0
            + CHIP_ICON
            + gap
            + word.size().x
            + gap
            + value.size().x
            + gap
            + arrow;
        let (rect, response) =
            row.allocate_exact_size(vec2(width, theme::size::CONTROL), Sense::click());
        let snap = self.grid.snap;
        response.widget_info(|| {
            let off = if snap { "" } else { ", snap off" };
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Editor grid: {label}{off}"),
            )
        });
        if row.is_rect_visible(rect) {
            let face = raised::face_of(row, &response);
            let rect = rect.round_to_pixels(row.pixels_per_point());
            let body = raised::paint(
                row.painter(),
                rect,
                theme::radius::for_height(rect.height()),
                &face,
            );
            let painter = row.painter();
            let mid = body.center().y.round();
            let mut x = (body.left() + theme::space::S).round();
            let (magnet, ink) = if snap {
                (Icon::Magnet, palette.accent)
            } else {
                (Icon::MagnetOff, palette.text_hint)
            };
            let icon =
                Rect::from_center_size(pos2(x + CHIP_ICON / 2.0, mid), vec2(CHIP_ICON, CHIP_ICON));
            painter.extend(icons::shapes(magnet, icon, ink));
            x += CHIP_ICON + gap;
            let top = |g: &egui::Galley| mid - (g.size().y / 2.0).round();
            painter.galley_with_override_text_color(pos2(x, top(&word)), word.clone(), face.text());
            x += word.size().x + gap;
            painter.galley_with_override_text_color(
                pos2(x, top(&value)),
                value.clone(),
                palette.text,
            );
            x += value.size().x + gap;
            // A down chevron, as in the mock-up.
            let c = pos2((x + arrow / 2.0).round(), mid);
            let stroke = egui::Stroke::new(1.5, palette.text_hint);
            painter.line_segment([c + vec2(-3.0, -1.5), c + vec2(0.0, 1.5)], stroke);
            painter.line_segment([c + vec2(0.0, 1.5), c + vec2(3.0, -1.5)], stroke);
            raised::focus_ring(painter, &response, rect);
        }
        let keys = self.keymap.keys(Command::SnapToGrid);
        let response = widgets::tip(
            response,
            "Grid and snapping",
            keys.as_ref(),
            Some("The magnet is lit while snapping is on"),
        );
        widgets::Menu::below(&response).show(|ui| self.grid_menu(ui));
    }
}

/// A thin rule between groups in the strip.
fn separator(row: &mut Ui) {
    let (rect, _) = row.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
    row.painter().rect_filled(rect, 0.0, theme::p().border);
}

/// A raised icon button that stays lit while `on` (`Tone::On`: `accent_muted`,
/// an accent icon and hairline, cross-fading), named `name` for assistive
/// technology.
fn icon_toggle(
    ui: &mut Ui,
    name: &str,
    on: bool,
    draw: impl FnOnce(&Painter, Pos2, Color32),
) -> Response {
    let size = vec2(theme::size::CONTROL, theme::size::CONTROL);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, name)
    });
    if ui.is_rect_visible(rect) {
        let lit = motion::hover(ui.ctx(), response.id.with("lit"), on);
        let face = raised::face_of(ui, &response).lit(lit, Tone::On);
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let body = raised::paint(
            ui.painter(),
            rect,
            theme::radius::for_height(rect.height()),
            &face,
        );
        draw(ui.painter(), body.center().round(), face.text());
        raised::focus_ring(ui.painter(), &response, rect);
    }
    response
}
