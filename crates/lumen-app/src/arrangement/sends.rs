//! Sends on track headers (M4 slice 4, the owner's K1 comet): on an
//! unfolded audio, MIDI or group track, a knob per return, lettered and
//! coloured as the return is. Hovering a knob lights its return's header,
//! and hovering a return lights every knob that feeds it.
//!
//! MT-MIX-4 run 1 (the owner's choices from mock-ups): knobs step up on a
//! selected row (C4); sends past the header's width are reached from a
//! "+N ▾" button that lists every send of the track (O1); an unfolded
//! return lists the tracks sending to it, each with its knob (R2).
//!
//! MT-MIX-4 run 2: a return lists its four loudest senders, then "+N more"
//! opens them all (L4); a send turned down to silence leaves the list once
//! it is let go (S1). The send itself stays on the track, pre- or
//! post-fader as it was.

use eframe::egui::{
    self, Align2, Color32, FontId, Id as EguiId, Rect, Sense, Stroke, Ui, pos2, vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{Id, Level, Project, Track, TrackKind};
use lumen_project::solo::Heard;

use super::{Arrangement, FOLDED, STEP, color_of, pinned};
use crate::document::Document;
use crate::widgets::knob::{SendKnob, send_knob};
use crate::widgets::{db_text, draw_arrow};
use crate::{fonts, motion, theme};

/// A send knob's size, and the gap between knobs.
const KNOB: f32 = 24.0;
const GAP: f32 = 5.0;
/// A knob in a list (the "+N" list, a return's senders): small enough
/// that knobs on 22 pt rows never touch.
const SMALL_KNOB: f32 = 18.0;
/// A return lists this many senders, then "+N more" (the owner's L4).
pub(super) const RETURN_ROWS: usize = 4;

/// Whether a track of `kind` shows sends: returns and the master do not.
pub(super) fn has_sends(kind: TrackKind) -> bool {
    matches!(kind, TrackKind::Audio | TrackKind::Midi | TrackKind::Group)
}

/// The height a header needs to show its sends: the folded row, volume
/// and pan, then two steps for the knobs (such tracks unfold this tall).
pub(super) const SENDS_FROM: f32 = FOLDED + 3.0 * STEP;

/// A return, as its sends show it.
#[derive(Debug, Clone)]
struct Return {
    id: Id,
    letter: String,
    name: String,
    color: Color32,
}

fn returns(project: &Project) -> Vec<Return> {
    project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Return)
        .enumerate()
        .map(|(n, t)| Return {
            id: t.id,
            letter: pinned::letter(n),
            name: t.name.clone(),
            color: color_of(t.color),
        })
        .collect()
}

/// The tracks that send to return `to`, in list order: sending (not
/// silent), or `held` (a send being set right now keeps its row until it
/// is let go, the owner's S1).
pub(super) fn senders(project: &Project, to: Id, held: Option<(Id, Id)>) -> Vec<&Track> {
    project
        .tracks
        .iter()
        .filter(|t| {
            t.sends
                .iter()
                .any(|s| s.to == to && (!s.level.is_silent() || held == Some((t.id, to))))
        })
        .map(|t| &**t)
        .collect()
}

/// The rows an unfolded return needs for its senders: up to
/// [`RETURN_ROWS`], and one for "+N more" past them.
pub(super) fn sender_rows(senders: usize) -> usize {
    if senders > RETURN_ROWS {
        RETURN_ROWS + 1
    } else {
        senders.max(1)
    }
}

/// `senders`' level into return `to`.
fn level_into(track: &Track, to: Id) -> f32 {
    track
        .sends
        .iter()
        .find(|s| s.to == to)
        .map_or(f32::NEG_INFINITY, |s| s.level.0)
}

/// One send's knob: where it is, what it shows, what it is called.
struct Knob<'a> {
    rect: Rect,
    track: &'a Track,
    to: &'a Return,
    selected: bool,
    name: String,
    /// Where it is drawn ("header", "list", "return", "return list"): each
    /// place's knobs are their own widgets.
    place: &'static str,
}

/// The "+N ▾" button, drawn as "+ Add track ▾" is.
struct Plus {
    id: EguiId,
    text: String,
    /// Its accessible name.
    name: String,
    tip: &'static str,
    selected: bool,
    /// A dot before the number in this colour (something hidden is up).
    dot: Option<Color32>,
    /// Its left edge, vertically centred.
    at: egui::Pos2,
}

fn plus_button(ui: &mut Ui, plus: Plus) -> egui::Response {
    let palette = *theme::p();
    let look = theme::controls(plus.selected);
    let font = FontId::proportional(theme::size::CAPTION);
    let galley = ui.painter().layout_no_wrap(plus.text, font, look.text);
    let dot = if plus.dot.is_some() { 8.0 } else { 0.0 };
    let width = 8.0 + dot + galley.size().x + 6.0 + 8.0 + 7.0;
    let button = Rect::from_min_size(pos2(plus.at.x, plus.at.y - 10.0), vec2(width, 20.0))
        .round_to_pixels(ui.ctx().pixels_per_point());
    let response = ui
        .interact(button, plus.id, Sense::click())
        .on_hover_text(plus.tip);
    let name = plus.name;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    let ctx = ui.ctx().clone();
    let hover = motion::hover(&ctx, response.id.with("hover"), response.hovered());
    let open = egui::Popup::is_id_open(&ctx, egui::Popup::default_response_id(&response));
    let painter = ui.painter();
    painter.rect_filled(
        button,
        theme::radius::MEDIUM,
        look.fill.lerp_to_gamma(look.hover, hover),
    );
    if open || hover > 0.0 {
        painter.rect_stroke(
            button,
            theme::radius::MEDIUM,
            Stroke::new(
                1.0,
                palette
                    .accent
                    .gamma_multiply(if open { 1.0 } else { hover }),
            ),
            egui::StrokeKind::Inside,
        );
    }
    let mut x = button.left() + 8.0;
    if let Some(color) = plus.dot {
        painter.circle_filled(pos2(x + 3.0, button.center().y), 3.0, color);
        x += dot;
    }
    let text_width = galley.size().x;
    painter.galley(
        pos2(x, button.center().y - galley.size().y / 2.0),
        galley,
        look.text,
    );
    draw_arrow(
        painter,
        pos2(x + text_width + 6.0 + 4.0, button.center().y),
        1.0,
        palette.text_secondary,
    );
    response
}

impl Arrangement {
    /// Draws a send knob and applies what the user does with it. Returns
    /// whether it is being set right now.
    fn send_control(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        heard: &Heard,
        knob: Knob<'_>,
    ) -> bool {
        let Knob {
            rect,
            track,
            to,
            selected,
            name,
            place,
        } = knob;
        let send = track.sends.iter().find(|s| s.to == to.id);
        let id = EguiId::new(("send", track.id, to.id, place));
        let out = send_knob(
            ui,
            rect,
            id,
            SendKnob {
                letter: &to.letter,
                color: to.color,
                db: send.map_or(f32::NEG_INFINITY, |s| s.level.0),
                pre_fader: send.is_some_and(|s| s.pre_fader),
                heard: heard.of(track.id).sends,
                // In a return's own list every knob feeds it: no ring.
                linked: self.hot_return == Some(to.id) && !place.starts_with("return"),
                selected,
                name: &name,
            },
        );
        if out.hovered {
            self.hot_return_next = Some(to.id);
        }
        if let Some(db) = out.db {
            let level = if db.is_finite() {
                Level(db)
            } else {
                Level::SILENT
            };
            if let Err(refused) = doc.set_send(track.id, to.id, level) {
                self.note = Some(refused.reason());
            }
        }
        if let Some(pre) = out.pre_fader
            && let Err(refused) = doc.set_send_pre_fader(track.id, to.id, pre)
        {
            self.note = Some(refused.reason());
        }
        out.active
    }

    /// The row of send knobs under volume and pan; past the header's width,
    /// a "+N ▾" button that lists every send.
    pub(super) fn sends_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        rect: Rect,
        selected: bool,
    ) {
        let returns = returns(doc.project());
        if returns.is_empty() {
            return;
        }
        let heard = lumen_project::solo::hearing(doc.project());
        // Under volume and pan, with room to breathe above the row's edge.
        let center_y = rect.top() + FOLDED + 2.0 * STEP - 2.0;
        let left = rect.left() + 30.0;
        let right = rect.right() - 8.0;
        let fits = (((right - left) + GAP) / (KNOB + GAP)).floor().max(0.0) as usize;
        // Room for the "+N" button after the knobs when they do not all
        // fit: it takes about a knob and a half.
        let shown = if returns.len() > fits {
            fits.saturating_sub(2)
        } else {
            returns.len()
        };
        let ppp = ui.ctx().pixels_per_point();
        for (n, to) in returns.iter().take(shown).enumerate() {
            let x = left + n as f32 * (KNOB + GAP);
            let rect = Rect::from_min_size(pos2(x, center_y - KNOB / 2.0), vec2(KNOB, KNOB))
                .round_to_pixels(ppp);
            let name = format!("Send {} to {} {}", track.name, to.letter, to.name);
            self.send_control(
                ui,
                doc,
                &heard,
                Knob {
                    rect,
                    track,
                    to,
                    selected,
                    name,
                    place: "header",
                },
            );
        }
        if shown < returns.len() {
            let x = left + shown as f32 * (KNOB + GAP);
            self.more_sends(
                ui,
                doc,
                track,
                &returns,
                (shown, pos2(x, center_y)),
                selected,
            );
        }
    }

    /// The "+N ▾" button (the owner's O1, drawn as "+ Add track ▾" is) and
    /// the list it opens: every send of the track, knob, letter, name and
    /// level. A dot in a return's colour before the number when a hidden
    /// send is turned up.
    fn more_sends(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        returns: &[Return],
        (shown, at): (usize, egui::Pos2),
        selected: bool,
    ) {
        let palette = *theme::p();
        let hidden = returns.len() - shown;
        let lit = returns[shown..].iter().find(|r| {
            track
                .sends
                .iter()
                .any(|s| s.to == r.id && !s.level.is_silent())
        });
        let response = plus_button(
            ui,
            Plus {
                id: EguiId::new(("more sends", track.id)),
                text: format!("+{hidden}"),
                name: format!("{hidden} more sends from {}", track.name),
                tip: "Every send of this track",
                selected,
                dot: lit.map(|r| r.color),
                at,
            },
        );
        let heard = lumen_project::solo::hearing(doc.project());
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(236.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("Sends from {}", track.name))
                            .font(fonts::medium(theme::size::BODY)),
                    );
                });
                ui.separator();
                for to in returns {
                    let (row, _) =
                        ui.allocate_exact_size(vec2(236.0, SMALL_KNOB + 4.0), Sense::hover());
                    let knob = Rect::from_min_size(
                        pos2(row.left() + 2.0, row.center().y - SMALL_KNOB / 2.0),
                        vec2(SMALL_KNOB, SMALL_KNOB),
                    );
                    let name = format!("Send {} to {} {}", track.name, to.letter, to.name);
                    self.send_control(
                        ui,
                        doc,
                        &heard,
                        Knob {
                            rect: knob,
                            track,
                            to,
                            selected: false,
                            name,
                            place: "list",
                        },
                    );
                    let badge_left = knob.right() + 8.0;
                    let badge = letter_badge(ui, pos2(badge_left, row.center().y), to);
                    let painter = ui.painter();
                    painter.text(
                        pos2(badge.right() + 6.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &to.name,
                        FontId::proportional(theme::size::BODY),
                        palette.text,
                    );
                    let db = track
                        .sends
                        .iter()
                        .find(|s| s.to == to.id)
                        .map_or(f32::NEG_INFINITY, |s| s.level.0);
                    painter.text(
                        pos2(row.right() - 4.0, row.center().y),
                        Align2::RIGHT_CENTER,
                        db_text(db.max(-60.0)),
                        FontId::proportional(theme::size::CAPTION),
                        palette.text_secondary,
                    );
                }
            });
    }

    /// An unfolded return lists the tracks sending to it, each with its send
    /// knob and level (the owner's R2), from the third step down: the four
    /// loudest, then "+N more" for them all (L4). A send turned down to
    /// silence leaves the list once let go (S1).
    pub(super) fn senders_rows(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        ret: &Track,
        rect: Rect,
    ) {
        let palette = *theme::p();
        let Some(to) = returns(doc.project()).into_iter().find(|r| r.id == ret.id) else {
            return;
        };
        let project = doc.project().clone();
        let held = self.held_send.filter(|(_, r)| *r == ret.id);
        let senders = senders(&project, ret.id, held);
        let top = rect.top() + FOLDED + STEP;
        let left = rect.left() + 30.0;
        let right = rect.right() - 8.0;
        if senders.is_empty() {
            ui.painter().text(
                pos2(left, top + STEP / 2.0),
                Align2::LEFT_CENTER,
                "No tracks send here yet",
                FontId::proportional(theme::size::CAPTION),
                palette.text_hint,
            );
            return;
        }
        let order = self.sender_order(ui, ret.id, &senders);
        let heard = lumen_project::solo::hearing(&project);
        let mut active = false;
        for (n, sender) in order.iter().take(RETURN_ROWS).enumerate() {
            let y = top + n as f32 * STEP + STEP / 2.0;
            if y + STEP / 2.0 > rect.bottom() + 0.5 {
                break;
            }
            let row = Rect::from_min_max(pos2(left, y - STEP / 2.0), pos2(right, y + STEP / 2.0));
            active |= self.sender_row(ui, doc, &heard, (sender, &to), row, "return");
        }
        if order.len() > RETURN_ROWS {
            let y = top + RETURN_ROWS as f32 * STEP + STEP / 2.0;
            if y + STEP / 2.0 <= rect.bottom() + 0.5 {
                active |= self.all_senders(ui, doc, &heard, (&order, &to), pos2(left, y));
            }
        }
        // The send being set keeps its row until it is let go.
        if active {
            self.held_next = self.held_next.or(self.held_send);
        }
    }

    /// The senders of return `id` in the order its list shows them: loudest
    /// first, re-sorted only while no button is held, so a row never jumps
    /// from under the pointer.
    fn sender_order<'p>(&mut self, ui: &Ui, id: Id, senders: &[&'p Track]) -> Vec<&'p Track> {
        let held = ui.input(|i| i.pointer.any_down());
        let known = self.sender_order.entry(id).or_default();
        if !held {
            let mut sorted: Vec<&Track> = senders.to_vec();
            sorted.sort_by(|a, b| level_into(b, id).total_cmp(&level_into(a, id)));
            *known = sorted.iter().map(|t| t.id).collect();
        }
        let mut order: Vec<&Track> = known
            .iter()
            .filter_map(|k| senders.iter().find(|t| t.id == *k).copied())
            .collect();
        for sender in senders {
            if !order.iter().any(|t| t.id == sender.id) {
                order.push(sender);
            }
        }
        order
    }

    /// One sender's row: a bar in its colour, its name, its knob and level.
    /// Returns whether its knob is being set.
    fn sender_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        heard: &Heard,
        (sender, to): (&Track, &Return),
        row: Rect,
        place: &'static str,
    ) -> bool {
        let palette = *theme::p();
        let y = row.center().y;
        let painter = ui.painter();
        let bar = Rect::from_center_size(pos2(row.left() + 2.0, y), vec2(4.0, 14.0));
        painter.rect_filled(bar, 2.0, color_of(sender.color));
        let knob = Rect::from_min_size(
            pos2(row.right() - 46.0 - SMALL_KNOB, y - SMALL_KNOB / 2.0),
            vec2(SMALL_KNOB, SMALL_KNOB),
        )
        .round_to_pixels(ui.ctx().pixels_per_point());
        let name_room = Rect::from_min_max(
            pos2(row.left() + 10.0, y - 8.0),
            pos2(knob.left() - 6.0, y + 8.0),
        );
        let font = FontId::proportional(theme::size::CAPTION);
        let galley = painter.layout(
            sender.name.clone(),
            font.clone(),
            palette.text_secondary,
            f32::INFINITY,
        );
        let name = if galley.size().x > name_room.width() {
            shorten(painter, &sender.name, font.clone(), name_room.width())
        } else {
            sender.name.clone()
        };
        painter.text(
            name_room.left_center(),
            Align2::LEFT_CENTER,
            name,
            font.clone(),
            palette.text_secondary,
        );
        painter.text(
            pos2(row.right(), y),
            Align2::RIGHT_CENTER,
            db_text(level_into(sender, to.id).max(-60.0)),
            font,
            palette.text_secondary,
        );
        let label = format!("{} into {} {}", sender.name, to.letter, to.name);
        let active = self.send_control(
            ui,
            doc,
            heard,
            Knob {
                rect: knob,
                track: sender,
                to,
                selected: false,
                name: label,
                place,
            },
        );
        if active {
            self.held_next = Some((sender.id, to.id));
        }
        active
    }

    /// "+N more ▾" under a return's four rows, opening every sender.
    fn all_senders(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        heard: &Heard,
        (order, to): (&[&Track], &Return),
        at: egui::Pos2,
    ) -> bool {
        let hidden = order.len() - RETURN_ROWS;
        let response = plus_button(
            ui,
            Plus {
                id: EguiId::new(("all senders", to.id)),
                text: format!("+{hidden} more"),
                name: format!(
                    "{hidden} more tracks sending into {} {}",
                    to.letter, to.name
                ),
                tip: "Every track sending here",
                selected: false,
                dot: None,
                at,
            },
        );
        let mut active = false;
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(236.0);
                ui.label(
                    egui::RichText::new(format!("Sends into {} {}", to.letter, to.name))
                        .font(fonts::medium(theme::size::BODY)),
                );
                ui.separator();
                for sender in order {
                    let (row, _) = ui.allocate_exact_size(vec2(236.0, STEP), Sense::hover());
                    let row = row.shrink2(vec2(4.0, 0.0));
                    active |= self.sender_row(ui, doc, heard, (sender, to), row, "return list");
                }
            });
        active
    }
}

/// A return's letter badge, left-centred at `at`, as on its header.
fn letter_badge(ui: &Ui, at: egui::Pos2, to: &Return) -> Rect {
    let badge = Rect::from_min_size(
        pos2(at.x, at.y - 8.0),
        vec2(16.0_f32.max(9.0 * to.letter.len() as f32 + 7.0), 16.0),
    )
    .round_to_pixels(ui.ctx().pixels_per_point());
    let painter = ui.painter();
    painter.rect_filled(badge, theme::radius::SMALL, to.color);
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        &to.letter,
        fonts::medium(theme::size::CAPTION),
        theme::text_on(to.color),
    );
    badge
}

/// `text` cut to fit `width`, ending in "…".
fn shorten(painter: &egui::Painter, text: &str, font: FontId, width: f32) -> String {
    let mut cut = text.to_owned();
    while !cut.is_empty() {
        cut.pop();
        let galley = painter.layout_no_wrap(format!("{cut}\u{2026}"), font.clone(), Color32::WHITE);
        if galley.size().x <= width {
            break;
        }
    }
    format!("{cut}\u{2026}")
}
