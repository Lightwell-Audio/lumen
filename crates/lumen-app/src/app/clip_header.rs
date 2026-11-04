//! The clip editors' header (M9.5 S1, the owner's TL C and KC C): one strip
//! that the note editor and the audio editor both draw, so the two never
//! drift apart. Its parts, left to right, in this order:
//!
//! - the clip's colour bar, its name and a caption ([`HeaderStrip::title`]);
//! - a rule;
//! - the tool switch, Pointer and Draw ([`HeaderStrip::tools`]): grey while
//!   Pointer is chosen, filled accent while Draw is (the arrangement's rule),
//!   with the Draw key only in its tooltip;
//! - the grid chip ([`App::header_grid_chip`]);
//! - the editor's own action (Quantize for notes);
//! - at the right, the editor's view toggles and Pop out.
//!
//! Every control is a raised control (the design system, ADR-0052). Icons and
//! letters share one pixel-snapped centre line.

use eframe::egui::{
    self, Align, Color32, FontId, Layout, Painter, Pos2, Rect, Response, Sense, Ui, UiBuilder,
    pos2, vec2,
};
use eframe::emath::GuiRounding as _;

use crate::commands::Command;
use crate::icons::{self, Icon};
use crate::note_editor::glyphs;
use crate::theme::Tone;
use crate::timeview::TimeView;
use crate::widgets::{self, Choice, raised};
use crate::{App, fonts, motion, theme};

/// The strip's height: roomy enough for the name over its caption.
pub(crate) const HEADER: f32 = 40.0;
/// The clip's colour bar.
const BAR: egui::Vec2 = vec2(8.0, 22.0);
/// An icon beside a label in the grid chip.
const CHIP_ICON: f32 = 13.0;
/// The widest the name and caption get before they are cut.
const TITLE_WIDTH: f32 = 220.0;

/// What a click or drag in a clip editor does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tool {
    #[default]
    Pointer,
    Draw,
}

/// A header being drawn: its left-to-right row.
pub(crate) struct HeaderStrip {
    pub row: Ui,
}

impl HeaderStrip {
    /// Paints the strip's ground in `rect` of `ui` and opens its row.
    pub fn new(ui: &mut Ui, rect: Rect) -> Self {
        let palette = theme::p();
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, palette.raised);
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, palette.border),
        );
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(vec2(theme::space::M, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.set_clip_rect(rect.intersect(ui.clip_rect()));
        row.spacing_mut().item_spacing.x = theme::space::M;
        Self { row }
    }

    /// The colour bar, the clip's `name` over its `caption` (mono, hint),
    /// then a rule. Named "Clip <name>" for assistive technology.
    pub fn title(&mut self, name: &str, caption: &str, colour: Color32) {
        let row = &mut self.row;
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
        let name_galley = row.painter().layout_no_wrap(
            name.to_owned(),
            fonts::medium(theme::size::BODY),
            palette.text,
        );
        let caption_galley = row.painter().layout_no_wrap(
            caption.to_owned(),
            FontId::monospace(theme::size::SCALE),
            palette.text_hint,
        );
        // Cut a long name so the strip never grows (the title shortens).
        let width = name_galley
            .size()
            .x
            .max(caption_galley.size().x)
            .min(TITLE_WIDTH);
        let height = name_galley.size().y + caption_galley.size().y + 1.0;
        let (rect, response) = row.allocate_exact_size(vec2(width, height), Sense::hover());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("Clip {name}"))
        });
        let painter = row.painter().with_clip_rect(rect);
        let top = rect.top().round();
        let below = top + name_galley.size().y.round() + 1.0;
        painter.galley_with_override_text_color(pos2(rect.left(), top), name_galley, palette.text);
        painter.galley_with_override_text_color(
            pos2(rect.left(), below),
            caption_galley,
            palette.text_hint,
        );
        separator(row);
    }

    /// The tool switch: Pointer and Draw with their icons, 30 pt. Pointer
    /// chosen is the grey thumb; Draw chosen is filled accent (TL C). The
    /// Draw key is in Draw's tooltip, not in the control (KC C). `tips` say
    /// what each tool does in this editor. Returns the tool picked.
    pub fn tools(&mut self, app: &App, tool: Tool, tips: [&str; 2]) -> Option<Tool> {
        let draw_keys = app.keymap.keys(Command::DrawTool);
        let choices = [
            Choice::new(Tool::Pointer, "Pointer")
                .icon(glyphs::pointer)
                .named("Pointer tool")
                .tip(tips[0]),
            Choice::new(Tool::Draw, "Draw")
                .icon(glyphs::pencil)
                .keys(draw_keys.as_ref())
                .named("Draw tool")
                .tip(tips[1]),
        ];
        let lit = (tool == Tool::Draw).then_some(Tone::Accent);
        let id = self.row.id().with("editor tool");
        widgets::segmented_tool(&mut self.row, id, tool, &choices, lit)
    }

    /// The right-hand group, laid out right to left: the first control
    /// drawn is at the right edge.
    pub fn right(&mut self, add: impl FnOnce(&mut Ui)) {
        self.row
            .with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = theme::space::S;
                add(ui);
            });
    }
}

impl App {
    /// The grid chip: a magnet (lit while snapping), "Grid", the value at the
    /// editor's zoom in the mono face and an arrow; a click opens the grid
    /// menu.
    pub(crate) fn header_grid_chip(&mut self, row: &mut Ui, time: TimeView) {
        let palette = theme::p();
        let label = self.grid_label_at(time.left, time.zoom);
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
pub(crate) fn separator(row: &mut Ui) {
    let (rect, _) = row.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
    row.painter().rect_filled(rect, 0.0, theme::p().border);
}

/// A raised icon button that stays lit while `on` (`Tone::On`: `accent_muted`,
/// an accent icon and hairline, cross-fading), named `name` for assistive
/// technology.
pub(crate) fn icon_toggle(
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
