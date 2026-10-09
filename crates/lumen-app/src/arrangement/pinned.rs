//! Returns and the master pinned at the bottom of the arrangement (M4
//! slice 4, the owner's A1, as in Ableton): a divider, a "Returns" caption
//! that folds them away, the return tracks lettered A, B, C, and the
//! master's row last. The tracks above scroll; this area stays put. Its
//! height is dragged at the divider (double-click: back to its natural
//! size) and saved with the project with the fold (format v10).
//!
//! Returns can sit anywhere in the project's track list: the arrangement
//! shows them here in list order, so the file needs no order rule.

use eframe::egui::{
    self, Align2, CursorIcon, FontId, Id as EguiId, Rect, Sense, Stroke, Ui, pos2, vec2,
};
use lumen_project::model::{Project, TrackKind};

use super::{Arrangement, FOLDED};
use crate::document::Document;
use crate::widgets::{draw_arrow, draw_edge_glow};
use crate::{fonts, motion, theme};

/// The divider above the pinned area.
pub(super) const DIVIDER: f32 = 7.0;

/// The "Returns" caption row.
pub(super) const CAPTION: f32 = 22.0;

/// The pinned area takes at most this share of the lanes' height.
const MOST: f32 = 0.6;

/// Where each part of the arrangement is, in points from the lanes' top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Zones {
    /// The scrolling tracks: from 0 to here, where the pinned area starts.
    pub main: f32,
    /// The caption, when there are returns.
    pub caption: Option<(f32, f32)>,
    /// The returns, between the caption and the master.
    pub returns: (f32, f32),
    /// The master's row, at the bottom.
    pub master: (f32, f32),
    /// The returns' full height (more than their zone when the area was
    /// dragged smaller: they scroll inside it).
    pub returns_content: f32,
}

/// The letter of the `n`th return: A to Z, then AA, AB and on.
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

/// The letter of return `id` in `project`, by its place among the returns.
pub(crate) fn letter_of(project: &Project, id: lumen_project::model::Id) -> Option<String> {
    project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Return)
        .position(|t| t.id == id)
        .map(letter)
}

impl Arrangement {
    /// The height the master's row is drawn at.
    pub(super) fn master_height(&self, ctx: &egui::Context, project: &Project) -> f32 {
        motion::glide(
            ctx,
            EguiId::new(("track height", project.master.id)),
            self.target_height(project.master.id),
            true,
        )
    }

    /// Where the zones are this frame.
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

    /// The divider (drag to resize the area, double-click for its natural
    /// size) and the caption (click to fold the returns), across the
    /// headers and the lanes (`full`, the lanes' height). Called after the
    /// lanes' own interaction, so it is on top.
    pub(crate) fn pinned_controls(&mut self, ui: &mut Ui, full: Rect, doc: &Document) {
        let ctx = ui.ctx().clone();
        let palette = *theme::p();
        let project = doc.project();
        let zones = self.zones(&ctx, project);
        let at = |y: f32| full.top() + y;
        let painter = ui.painter_at(full);

        // The divider.
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
            // Dragged to its natural size or past it: it follows its
            // contents again.
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
            Stroke::new(1.0, palette.border_strong),
        );
        painter.hline(
            divider.x_range(),
            divider.bottom() - 0.5,
            Stroke::new(1.0, palette.border_strong),
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

        // The caption: click to fold the returns away.
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
                Stroke::new(1.0, palette.border),
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

    /// The wheel over the returns scrolls them when they do not fit (before
    /// the arrangement's own scrolling takes the wheel). `full` is the
    /// lanes' height across the headers and the lanes.
    pub(crate) fn pinned_wheel(&mut self, ui: &mut Ui, full: Rect, project: &Project) {
        let zones = self.zones(ui.ctx(), project);
        let at = |y: f32| full.top() + y;
        // The wheel over the returns scrolls them when they do not fit.
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

    /// The pinned area's lanes, under the divider: a quieter ground so the
    /// grid recedes, a soloed return's wash, the edges between rows and a
    /// resize glow. Returns and the master hold no clips.
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
        painter.rect_filled(area, 0.0, palette.canvas.gamma_multiply(0.7));
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
                        painter.rect_filled(lane, 0.0, palette.accent.gamma_multiply(0.06));
                    }
                    painter.hline(
                        canvas.x_range(),
                        lane.bottom() - 0.5,
                        Stroke::new(1.0, palette.border.gamma_multiply(0.6)),
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
                        Stroke::new(1.0, palette.border),
                    );
                }
                super::RowKind::Pending(_) => {}
            }
        }
    }

    /// The Solo/Cue switch at the master's right (the owner's C1). On Cue,
    /// the tracks' S buttons cue them to the headphones. Without a cue pair
    /// (Audio settings) it is quiet and says why when clicked.
    fn cue_switch(&mut self, ui: &mut Ui, rect: Rect, mid: f32) {
        let palette = *theme::p();
        let switch = Rect::from_min_size(
            pos2(rect.right() - 10.0 - 46.0, mid - 9.0),
            vec2(46.0, 18.0),
        );
        let ready = self.cue.ready;
        let on = self.cue.mode && ready;
        let response = ui
            .interact(switch, ui.id().with("solo cue switch"), Sense::click())
            .on_hover_text(if ready {
                "Solo or Cue: on Cue, the tracks' S buttons send them to the headphones, before the fader"
            } else {
                "Cue needs a second pair of outputs: choose a Cue output in Audio settings"
            });
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                ready,
                on,
                "Solo/Cue switch: Cue",
            )
        });
        if response.clicked() {
            if ready {
                self.cue.mode = !self.cue.mode;
            } else {
                self.note =
                    Some("Choose a Cue output in Audio settings first: a second pair of outputs");
            }
        }
        let hover = motion::hover(ui.ctx(), ui.id().with("solo cue hover"), response.hovered());
        let look = theme::controls(false);
        let (fill, ink) = if on {
            (palette.accent, theme::text_on(palette.accent))
        } else if ready {
            (look.fill.lerp_to_gamma(look.hover, hover), look.text)
        } else {
            (look.fill.gamma_multiply(0.6), palette.text_disabled)
        };
        let painter = ui.painter();
        painter.rect_filled(switch, theme::radius::SMALL, fill);
        if on {
            crate::icons::draw_headphones(
                painter,
                pos2(switch.left() + 11.0, switch.center().y),
                ink,
            );
        }
        painter.text(
            pos2(
                switch.center().x + if on { 5.0 } else { 0.0 },
                switch.center().y,
            ),
            Align2::CENTER_CENTER,
            if on { "Cue" } else { "Solo" },
            fonts::medium(theme::size::CAPTION),
            ink,
        );
    }

    /// The master's header: its rail and name, the fold arrow, and volume
    /// and pan when unfolded. The Solo/Cue switch joins it with the cue
    /// (slice 4d).
    pub(super) fn master_header(&mut self, ui: &mut Ui, doc: &mut Document, rect: Rect) {
        let palette = *theme::p();
        let master = doc.project().master.clone();
        let id = EguiId::new(("track header", master.id));
        let selected = false;
        let look = theme::controls(selected);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, palette.header);
        let rail = Rect::from_min_size(rect.min, vec2(4.0, rect.height()));
        painter.rect_filled(rail, 0.0, palette.text_hint);
        let mid = rect.top() + FOLDED / 2.0;
        let open = self.expanded.contains(&master.id);
        let arrow = Rect::from_center_size(pos2(rect.left() + 4.0 + 14.0, mid), vec2(16.0, 18.0));
        let fold = ui
            .interact(arrow, id.with("fold button"), Sense::click())
            .on_hover_text(if open {
                "Fold the master"
            } else {
                "Unfold the master (volume and pan)"
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
        draw_arrow(
            &painter,
            arrow.center(),
            unfolded,
            if fold.hovered() {
                palette.text
            } else {
                palette.text_secondary
            },
        );
        crate::icons::draw_type_icon(
            &painter,
            pos2(rect.left() + 4.0 + 34.0, mid),
            TrackKind::Master,
            palette.text_secondary,
        );
        painter.text(
            pos2(rect.left() + 4.0 + 48.0, mid),
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
                pos2(rect.left() + 48.0, mid - 10.0),
                pos2(rect.right() - 10.0, mid + 10.0),
            ),
            id.with("name"),
            Sense::hover(),
        );
        name.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Master track")
        });
        self.cue_switch(ui, rect, mid);
        if rect.height() >= FOLDED + 22.0 {
            self.mixer_row(ui, doc, &master, rect, id, look);
        }
        // The cue level, under volume and pan, while a cue pair is open.
        if self.cue.ready && rect.height() >= FOLDED + 2.0 * super::STEP {
            let y = rect.top() + FOLDED + super::STEP + super::STEP / 2.0;
            let label = Rect::from_min_max(
                pos2(rect.left() + 30.0, y - 10.0),
                pos2(rect.left() + 64.0, y + 10.0),
            );
            painter.text(
                label.left_center(),
                Align2::LEFT_CENTER,
                "Cue",
                FontId::proportional(theme::size::CAPTION),
                palette.text_secondary,
            );
            let slider = Rect::from_min_max(
                pos2(label.right(), y - 10.0),
                pos2(rect.right() - 8.0, y + 10.0),
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
            Stroke::new(1.0, palette.border),
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
