use eframe::egui::{
    self, Align2, CursorIcon, FontId, Id as EguiId, Rect, Sense, Stroke, Ui, pos2, vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{Project, TrackKind};

use super::{Arrangement, FOLDED};
use crate::document::Document;
use crate::theme::Tone;
use crate::widgets::{draw_arrow, draw_edge_glow, raised};
use crate::{fonts, motion, theme};

pub(super) const DIVIDER: f32 = 7.0;

pub(super) const CAPTION: f32 = 22.0;

const MOST: f32 = 0.6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Zones {
    pub main: f32,
    pub caption: Option<(f32, f32)>,
    pub returns: (f32, f32),
    pub master: (f32, f32),
    pub returns_content: f32,
}

pub(crate) fn letter(n: usize) -> String {
    let mut n = n;
    let mut text = String::new();
    loop {
        text.insert(0, char::from(b'A' + (n % 26) as u8));
        if n < 26 {
            return text;
        }
        n = n / 26 - 1;
    }
}

pub(crate) fn letter_of(project: &Project, id: lumen_project::model::Id) -> Option<String> {
    project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Return)
        .position(|t| t.id == id)
        .map(letter)
}

impl Arrangement {
    pub(super) fn master_height(&self, ctx: &egui::Context, project: &Project) -> f32 {
        motion::glide(
            ctx,
            EguiId::new(("track height", project.master.id)),
            self.target_height(project.master.id),
            true,
        )
    }

    pub(super) fn zones(&self, ctx: &egui::Context, project: &Project) -> Zones {
        let returns = project
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Return);
        let has_returns = returns.clone().next().is_some();
        let returns_content: f32 = if self.returns_folded {
            0.0
        } else {
            returns
                .map(|t| {
                    motion::glide(
                        ctx,
                        EguiId::new(("track height", t.id)),
                        self.target_height(t.id),
                        true,
                    )
                })
                .sum()
        };
        let caption = if has_returns { CAPTION } else { 0.0 };
        let master = self.master_height(ctx, project);
        let least = DIVIDER + caption + master;
        let natural = least + returns_content;
        let most = (self.view_height * MOST).max(least);
        let size = self
            .returns_size
            .unwrap_or(natural)
            .min(natural)
            .min(most)
            .max(least);
        let top = (self.view_height - size).max(0.0);
        let caption_top = top + DIVIDER;
        let returns_top = caption_top + caption;
        let master_top = top + size - master;
        Zones {
            main: top,
            caption: has_returns.then_some((caption_top, returns_top)),
            returns: (returns_top, master_top),
            master: (master_top, master_top + master),
            returns_content,
        }
    }

    pub(crate) fn pinned_controls(&mut self, ui: &mut Ui, full: Rect, doc: &Document) {
        let ctx = ui.ctx().clone();
        let palette = *theme::p();
        let project = doc.project();
        let zones = self.zones(&ctx, project);
        let at = |y: f32| full.top() + y;
        let painter = ui.painter_at(full);

        let divider = Rect::from_min_max(
            pos2(full.left(), at(zones.main)),
            pos2(full.right(), at(zones.main) + DIVIDER),
        );
        let id = ui.id().with("returns divider");
        let response = ui
            .interact(divider.expand2(vec2(0.0, 2.0)), id, Sense::click_and_drag())
            .on_hover_cursor(CursorIcon::ResizeVertical)
            .on_hover_text(
                "Drag to resize returns and master; double-click for their natural size",
            );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Returns and master divider")
        });
        if response.hovered() {
            use crate::app::info::{Held, Info, Mouse};
            self.info = Some(
                Info::new("Returns and master")
                    .on(Held::NONE, Mouse::Drag, "resize")
                    .on(Held::NONE, Mouse::DoubleClick, "natural size"),
            );
        }
        let size = self.view_height - zones.main;
        if response.drag_started() {
            self.sizing_returns = Some(size);
        }
        if response.dragged()
            && let Some(start) = self.sizing_returns
        {
            let moved = ui.input(|i| {
                i.pointer
                    .press_origin()
                    .zip(i.pointer.interact_pos())
                    .map(|(a, b)| b.y - a.y)
            });
            if let Some(moved) = moved {
                self.returns_size = Some((start - moved).max(0.0).round());
            }
        }
        if response.drag_stopped() {
            self.sizing_returns = None;
            let natural = zones.returns_content
                + DIVIDER
                + zones.caption.map_or(0.0, |(a, b)| b - a)
                + (zones.master.1 - zones.master.0);
            if self.returns_size.is_some_and(|s| s >= natural) {
                self.returns_size = None;
            }
        }
        if response.double_clicked() {
            self.returns_size = None;
        }
        painter.rect_filled(divider, 0.0, palette.raised);
        painter.hline(
            divider.x_range(),
            divider.top() + 0.5,
            Stroke::new(theme::stroke::HAIRLINE, palette.border_strong),
        );
        painter.hline(
            divider.x_range(),
            divider.bottom() - 0.5,
            Stroke::new(theme::stroke::HAIRLINE, palette.border_strong),
        );
        let grip = Rect::from_center_size(divider.center(), vec2(28.0, 2.0));
        painter.rect_filled(grip, 1.0, palette.text_hint);
        let glow = motion::hover(
            &ctx,
            id.with("glow"),
            response.hovered() || self.sizing_returns.is_some(),
        );
        if glow > 0.0 {
            draw_edge_glow(&painter, divider.x_range(), divider.center().y, glow);
        }

        if let Some((top, bottom)) = zones.caption {
            let rect =
                Rect::from_min_max(pos2(full.left(), at(top)), pos2(full.right(), at(bottom)));
            let count = project
                .tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Return)
                .count();
            let id = ui.id().with("returns caption");
            let response =
                ui.interact(rect, id, Sense::click())
                    .on_hover_text(if self.returns_folded {
                        "Show the return tracks"
                    } else {
                        "Hide the return tracks"
                    });
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    true,
                    self.returns_folded,
                    "Fold returns",
                )
            });
            if response.clicked() {
                self.returns_folded = !self.returns_folded;
            }
            let hover = motion::hover(&ctx, id.with("hover"), response.hovered());
            painter.rect_filled(rect, 0.0, palette.header);
            painter.hline(
                rect.x_range(),
                rect.bottom() - 0.5,
                Stroke::new(theme::stroke::HAIRLINE, palette.border),
            );
            let open = motion::glide(
                &ctx,
                id.with("open"),
                if self.returns_folded { 0.0 } else { 1.0 },
                true,
            );
            let ink = palette.text_hint.lerp_to_gamma(palette.text, hover);
            draw_arrow(
                &painter,
                pos2(rect.left() + 18.0, rect.center().y),
                open,
                ink,
            );
            painter.text(
                pos2(rect.left() + 30.0, rect.center().y),
                Align2::LEFT_CENTER,
                "Returns",
                fonts::medium(theme::size::CAPTION),
                ink,
            );
            painter.text(
                pos2(self.header_width() - 12.0 + full.left(), rect.center().y),
                Align2::RIGHT_CENTER,
                count.to_string(),
                FontId::proportional(theme::size::CAPTION),
                palette.text_hint,
            );
        }
    }

    pub(crate) fn pinned_wheel(&mut self, ui: &mut Ui, full: Rect, project: &Project) {
        let zones = self.zones(ui.ctx(), project);
        let at = |y: f32| full.top() + y;
        let returns = Rect::from_min_max(
            pos2(full.left(), at(zones.returns.0)),
            pos2(full.right(), at(zones.returns.1)),
        );
        let room = zones.returns.1 - zones.returns.0;
        let spare = (zones.returns_content - room).max(0.0);
        if spare > 0.0
            && ui.rect_contains_pointer(returns)
            && !ui.input(|i| i.modifiers.alt || i.modifiers.command)
        {
            let delta = ui.input(|i| i.smooth_scroll_delta.y);
            if delta != 0.0 {
                self.returns_top = (self.returns_top - delta).clamp(0.0, spare);
                ui.input_mut(|i| i.smooth_scroll_delta.y = 0.0);
            }
        }
        self.returns_top = self.returns_top.clamp(0.0, spare);
    }

    pub(super) fn pinned_lanes(
        &self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        canvas: Rect,
        project: &Project,
        rows: &[super::Row],
    ) {
        let palette = *theme::p();
        let zones = self.zones(ctx, project);
        let area = Rect::from_min_max(
            pos2(
                canvas.left(),
                (canvas.top() + zones.main).min(canvas.bottom()),
            ),
            canvas.max,
        );
        if area.height() <= 0.0 {
            return;
        }
        let painter = painter.with_clip_rect(area.intersect(painter.clip_rect()));
        painter.rect_filled(
            area,
            0.0,
            palette.canvas.gamma_multiply(theme::alpha::INK_DIM),
        );
        for row in rows.iter().filter(|r| r.zone != super::Zone::Main) {
            let span = self.zone_span(ctx, project, row.zone, canvas.y_range());
            let lane = Rect::from_min_size(
                pos2(canvas.left(), canvas.top() + row.y - self.top),
                vec2(canvas.width(), row.height),
            );
            let clip = Rect::from_x_y_ranges(canvas.x_range(), span);
            if !clip.intersects(lane) {
                continue;
            }
            let painter = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
            match row.kind {
                super::RowKind::Track(index) => {
                    let track = &project.tracks[index];
                    if track.solo {
                        painter.rect_filled(
                            lane,
                            0.0,
                            palette.accent.gamma_multiply(theme::alpha::WASH_LANE),
                        );
                    }
                    painter.hline(
                        canvas.x_range(),
                        lane.bottom() - 0.5,
                        Stroke::new(
                            theme::stroke::HAIRLINE,
                            palette.border.gamma_multiply(theme::alpha::EDGE_DIM),
                        ),
                    );
                    if let Some((hot, t)) = self.hot_edge
                        && hot == track.id
                    {
                        draw_edge_glow(&painter, canvas.x_range(), lane.bottom(), t);
                    }
                }
                super::RowKind::Master => {
                    painter.hline(
                        canvas.x_range(),
                        lane.top() + 0.5,
                        Stroke::new(theme::stroke::HAIRLINE, palette.border),
                    );
                }
                super::RowKind::Pending(_) => {}
            }
        }
    }

    pub(super) fn cue_switch(&mut self, ui: &mut Ui, rect: Rect, mid: f32, place: &str) {
        if !self.cue.ready {
            return;
        }
        let switch = Rect::from_min_size(
            pos2(
                rect.right() - super::headers::CONTENT_RIGHT - 46.0,
                mid - 9.0,
            ),
            vec2(46.0, 18.0),
        );
        let on = self.cue.mode;
        let response = ui
            .interact(switch, ui.id().with(("solo cue switch", place)), Sense::click())
            .on_hover_text(
                "Solo or Cue: on Cue, the tracks' S buttons send them to the headphones, before the fader",
            );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                true,
                on,
                format!("Solo/Cue switch: Cue{place}"),
            )
        });
        if response.clicked() {
            self.cue.mode = !self.cue.mode;
        }
        let hover = motion::hover(
            ui.ctx(),
            ui.id().with(("solo cue hover", place)),
            response.hovered(),
        );
        let lit = motion::hover(ui.ctx(), ui.id().with(("solo cue lit", place)), on);
        let face = raised::Face::new(theme::controls(false))
            .hover(hover)
            .pressed(response.is_pointer_button_down_on())
            .lit(lit, Tone::On);
        let switch = switch.round_to_pixels(ui.pixels_per_point());
        let body = raised::paint(
            ui.painter(),
            switch,
            theme::radius::for_height(switch.height()),
            &face,
        );
        let ink = face.text();
        let painter = ui.painter();
        if on {
            crate::icons::draw_headphones(
                painter,
                pos2(switch.left() + 11.0, body.center().y),
                ink,
            );
        }
        painter.text(
            pos2(
                body.center().x + if on { 5.0 } else { 0.0 },
                body.center().y,
            ),
            Align2::CENTER_CENTER,
            if on { "Cue" } else { "Solo" },
            fonts::medium(theme::size::CAPTION),
            ink,
        );
        raised::focus_ring(painter, &response, switch);
    }

    pub(super) fn master_header(&mut self, ui: &mut Ui, doc: &mut Document, rect: Rect) {
        let palette = *theme::p();
        let master = doc.project().master.clone();
        let id = EguiId::new(("track header", master.id));
        let selected = self.selected.contains(&master.id);
        let look = theme::controls(selected);
        let painter = ui.painter_at(rect);
        let background = ui.interact(rect, id, Sense::click());
        background.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                selected,
                "Track Master, master track",
            )
        });
        if background.clicked() {
            let modifiers = ui.input(|i| i.modifiers);
            self.click_select(doc.project(), master.id, modifiers);
        }
        painter.rect_filled(
            rect,
            0.0,
            if selected {
                palette.selected_row
            } else {
                palette.header
            },
        );
        let alt = ui.input(|i| i.modifiers.alt);
        self.device_drop_target(ui.ctx(), &painter, doc, rect, master.id, alt);
        let rail = Rect::from_min_size(rect.min, vec2(super::nesting::RAIL, rect.height()));
        painter.rect_filled(rail, 0.0, palette.text_hint);
        let mid = super::headers::name_center(rect.top(), rect.height());
        let open = self.expanded.contains(&master.id);
        let fold_rect = Rect::from_min_size(
            pos2(
                rect.left() + super::headers::CONTENT_X,
                mid - super::headers::FOLD / 2.0,
            ),
            vec2(super::headers::FOLD, super::headers::FOLD),
        )
        .round_to_pixels(ui.pixels_per_point());
        let fold = ui
            .interact(fold_rect, id.with("fold button"), Sense::click())
            .on_hover_text(if open {
                "Master track \u{b7} fold the master"
            } else {
                "Master track \u{b7} unfold the master (volume and pan)"
            });
        fold.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Fold Master")
        });
        if fold.clicked() {
            self.toggle_fold(master.id);
        }
        let unfolded = motion::glide(
            ui.ctx(),
            id.with("fold"),
            if open { 1.0 } else { 0.0 },
            true,
        );
        let pointed = fold.has_focus() || ui.rect_contains_pointer(rect);
        let arrow = motion::hover(ui.ctx(), id.with("fold shows arrow"), pointed);
        let ink = if fold.hovered() {
            palette.text
        } else {
            palette.text_secondary
        };
        if arrow < 1.0 {
            crate::icons::draw_type_icon(
                &painter,
                fold_rect.center(),
                TrackKind::Master,
                ink.gamma_multiply(1.0 - arrow),
            );
        }
        if arrow > 0.0 {
            draw_arrow(
                &painter,
                fold_rect.center(),
                unfolded,
                ink.gamma_multiply(arrow),
            );
        }
        raised::focus_ring(&painter, &fold, fold_rect);
        let name_left = fold_rect.right() + super::headers::GAP;
        painter.text(
            pos2(name_left, mid),
            Align2::LEFT_CENTER,
            &master.name,
            fonts::medium(theme::size::BODY),
            if master.mute {
                palette.text_hint
            } else {
                palette.text
            },
        );
        let name = ui.interact(
            Rect::from_min_max(
                pos2(name_left, mid - 10.0),
                pos2(rect.right() - 10.0, mid + 10.0),
            ),
            id.with("name"),
            Sense::hover(),
        );
        name.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Master track")
        });
        self.cue_switch(ui, rect, mid, "");
        self.level_strip(ui, rect, rect.left(), &master, (id, true));
        if rect.height() >= FOLDED + 22.0 {
            self.mixer_row(ui, doc, &master, (rect, rect.left()), (id, look));
        }
        let (start, _, third) = super::headers::columns(rect, rect.left());
        if self.cue.ready && rect.height() >= third.bottom() - rect.top() {
            let y = third.center().y;
            let label = Rect::from_min_max(pos2(start, y - 10.0), pos2(start + 34.0, y + 10.0));
            painter.text(
                label.left_center(),
                Align2::LEFT_CENTER,
                "Cue",
                FontId::proportional(theme::size::CAPTION),
                palette.text_secondary,
            );
            let slider = Rect::from_min_max(
                pos2(label.right(), y - 10.0),
                pos2(rect.right() - super::headers::CONTENT_RIGHT, y + 10.0),
            );
            let db = crate::widgets::value_slider(
                ui,
                slider,
                id.with("cue volume"),
                self.cue.volume_db,
                crate::widgets::SliderKind::Volume,
                "Cue volume",
                look,
            );
            self.cue.volume_db = db;
        }
        painter.hline(
            rect.x_range(),
            rect.top() + 0.5,
            Stroke::new(theme::stroke::HAIRLINE, palette.border),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_are_lettered_like_spreadsheet_columns() {
        let letters: Vec<String> = [0, 1, 25, 26, 27, 51, 52, 701, 702]
            .into_iter()
            .map(letter)
            .collect();
        assert_eq!(
            letters,
            ["A", "B", "Z", "AA", "AB", "AZ", "BA", "ZZ", "AAA"]
        );
    }
}
