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
//! - at the right, the editor's view toggles and Pop out, drawn first so
//!   the left parts take only the room left over.
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
use crate::note_editor::{Tool, glyphs};
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
/// The name alone, shortened, when the header is narrow.
const TITLE_SHORT: f32 = 64.0;
/// The room of the grid chip's chevron.
const CHIP_ARROW: f32 = 9.0;

/// A rule's width.
const RULE: f32 = 1.0;

/// How much of itself the grid chip shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GridChip {
    /// The magnet, "Grid", the value and the arrow.
    Full,
    /// Without the word.
    Value,
    /// The magnet alone.
    Icon,
}

/// A header being drawn: its left-to-right row, which ends where the
/// right-hand group begins.
pub(crate) struct HeaderStrip {
    pub row: Ui,
}

impl HeaderStrip {
    /// Paints the strip's ground in `rect` of `ui`, draws the right-hand
    /// group with `right` (laid out right to left: the first control drawn is
    /// at the right edge), then opens the left-to-right row in the room that
    /// is left. The row is clipped to that room, so in a narrow panel the
    /// left parts are cut rather than drawn over the right ones.
    pub fn new(ui: &mut Ui, rect: Rect, right: impl FnOnce(&mut Ui)) -> Self {
        let palette = theme::p();
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, palette.raised);
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(theme::stroke::HAIRLINE, palette.border),
        );
        let inner = rect.shrink2(vec2(theme::space::M, 0.0));
        let clip = rect.intersect(ui.clip_rect());
        let mut group = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::right_to_left(Align::Center)),
        );
        group.set_clip_rect(clip);
        group.spacing_mut().item_spacing.x = theme::space::S;
        right(&mut group);
        let used = group.min_rect();
        let end = if used.width() > 0.0 {
            used.left() - theme::space::M
        } else {
            inner.right()
        };
        let room = Rect::from_x_y_ranges(inner.left()..=end.max(inner.left()), inner.y_range());
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(room)
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.set_clip_rect(Rect::from_x_y_ranges(
            clip.left()..=end.max(clip.left()),
            clip.y_range(),
        ));
        row.spacing_mut().item_spacing.x = theme::space::M;
        Self { row }
    }

    /// The colour bar, the clip's `name` over its `caption` (mono, hint),
    /// then a rule. Named "Clip <name>" for assistive technology.
    pub fn title(&mut self, name: &str, caption: &str, colour: Color32) {
        let row = &mut self.row;
        let palette = theme::p();
        colour_bar(row, colour);
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

    /// The whole title's width: what [`Self::title`] takes, rule included.
    pub fn title_width(ui: &Ui, name: &str, caption: &str) -> f32 {
        let measure = |text: &str, font| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
                .size()
                .x
        };
        let words = measure(name, fonts::medium(theme::size::BODY))
            .max(measure(caption, FontId::monospace(theme::size::SCALE)))
            .min(TITLE_WIDTH);
        BAR.x + words + 2.0 * theme::space::M + RULE
    }

    /// The name alone, shortened with "…" to [`TITLE_SHORT`], then a rule.
    pub fn title_short(&mut self, name: &str, colour: Color32) {
        let row = &mut self.row;
        let palette = theme::p();
        colour_bar(row, colour);
        let mut job = egui::text::LayoutJob::simple_singleline(
            name.to_owned(),
            fonts::medium(theme::size::BODY),
            palette.text,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(TITLE_SHORT);
        let galley = row.painter().layout_job(job);
        let (rect, response) =
            row.allocate_exact_size(vec2(TITLE_SHORT, galley.size().y), Sense::hover());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("Clip {name}"))
        });
        row.painter()
            .galley_with_override_text_color(rect.min.round(), galley, palette.text);
        separator(row);
    }

    pub fn title_short_width() -> f32 {
        BAR.x + TITLE_SHORT + 2.0 * theme::space::M + RULE
    }

    /// Only the clip's colour bar, which keeps the title's name for assistive
    /// technology.
    pub fn title_bar(&mut self, name: &str, colour: Color32) {
        let response = colour_bar(&mut self.row, colour);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("Clip {name}"))
        });
    }

    pub fn title_bar_width() -> f32 {
        BAR.x
    }

    /// The tool switch: Pointer and Draw with their icons, 30 pt. Pointer
    /// chosen is the grey thumb; Draw chosen is filled accent (TL C). The
    /// Draw key is in Draw's tooltip, not in the control (KC C). `tips` say
    /// what each tool does in this editor. Returns the tool picked.
    pub fn tools(&mut self, app: &App, tool: Tool, tips: [&str; 2]) -> Option<Tool> {
        self.tools_with(app, tool, tips, true)
    }

    /// [`Self::tools`], with its words or as icons alone (`words` false);
    /// the names and tooltips stay.
    pub fn tools_with(
        &mut self,
        app: &App,
        tool: Tool,
        tips: [&str; 2],
        words: bool,
    ) -> Option<Tool> {
        let draw_keys = app.keymap.keys(Command::DrawTool);
        let choices = tool_choices(tips, draw_keys.as_ref(), words);
        let lit = (tool == Tool::Draw).then_some(Tone::Accent);
        let id = self.row.id().with("editor tool");
        widgets::segmented_tool(&mut self.row, id, tool, &choices, lit)
    }

    /// The tool switch's width, with its words or without.
    pub fn tools_width(ui: &Ui, words: bool) -> f32 {
        widgets::segmented_tool_width(ui, &tool_choices(["", ""], None, words))
    }
}

/// The tool switch's two choices.
fn tool_choices<'a>(
    tips: [&'a str; 2],
    draw_keys: Option<&'a widgets::Keys>,
    words: bool,
) -> [Choice<'a, Tool>; 2] {
    let word = |w: &'a str| if words { w } else { "" };
    [
        Choice::new(Tool::Pointer, word("Pointer"))
            .icon(glyphs::pointer)
            .named("Pointer tool")
            .tip(tips[0]),
        Choice::new(Tool::Draw, word("Draw"))
            .icon(glyphs::pencil)
            .keys(draw_keys)
            .named("Draw tool")
            .tip(tips[1]),
    ]
}

impl App {
    /// The grid chip's widths: as a whole, without its word, as its icon.
    pub(crate) fn grid_chip_widths(&self, ui: &Ui, time: TimeView) -> (f32, f32, f32) {
        let measure = |text: String, font| {
            ui.painter()
                .layout_no_wrap(text, font, Color32::PLACEHOLDER)
                .size()
                .x
        };
        let word = measure("Grid".to_owned(), FontId::proportional(theme::size::BODY));
        let value = measure(
            self.grid_label_at(time.left, time.zoom),
            FontId::monospace(theme::size::CAPTION),
        );
        let gap = theme::space::S;
        let without = 2.0 * gap + CHIP_ICON + gap + value + gap + CHIP_ARROW;
        (without + word + gap, without, theme::size::CONTROL)
    }

    /// The grid chip: a magnet (lit while snapping), "Grid", the value at the
    /// editor's zoom in the mono face and an arrow; a click opens the grid
    /// menu. `shows` says how much of that fits.
    pub(crate) fn header_grid_chip(&mut self, row: &mut Ui, time: TimeView, shows: GridChip) {
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
        let (full, without, icon_only) = self.grid_chip_widths(row, time);
        let width = match shows {
            GridChip::Full => full,
            GridChip::Value => without,
            GridChip::Icon => icon_only,
        };
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
            let (magnet, ink) = if snap {
                (Icon::Magnet, palette.accent)
            } else {
                (Icon::MagnetOff, palette.text_hint)
            };
            if shows == GridChip::Icon {
                let icon =
                    Rect::from_center_size(body.center().round(), vec2(CHIP_ICON, CHIP_ICON));
                painter.extend(icons::shapes(magnet, icon, ink));
            } else {
                let mut x = (body.left() + theme::space::S).round();
                let icon = Rect::from_center_size(
                    pos2(x + CHIP_ICON / 2.0, mid),
                    vec2(CHIP_ICON, CHIP_ICON),
                );
                painter.extend(icons::shapes(magnet, icon, ink));
                x += CHIP_ICON + gap;
                let top = |g: &egui::Galley| mid - (g.size().y / 2.0).round();
                if shows == GridChip::Full {
                    painter.galley_with_override_text_color(
                        pos2(x, top(&word)),
                        word.clone(),
                        face.text(),
                    );
                    x += word.size().x + gap;
                }
                painter.galley_with_override_text_color(
                    pos2(x, top(&value)),
                    value.clone(),
                    palette.text,
                );
                x += value.size().x + gap;
                // A down chevron, as in the mock-up.
                let c = pos2((x + CHIP_ARROW / 2.0).round(), mid);
                let stroke = egui::Stroke::new(theme::stroke::LIT, palette.text_hint);
                painter.line_segment([c + vec2(-3.0, -1.5), c + vec2(0.0, 1.5)], stroke);
                painter.line_segment([c + vec2(0.0, 1.5), c + vec2(3.0, -1.5)], stroke);
            }
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

/// The clip's colour bar, with the light top edge a lit thing has, as on the
/// notes.
fn colour_bar(row: &mut Ui, colour: Color32) -> Response {
    let (bar, response) = row.allocate_exact_size(BAR, Sense::hover());
    let bar = bar.round_to_pixels(row.pixels_per_point());
    row.painter().rect_filled(bar, 3.0, colour);
    row.painter().rect_filled(
        Rect::from_min_size(bar.min, vec2(bar.width(), bar.height() / 2.0)),
        3.0,
        Color32::WHITE.gamma_multiply(0.18),
    );
    response
}

/// A thin rule between groups in the strip.
pub(crate) fn separator(row: &mut Ui) {
    let (rect, _) = row.allocate_exact_size(vec2(RULE, 24.0), Sense::hover());
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
