use eframe::egui::{self, FontId, Id as EguiId, Rect, Sense, Ui, pos2, vec2};

use crate::widgets::raised;
use crate::{light, motion, theme};

use super::slide_between;

#[derive(Debug, Clone, Copy)]
pub struct Tab<'a> {
    pub label: &'a str,
    pub note: Option<&'a str>,
    pub disabled_because: Option<&'a str>,
    pub tip: &'a str,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TabsLayout {
    pub flush: bool,
    pub right_from: Option<usize>,
    pub focus: Option<usize>,
}

pub fn tab_divider(ui: &Ui, x: f32, middle: f32) {
    let line = Rect::from_center_size(
        pos2(x, middle),
        vec2(theme::stroke::HAIRLINE, theme::size::TAB_DIVIDER),
    );
    ui.painter()
        .rect_filled(line, 0.0, theme::p().border_strong);
}

pub fn tabs_with(
    ui: &mut Ui,
    id: EguiId,
    chosen: usize,
    tabs: &[Tab<'_>],
    layout: TabsLayout,
) -> Option<usize> {
    let palette = theme::p();
    let gap = theme::space::XS;
    let note_room = theme::space::XS + theme::space::XXS;
    let mut picked = None;
    let mut spans = Vec::with_capacity(tabs.len());
    let mut bottom = 0.0;
    let laid: Vec<_> = tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let enabled = tab.disabled_because.is_none();
            let colour = if !enabled {
                palette.text_disabled
            } else if i == chosen {
                palette.text
            } else {
                palette.text_secondary
            };
            let galley = ui.painter().layout_no_wrap(
                tab.label.to_owned(),
                FontId::proportional(theme::size::BODY),
                colour,
            );
            let note = tab.note.map(|note| {
                let ink = if i == chosen {
                    palette.text_secondary
                } else {
                    palette.text_hint
                };
                let mut job = egui::text::LayoutJob::simple_singleline(
                    note.to_owned(),
                    FontId::proportional(theme::size::CAPTION),
                    ink,
                );
                job.wrap = egui::text::TextWrapping {
                    max_width: theme::size::TAB_NOTE,
                    max_rows: 1,
                    break_anywhere: true,
                    overflow_character: Some('\u{2026}'),
                };
                ui.painter().layout_job(job)
            });
            let left_room = if layout.flush && i == 0 {
                0.0
            } else {
                theme::space::M
            };
            let words = galley.size().x + note.as_ref().map_or(0.0, |n| note_room + n.size().x);
            let size = vec2(left_room + words + theme::space::M, theme::size::CONTROL);
            (enabled, colour, galley, note, left_room, size)
        })
        .collect::<Vec<_>>();
    let widths: Vec<f32> = laid.iter().map(|l| l.5.x).collect();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (i, (tab, (enabled, colour, galley, note, left_room, size))) in
            tabs.iter().zip(laid).enumerate()
        {
            if layout.right_from == Some(i) {
                let rest: f32 =
                    widths[i..].iter().sum::<f32>() + (widths.len() - i - 1) as f32 * gap;
                let room = (ui.available_width() - rest - gap).max(gap);
                let (spacer, _) = ui.allocate_exact_size(vec2(room, size.y), Sense::hover());
                tab_divider(ui, spacer.right() + gap, spacer.center().y);
            }
            let sense = if enabled {
                Sense::click()
            } else {
                Sense::hover()
            };
            let (rect, response) = ui.allocate_exact_size(size, sense);
            let selected = i == chosen;
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    enabled,
                    selected,
                    format!("{} tab", tab.label),
                )
            });
            if layout.focus == Some(i) {
                response.request_focus();
            }
            let response = match tab.disabled_because {
                Some(why) => response.on_disabled_hover_text(why).on_hover_text(why),
                None => response.on_hover_text(tab.tip),
            };
            let hover = response.hovered() && enabled && !selected;
            let label_right = rect.left() + left_room + galley.size().x;
            ui.painter().galley_with_override_text_color(
                pos2(
                    rect.left() + left_room,
                    rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                if hover { palette.text } else { colour },
            );
            if let Some(note) = note {
                let at = pos2(
                    label_right + note_room,
                    rect.center().y - note.size().y / 2.0 + theme::stroke::HAIRLINE,
                );
                ui.painter().galley(at, note, palette.text_hint);
            }
            raised::focus_ring(ui.painter(), &response, rect);
            if response.clicked() && !selected {
                picked = Some(i);
            }
            spans.push((rect.left() + left_room, rect.right() - theme::space::M));
            bottom = rect.bottom();
        }
    });
    let index = picked.unwrap_or(chosen);
    if index < spans.len() {
        let ctx = ui.ctx().clone();
        let at = motion::glide(&ctx, id.with("underline at"), index as f32, true);
        let (left, right) = slide_between(&spans, at);
        let line = Rect::from_min_max(
            pos2(left, bottom - theme::stroke::FOCUS),
            pos2(right, bottom),
        );
        light::glow(ui.painter(), light::LIT, line, 1.0, palette.accent, 1.0);
        ui.painter().rect_filled(line, 1.0, palette.accent);
    }
    picked
}
