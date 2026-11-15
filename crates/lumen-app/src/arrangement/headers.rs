use std::sync::Arc;

use eframe::egui::{self, Align2, FontId, Id as EguiId, Key, Rect, Sense, Stroke, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_project::automation::Target;
use lumen_project::model::{Id, TrackKind};

use super::control_menu::{control_menu_for, waiting_ring};
use super::follow;
use super::header_row::HeaderRow;
use super::menus::AddMenu;
use super::nesting::Arm;
use super::{Arrangement, FOLDED, RowKind, Zone};
use crate::commands::{Command, Keymap};
use crate::document::Document;
use crate::lanes::Lanes;
use crate::widgets::{self, SliderKind, draw_arrow, value_slider};

pub(super) const CONTENT_X: f32 = 12.0;
pub(super) const CONTENT_RIGHT: f32 = 8.0;
pub(super) const FOLD: f32 = 16.0;
pub(super) const GAP: f32 = 6.0;

const ROW_TOP: f32 = 8.0;
const ROW: f32 = theme::size::CONTROL_SMALL;

pub(super) fn row_center(top: f32, n: usize) -> f32 {
    top + ROW_TOP + n as f32 * (ROW + GAP) + ROW / 2.0
}

pub(super) fn name_center(top: f32, height: f32) -> f32 {
    let open = ((height - FOLDED) / (super::UNFOLDED - FOLDED)).clamp(0.0, 1.0);
    let folded = top + FOLDED / 2.0;
    folded + (row_center(top, 0) - folded) * open
}

pub(super) const SILENCED: f32 = 0.45;
use crate::theme::Tone;
use crate::widgets::raised;
use crate::{fonts, motion, theme};

impl Arrangement {
    pub fn headers(
        &mut self,
        ui: &mut Ui,
        column: Rect,
        doc: &mut Document,
        lanes: &Lanes,
        keymap: &Keymap,
    ) {
        self.prune(doc.project());
        let ctx = ui.ctx().clone();
        let palette = *theme::p();
        let painter = ui.painter_at(column);
        painter.rect_filled(column, 0.0, palette.header);
        self.view_height = column.height();
        self.hot_return = self.hot_return_next.take();
        self.held_send = self.held_next.take();
        let mut rows = self.rows(&ctx, doc.project(), lanes.pending().len());
        let content = self.content_height(&ctx, doc.project(), &rows);
        if self.keep_in_content(content, column.height()) {
            rows = self.rows(&ctx, doc.project(), lanes.pending().len());
        }
        let main = self.zone_span(&ctx, doc.project(), Zone::Main, column.y_range());

        let below = Rect::from_min_max(
            pos2(
                column.left(),
                (column.top() + Self::main_bottom(&rows) - self.top).max(column.top()),
            ),
            pos2(column.right(), main.max),
        );
        if below.height() > 0.0 {
            self.empty_area(ui, &painter, doc, keymap, (below, main.max));
        }

        let modifiers = ui.input(|i| i.modifiers);
        let hearing = lumen_project::solo::hearing(doc.project());
        let mut hot_edge = None;
        let laid_out: Vec<Option<Id>> = rows
            .iter()
            .map(|r| match r.kind {
                RowKind::Track(i) => doc.project().tracks.get(i).map(|t| t.id),
                RowKind::Pending(_) | RowKind::Master => None,
            })
            .collect();
        let still = |doc: &Document, index: usize, row: usize| {
            doc.project()
                .tracks
                .get(index)
                .filter(|t| Some(t.id) == laid_out[row])
                .cloned()
        };
        let base_clip = ui.clip_rect();
        let base_painter = painter.clone();
        let mut edges = Vec::new();
        let mut covered = None;
        for (n, &row) in rows.iter().enumerate() {
            let zone = Rect::from_x_y_ranges(
                column.x_range(),
                self.zone_span(&ctx, doc.project(), row.zone, column.y_range()),
            );
            let rect = Rect::from_min_size(
                pos2(column.left(), column.top() + row.y - self.top),
                vec2(column.width(), row.height),
            );
            if rect.bottom() < zone.top() || rect.top() > zone.bottom() {
                continue;
            }
            let zone = if row.clipped {
                zone.intersect(rect)
            } else {
                zone
            };
            ui.set_clip_rect(zone.intersect(base_clip));
            let painter = base_painter.with_clip_rect(zone.intersect(base_painter.clip_rect()));
            if row.kind == RowKind::Master {
                self.master_header(ui, doc, rect);
                continue;
            }
            let RowKind::Track(index) = row.kind else {
                let name = lanes
                    .pending()
                    .get(match row.kind {
                        RowKind::Pending(i) => i,
                        RowKind::Track(_) | RowKind::Master => unreachable!(),
                    })
                    .and_then(|(id, _)| lanes.library.get(*id))
                    .map_or_else(String::new, |i| i.name.clone());
                painter.text(
                    pos2(rect.left() + 12.0, rect.top() + FOLDED / 2.0),
                    Align2::LEFT_CENTER,
                    format!("{name} \u{00B7} loading"),
                    FontId::proportional(theme::size::BODY),
                    palette.text_hint,
                );
                painter.hline(
                    column.x_range(),
                    rect.bottom() - 0.5,
                    Stroke::new(theme::stroke::HAIRLINE, palette.border),
                );
                continue;
            };
            let Some(track) = still(doc, index, n) else {
                continue;
            };
            self.track_header(
                ui,
                &painter,
                doc,
                keymap,
                HeaderRow {
                    track: Arc::clone(&track),
                    index,
                    rect,
                    column,
                    modifiers,
                    hearing: &hearing,
                },
            );
            if row.zone == Zone::Main
                && !zone.contains_rect(rect)
                && let Some(focused) = ui.ctx().memory(|m| m.focused())
                && ui
                    .ctx()
                    .read_response(focused)
                    .is_some_and(|r| rect.contains_rect(r.rect) && !zone.contains_rect(r.rect))
            {
                covered = Some(track.id);
            }
            edges.push((track, rect, zone));
        }
        if let Some(id) = covered {
            let pending = lanes.pending().len();
            self.scroll_to_track(&ctx, doc.project(), pending, id, column.height());
            ui.ctx().request_repaint();
        }
        for (track, rect, zone) in edges {
            ui.set_clip_rect(zone.intersect(base_clip));
            let painter = base_painter.with_clip_rect(zone.intersect(base_painter.clip_rect()));
            if let Some(glow) = self.resize_edge(ui, &painter, &track, (rect, column)) {
                hot_edge = Some(glow);
            }
        }
        ui.set_clip_rect(base_clip);
        self.hot_edge = hot_edge;
        painter.vline(
            column.right() - 0.5,
            column.y_range(),
            Stroke::new(theme::stroke::HAIRLINE, palette.border),
        );
        for (n, row) in rows.iter().enumerate() {
            if let RowKind::Track(index) = row.kind
                && still(doc, index, n).is_some_and(|t| self.is_dragged(t.id))
            {
                let rect = Rect::from_min_size(
                    pos2(column.left(), column.top() + row.y - self.top),
                    vec2(column.width(), row.height),
                );
                painter.rect_filled(
                    rect,
                    0.0,
                    palette.header.gamma_multiply(theme::alpha::EDGE_DIM),
                );
            }
        }
        let unchanged = rows.iter().enumerate().all(|(n, row)| match row.kind {
            RowKind::Track(index) => still(doc, index, n).is_some(),
            RowKind::Pending(_) | RowKind::Master => true,
        });
        if unchanged {
            self.follow_drag(ui, column, &rows, content, doc);
        }
        self.draw_drop_line(&painter, column, true);
    }

    fn empty_area(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        doc: &mut Document,
        keymap: &Keymap,
        (below, bottom): (Rect, f32),
    ) {
        let ctx = ui.ctx().clone();
        let palette = *theme::p();
        let empty = ui.interact(below, ui.id().with("headers below"), Sense::click());
        empty.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Empty track area: double-click to add an audio track",
            )
        });
        if empty.hovered() {
            use crate::app::info::{Held, Info, Mouse};
            self.info = Some(
                Info::new("Empty track area")
                    .on(Held::NONE, Mouse::DoubleClick, "add an audio track")
                    .on(Held::NONE, Mouse::RightClick, "add another kind")
                    .key(Command::AddAudioTrack, "audio track")
                    .key(Command::AddMidiTrack, "MIDI track"),
            );
        }
        if empty.double_clicked() {
            self.selected = vec![doc.add_track(TrackKind::Audio, None)];
        } else if empty.clicked() {
            self.selected.clear();
        }
        widgets::Menu::context(&empty)
            .show(|ui| self.add_menu(ui, doc, None, keymap, AddMenu::Context));

        let button =
            Rect::from_min_size(below.min + vec2(8.0, 8.0), vec2(below.width() - 16.0, 26.0));
        if button.bottom() < bottom {
            let response = ui
                .interact(button, ui.id().with("add track"), Sense::click())
                .on_hover_text("Add an audio, MIDI, group or return track");
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add track")
            });
            let hover = motion::hover(&ctx, ui.id().with("add track hover"), response.hovered());
            let open = egui::Popup::is_id_open(&ctx, egui::Popup::default_response_id(&response));
            let face = raised::Face::new(theme::controls(false))
                .hover(hover)
                .pressed(response.is_pointer_button_down_on())
                .lit(if open { 1.0 } else { 0.0 }, Tone::On);
            let button_rect = button.round_to_pixels(ctx.pixels_per_point());
            raised::paint(
                painter,
                button_rect,
                theme::radius::for_height(button_rect.height()),
                &face,
            );
            raised::focus_ring(painter, &response, button_rect);
            painter.text(
                pos2(button.left() + 10.0, button.center().y),
                Align2::LEFT_CENTER,
                "+ Add track",
                FontId::proportional(theme::size::BODY),
                palette
                    .text
                    .lerp_to_gamma(theme::lit(Tone::On).text, face.lit),
            );
            draw_arrow(
                painter,
                pos2(button.right() - 12.0, button.center().y),
                1.0,
                palette.text_secondary,
            );
            widgets::Menu::below(&response).show(|ui| {
                ui.set_min_width(button.width());
                self.add_menu(
                    ui,
                    doc,
                    self.selected.last().copied(),
                    keymap,
                    AddMenu::Button,
                );
            });
        }
    }

    pub(super) fn name(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &lumen_project::model::Track,
        rect: Rect,
        (heard, count): (bool, Option<usize>),
        (id, in_strip): (EguiId, bool),
    ) -> Option<egui::Response> {
        let palette = theme::p();
        let font = if track.kind == TrackKind::Group {
            fonts::medium(theme::size::BODY)
        } else {
            FontId::proportional(theme::size::BODY)
        };
        let rect = if !in_strip && self.live.listens(track, heard) {
            Rect::from_min_max(
                rect.min,
                pos2(
                    (rect.right() - super::listen_light::ROOM).max(rect.left()),
                    rect.bottom(),
                ),
            )
        } else {
            rect
        };
        let (rect, count) = match count {
            Some(n) => {
                let count = painter_count(ui, n);
                let width = count.size().x + 6.0;
                (
                    Rect::from_min_max(
                        rect.min,
                        pos2((rect.right() - width).max(rect.left()), rect.bottom()),
                    ),
                    Some((count, width)),
                )
            }
            None => (rect, None),
        };
        let frozen = track.frozen.is_some() && !in_strip;
        let (rect, badge_row) = if frozen {
            let room = super::frozen_badge::room(ui, rect.width());
            (
                Rect::from_min_max(
                    rect.min,
                    pos2((rect.right() - room).max(rect.left()), rect.bottom()),
                ),
                Some(rect),
            )
        } else {
            (rect, None)
        };
        if let Some((renaming, text)) = &mut self.renaming
            && *renaming == track.id
            && self.renaming_in_strip == in_strip
        {
            let edit_id = id.with("rename");
            let response = ui.put(
                rect,
                egui::TextEdit::singleline(text)
                    .id(edit_id)
                    .font(FontId::proportional(theme::size::BODY))
                    .margin(vec2(4.0, 2.0)),
            );
            if !response.has_focus() && !response.lost_focus() {
                response.request_focus();
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                self.renaming = None;
            } else if response.lost_focus() {
                let name = text.clone();
                self.renaming = None;
                doc.rename_track(track.id, &name);
            }
            return None;
        }
        let response = ui.interact(rect, id.with("name"), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!(
                    "{}{}: double-click to rename",
                    track.name,
                    if in_strip { " in the mixer" } else { "" }
                ),
            )
        });
        if response.double_clicked() {
            self.renaming = Some((track.id, track.name.clone()));
            self.renaming_in_strip = in_strip;
        } else if response.clicked() {
            let modifiers = ui.input(|i| i.modifiers);
            self.click_select(doc.project(), track.id, modifiers);
        }
        let painter = ui.painter().clone();
        let name_ink = if heard {
            palette.text
        } else {
            palette.text.gamma_multiply(SILENCED)
        };
        let galley = painter.layout(track.name.clone(), font.clone(), name_ink, rect.width());
        let text = if galley.rows.len() > 1 {
            let mut name = track.name.clone();
            while !name.is_empty() {
                name.pop();
                let g =
                    painter.layout_no_wrap(format!("{name}\u{2026}"), font.clone(), palette.text);
                if g.size().x <= rect.width() {
                    break;
                }
            }
            format!("{name}\u{2026}")
        } else {
            track.name.clone()
        };
        let drawn = painter.text(
            pos2(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            text,
            font,
            name_ink,
        );
        if !in_strip {
            self.live.paint(ui, track, heard, drawn);
        }
        if let Some((count, _)) = count {
            let at = pos2(drawn.right() + 6.0, rect.center().y - count.size().y / 2.0);
            painter.galley(at, count, palette.text_hint);
        }
        if let Some(row) = badge_row
            && super::frozen_badge::badge(ui, row, id, track, self.selected.contains(&track.id))
        {
            self.selected = vec![track.id];
            self.track_command = Some(crate::commands::Command::UnfreezeTracks);
        }
        Some(response)
    }

    pub(super) fn mixer_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &lumen_project::model::Track,
        (rect, left): (Rect, f32),
        (id, look): (EguiId, theme::Controls),
    ) {
        let second = row_center(rect.top(), 1);
        let (start, side, _) = columns(rect, left);
        let right = rect.right() - CONTENT_RIGHT;
        let volume_rect = Rect::from_min_max(
            pos2(start, second - ROW / 2.0),
            pos2(side - GAP, second + ROW / 2.0),
        );
        let pan_rect = Rect::from_min_max(
            pos2(side, second - ROW / 2.0),
            pos2(right, second + ROW / 2.0),
        );
        if track.kind == lumen_project::model::TrackKind::Audio {
            let third = row_center(rect.top(), 2);
            let chip = Rect::from_min_max(
                pos2(side, third - ROW / 2.0),
                pos2(right, third + ROW / 2.0),
            );
            self.input_chip(ui, doc, track, chip, id);
        }
        let gain = self.shown(doc, track, Target::Volume, track.gain_db);
        let db = widgets::value_slider_styled(
            ui,
            volume_rect,
            id.with("volume"),
            gain,
            SliderKind::Volume,
            &format!("Volume {}", track.name),
            widgets::SliderStyle { look, comet: false },
        );
        if db != gain {
            doc.set_volume(track.id, db);
        }
        if self.taken_over(doc, track, Target::Volume) {
            follow::ring(ui, volume_rect);
        }
        control_menu_for(
            ui,
            id.with("volume"),
            doc,
            &mut self.auto,
            track.id,
            Target::Volume,
        );
        waiting_ring(ui, &self.auto, volume_rect, track.id, Target::Volume);
        let shown_pan = self.shown(doc, track, Target::Pan, track.pan);
        let pan = value_slider(
            ui,
            pan_rect,
            id.with("pan"),
            shown_pan,
            SliderKind::Pan,
            &format!("Pan {}", track.name),
            look,
        );
        if pan != shown_pan {
            doc.set_pan(track.id, pan);
        }
        if self.taken_over(doc, track, Target::Pan) {
            follow::ring(ui, pan_rect);
        }
        control_menu_for(
            ui,
            id.with("pan"),
            doc,
            &mut self.auto,
            track.id,
            Target::Pan,
        );
        waiting_ring(ui, &self.auto, pan_rect, track.id, Target::Pan);
    }
}

pub(super) fn columns(rect: Rect, left: f32) -> (f32, f32, Rect) {
    let start = left + CONTENT_X;
    let side = rect.right() - CONTENT_RIGHT - theme::size::HEADER_SIDE;
    let third = row_center(rect.top(), 2);
    let sends = Rect::from_min_max(
        pos2(start, third - ROW / 2.0),
        pos2((side - GAP).max(start), third + ROW / 2.0),
    );
    (start, side, sends)
}

fn painter_count(ui: &Ui, n: usize) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout_no_wrap(
        format!("\u{00B7} {n}"),
        FontId::proportional(theme::size::BODY),
        theme::p().text_hint,
    )
}

pub(super) fn arm_button(ui: &mut Ui, rect: Rect, id: EguiId, name: &str, arm: Arm) -> bool {
    let armed = arm.on();
    let response = ui
        .interact(rect, id.with("arm"), Sense::click())
        .on_hover_text(if arm == Arm::Listening {
            "Listening because selected \u{b7} click to arm for recording (Ctrl keeps the other tracks armed)"
        } else {
            "Arm for recording (Ctrl keeps the other tracks armed; MIDI tracks record now, audio recording arrives in Milestone 9)"
        });
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            armed,
            format!("Arm {name}"),
        )
    });
    let hover = motion::hover(ui.ctx(), id.with("arm hover"), response.hovered());
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    arm_piece(ui, rect, arm, hover);
    if response.has_focus() {
        raised::paint_focus(ui.painter(), rect, crate::widgets::well::PIECE_RADIUS);
    }
    response.clicked()
}

fn arm_piece(ui: &Ui, rect: Rect, arm: Arm, hover: f32) {
    let palette = theme::p();
    let p = ui.painter();
    let center = rect.center();
    let r = f32::from(crate::widgets::well::PIECE_RADIUS);
    let ring = rect.height() / 2.0 - PIECE_RING_INSET;
    match arm {
        Arm::All => {
            let breath = breath(ui);
            let swell = PIECE_BODY_LOW + (1.0 - PIECE_BODY_LOW) * 2.0 * breath;
            p.rect_filled(rect, r, palette.record.gamma_multiply(ARM_BODY * swell));
            p.circle_filled(
                center,
                PIECE_DOT + PIECE_HALO * breath,
                palette.record.gamma_multiply(PIECE_HALO_ALPHA * breath),
            );
            p.circle_stroke(
                center,
                ring - 0.5 + 0.5 * breath,
                Stroke::new(
                    theme::stroke::LIT,
                    palette
                        .record
                        .gamma_multiply(theme::alpha::EDGE_SOFT + ARM_RING_GAIN * breath),
                ),
            );
            p.circle_filled(center, PIECE_DOT, palette.record);
        }
        Arm::Partly => {
            p.rect_filled(rect, r, palette.record.gamma_multiply(ARM_BODY));
            p.add(egui::Shape::line(
                half_ring(center, ring),
                Stroke::new(
                    theme::stroke::LIT,
                    palette.record.gamma_multiply(theme::alpha::EDGE_DIM),
                ),
            ));
            p.circle_filled(center, PIECE_DOT, palette.record);
        }
        Arm::Listening => {
            p.rect_filled(rect, r, palette.ember.gamma_multiply(hover));
            p.circle_stroke(
                center,
                PIECE_DOT,
                Stroke::new(theme::stroke::LIT, palette.record),
            );
        }
        Arm::Off => {
            p.rect_filled(rect, r, palette.ember.gamma_multiply(hover));
            let rest = palette.text_secondary.gamma_multiply(ARM_REST_DOT);
            p.circle_filled(center, PIECE_DOT, rest.lerp_to_gamma(palette.record, hover));
        }
    }
}

fn breath(ui: &Ui) -> f32 {
    if motion::reduced() {
        0.5
    } else {
        let t = ui.input(|i| i.time) as f32;
        ui.ctx().request_repaint_after(motion::BREATH_FRAME);
        0.5 - 0.5 * (t * std::f32::consts::TAU / ARM_BREATH).cos()
    }
}

const PIECE_DOT: f32 = 4.0;
const PIECE_RING_INSET: f32 = 2.0;
const PIECE_BODY_LOW: f32 = 0.7;
const PIECE_HALO: f32 = 1.5;
const PIECE_HALO_ALPHA: f32 = 0.35;

fn half_ring(center: egui::Pos2, radius: f32) -> Vec<egui::Pos2> {
    use std::f32::consts::PI;
    (0..=16)
        .map(|i| {
            let a = -PI / 2.0 + PI * i as f32 / 16.0;
            pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

const ARM_BREATH: f32 = motion::BREATH;

const ARM_BODY: f32 = 0.25;
const ARM_REST_DOT: f32 = 0.55;
const ARM_RING_GAIN: f32 = 0.30;

pub(super) fn group_description(
    track: &lumen_project::model::Track,
    inside: usize,
    collapsed: bool,
) -> String {
    let mut text = track_description(track);
    if track.kind == TrackKind::Group {
        text.push_str(&format!(
            ", {inside} track{} inside",
            if inside == 1 { "" } else { "s" }
        ));
        if collapsed {
            text.push_str(", folded");
        }
    }
    text
}

pub(super) fn track_description(track: &lumen_project::model::Track) -> String {
    let mut parts = vec![
        format!("Track {}", track.name),
        kind_name(track.kind).to_owned(),
    ];
    if track.mute {
        parts.push("muted".into());
    }
    if track.solo {
        parts.push("soloed".into());
    }
    if track.armed {
        parts.push("armed for recording".into());
    }
    parts.join(", ")
}

pub(super) fn kind_name(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Audio => "Audio track",
        TrackKind::Midi => "MIDI track",
        TrackKind::Group => "Group track",
        TrackKind::Return => "Return track",
        TrackKind::Master => "Master track",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unfolded_track_has_even_margins_around_its_three_rows() {
        assert_eq!(
            ROW_TOP + 3.0 * ROW + 2.0 * GAP + 8.0,
            super::super::UNFOLDED
        );
        assert_eq!(row_center(0.0, 0), 18.0);
        assert_eq!(row_center(0.0, 1), 44.0);
        assert_eq!(row_center(0.0, 2), 70.0);
        assert_eq!(
            super::super::UNFOLDED - (row_center(0.0, 2) + ROW / 2.0),
            8.0
        );
    }

    #[test]
    fn the_name_row_moves_up_as_the_track_opens() {
        assert_eq!(name_center(100.0, FOLDED), 122.0);
        assert_eq!(name_center(100.0, super::super::UNFOLDED), 118.0);
        assert_eq!(name_center(100.0, 300.0), 118.0);
        let half = name_center(100.0, (FOLDED + super::super::UNFOLDED) / 2.0);
        assert!(half > 118.0 && half < 122.0);
    }

    #[test]
    fn the_columns_line_up_at_every_width() {
        for width in [204.0, 280.0, 480.0] {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 88.0));
            let (start, side, sends) = columns(rect, 0.0);
            assert_eq!(start, 12.0);
            assert_eq!(side, width - 8.0 - 60.0);
            assert_eq!(sends.left(), start);
            assert_eq!(sends.right(), side - GAP);
            assert_eq!(sends.center().y, 70.0);
        }
    }
}
