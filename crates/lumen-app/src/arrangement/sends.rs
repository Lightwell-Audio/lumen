//! Sends on track headers (M4 slice 4, the owner's K1 comet): on an
//! unfolded audio, MIDI or group track, a control per return, lettered and
//! coloured as the return is. Hovering one lights its return's header, and
//! hovering a return lights every send that feeds it.
//!
//! M9 slice R1b (the owner's B·1, ADR-0101): on a header the sends are
//! slots in one sunk well, every return always shown (up to twelve), on the
//! third row beside the In chip. The interface audit (X1–X4) puts the
//! same well on a mixer strip, under its rule; lists keep the knobs.
//!
//! MT-MIX-4 run 1 (the owner's choices from mock-ups): knobs step up on a
//! selected row (C4); an unfolded return lists the tracks sending to it,
//! each with its knob (R2). (O1's "+N ▾" list of a track's sends went
//! with the wells, which show every return.)
//!
//! MT-MIX-4 run 2: a return lists its four loudest senders, then "+N more"
//! opens them all (L4); a send turned down to silence leaves the list once
//! it is let go (S1). The send itself stays on the track, pre- or
//! post-fader as it was.

use eframe::egui::{self, Align2, Color32, FontId, Id as EguiId, Rect, Sense, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_project::automation::Target;
use lumen_project::model::{Id, Level, Project, Track, TrackKind};
use lumen_project::solo::Heard;

use super::{Arrangement, STEP, color_of, pinned};
use crate::document::Document;
use crate::theme::Tone;
use crate::widgets::knob::{SendKnob, Style, send_knob};
use crate::widgets::{db_text, draw_arrow, raised};
use crate::{fonts, motion, theme, widgets};

/// The gap between the slots of a send well, and its padding.
const SLOT_GAP: f32 = 1.0;
const WELL_PAD: f32 = theme::space::XXS;
/// The narrowest a slot gets (a deeply nested track with twelve returns).
const SLOT_MIN: f32 = 4.0;
/// A knob in a list (the "+N" list, a return's senders): small enough
/// that knobs on 22 pt rows never touch.
const SMALL_KNOB: f32 = theme::size::KNOB_SMALL;
/// A return's "+N more" list scrolls past this many senders.
const LIST_ROWS: usize = 10;
/// A return lists this many senders, then "+N more" (the owner's L4).
pub(super) const RETURN_ROWS: usize = 4;

/// Whether a track of `kind` shows sends: returns and the master do not.
pub(super) fn has_sends(kind: TrackKind) -> bool {
    matches!(kind, TrackKind::Audio | TrackKind::Midi | TrackKind::Group)
}

/// How wide each of `count` slots is in a send well given `room` points,
/// and how wide the well is: as wide as fits, up to `SEND_SLOT`.
pub(super) fn slot_layout(count: usize, room: f32) -> (f32, f32) {
    if count == 0 {
        return (0.0, 0.0);
    }
    let n = count as f32;
    let slot = ((room - 2.0 * WELL_PAD - (n - 1.0) * SLOT_GAP) / n)
        .clamp(SLOT_MIN, theme::size::SEND_SLOT);
    (slot, 2.0 * WELL_PAD + n * slot + (n - 1.0) * SLOT_GAP)
}

/// A return, as its sends show it.
#[derive(Debug, Clone)]
pub(super) struct Return {
    pub(super) id: Id,
    pub(super) letter: String,
    pub(super) name: String,
    pub(super) color: Color32,
}

pub(super) fn returns(project: &Project) -> Vec<Return> {
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
pub(super) struct Knob<'a> {
    pub(super) rect: Rect,
    pub(super) track: &'a Track,
    pub(super) to: &'a Return,
    pub(super) selected: bool,
    pub(super) name: String,
    /// Where it is drawn ("header", "list", "return", "return list",
    /// "strip"): each place's knobs are their own widgets.
    pub(super) place: &'static str,
}

/// The "+N more ▾" button of a return's senders, drawn as "+ Add track ▾" is.
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
    // A raised menu button (ADR-0052), on while its list is open.
    let face = raised::Face::new(look)
        .hover(hover)
        .pressed(response.is_pointer_button_down_on())
        .lit(
            motion::hover(&ctx, response.id.with("open"), open),
            Tone::On,
        );
    let body = raised::paint(
        ui.painter(),
        button,
        theme::radius::for_height(button.height()),
        &face,
    );
    let painter = ui.painter();
    let mut x = button.left() + 8.0;
    if let Some(color) = plus.dot {
        painter.circle_filled(pos2(x + 3.0, button.center().y), 3.0, color);
        x += dot;
    }
    let text_width = galley.size().x;
    painter.galley_with_override_text_color(
        pos2(x, body.center().y - galley.size().y / 2.0),
        galley,
        face.text(),
    );
    raised::focus_ring(painter, &response, button);
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
    pub(super) fn send_control(
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
        // On a header or strip the send is a slot in the well (ADR-0101).
        let slot = matches!(place, "header" | "strip");
        let target = Target::Send { to: to.id };
        let own = send.map_or(f32::NEG_INFINITY, |s| s.level.0);
        let out = send_knob(
            ui,
            rect,
            id,
            SendKnob {
                style: if slot {
                    Style::Slot { title: &to.name }
                } else {
                    Style::Ring
                },
                letter: &to.letter,
                color: to.color,
                db: self.shown(doc, track, target, own),
                pre_fader: send.is_some_and(|s| s.pre_fader),
                heard: heard.of(track.id).sends,
                // In a return's own list every knob feeds it: no ring.
                linked: self.hot_return == Some(to.id) && !place.starts_with("return"),
                selected,
                name: &name,
                live_value: slot,
                mapped: doc
                    .mapping_of(track.id, target)
                    .map(|m| (m.channel, m.number)),
                waiting: self.auto.learning == Some((track.id, target)),
            },
        );
        if let Some(asked) = out.ask {
            super::control_menu::ask(&mut self.auto, asked, track.id, target);
        }
        if self.auto.learning == Some((track.id, target)) {
            if slot {
                super::follow::ring_with(ui, rect, crate::theme::p().accent);
            } else {
                super::follow::ring_round_with(ui, rect, crate::theme::p().accent);
            }
        }
        if out.hovered {
            self.hot_return_next = Some(to.id);
        }
        if self.taken_over(doc, track, target) {
            if slot {
                super::follow::ring(ui, rect);
            } else {
                super::follow::ring_round(ui, rect);
            }
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

    /// The send well of a track in `room` (an unfolded header's third row,
    /// left column, or a strip's sends row, centred): one sunk well, a slot
    /// per return in its colour, filling from the bottom with the send's
    /// level (the owner's B·1, ADR-0101).
    pub(super) fn send_well(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        room: Rect,
        (selected, in_strip): (bool, bool),
    ) {
        let returns = returns(doc.project());
        if returns.is_empty() {
            return;
        }
        let heard = lumen_project::solo::hearing(doc.project());
        let ppp = ui.ctx().pixels_per_point();
        let (slot, width) = slot_layout(returns.len(), room.width());
        let left = if in_strip {
            room.center().x - width / 2.0
        } else {
            room.left()
        };
        let well = Rect::from_min_size(pos2(left, room.top()), vec2(width, room.height()))
            .round_to_pixels(ppp);
        // Headers and strips each have their own slots.
        let (place, suffix) = if in_strip {
            ("strip", " in the mixer")
        } else {
            ("header", "")
        };
        raised::paint_sunken(ui.painter(), well, theme::radius::SMALL);
        for (n, to) in returns.iter().enumerate() {
            let x = well.left() + WELL_PAD + n as f32 * (slot + SLOT_GAP);
            let rect = Rect::from_min_max(
                pos2(x, well.top() + WELL_PAD),
                pos2(x + slot, well.bottom() - WELL_PAD),
            )
            .round_to_pixels(ppp);
            let name = format!("Send {} to {} {}{suffix}", track.name, to.letter, to.name);
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
                    place,
                },
            );
        }
    }

    /// An unfolded return lists the tracks sending to it, each with its send
    /// knob and level (the owner's R2), from the third row down (`first`,
    /// the third row's left column): the four loudest, then "+N more" for
    /// them all (L4). A send turned down to silence leaves the list once let
    /// go (S1).
    pub(super) fn senders_rows(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        ret: &Track,
        (rect, first): (Rect, Rect),
    ) {
        let palette = *theme::p();
        let Some(to) = returns(doc.project()).into_iter().find(|r| r.id == ret.id) else {
            return;
        };
        let project = doc.project().clone();
        let held = self.held_send.filter(|(_, r)| *r == ret.id);
        let senders = senders(&project, ret.id, held);
        let top = first.center().y - STEP / 2.0;
        let left = first.left();
        let right = rect.right() - super::headers::CONTENT_RIGHT;
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
        let order = self.sender_order(ret.id, &senders);
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

    /// The senders of return `id` in the order its list shows them:
    /// loudest first when a sender joins or leaves (or the list is first
    /// drawn), and otherwise kept as it was. Changing a level never moves a
    /// row: with two senders of the same name (a duplicated track), a
    /// re-sort after each change swapped them, and setting one seemed to
    /// change the other (owner, MT-MIX-4 run 4).
    fn sender_order<'p>(&mut self, id: Id, senders: &[&'p Track]) -> Vec<&'p Track> {
        let known = self.sender_order.entry(id).or_default();
        let same_set =
            known.len() == senders.len() && senders.iter().all(|t| known.contains(&t.id));
        if !same_set {
            let mut sorted: Vec<&Track> = senders.to_vec();
            sorted.sort_by(|a, b| level_into(b, id).total_cmp(&level_into(a, id)));
            *known = sorted.iter().map(|t| t.id).collect();
        }
        known
            .iter()
            .filter_map(|k| senders.iter().find(|t| t.id == *k).copied())
            .collect()
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
            widgets::shorten(painter, &sender.name, font.clone(), name_room.width())
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
        widgets::Menu::below(&response).show(|ui| {
            ui.set_min_width(236.0);
            ui.label(
                egui::RichText::new(format!("Sends into {} {}", to.letter, to.name))
                    .font(fonts::medium(theme::size::BODY)),
            );
            ui.separator();
            // Scrolls past ten rows (MT-MIX-4 run 3: a long list ran off
            // the window).
            egui::ScrollArea::vertical()
                .max_height(LIST_ROWS as f32 * STEP + raised::ring_room_height())
                .show(ui, |ui| {
                    raised::ring_room(ui, |ui| {
                        for sender in order {
                            let (row, _) =
                                ui.allocate_exact_size(vec2(236.0, STEP), Sense::hover());
                            let row = row.shrink2(vec2(10.0, 0.0));
                            active |=
                                self.sender_row(ui, doc, heard, (sender, to), row, "return list");
                        }
                    });
                });
        });
        active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_sends_fit_the_narrowest_header() {
        // 204 pt column: 118 pt for the sends.
        let (slot, width) = slot_layout(12, 118.0);
        assert!((slot - 103.0 / 12.0).abs() < 1e-4, "{slot}");
        assert!(width <= 118.0 + 1e-4);
        assert!(
            slot < theme::size::SEND_SLOT_LETTER,
            "no letters this narrow"
        );
        // A few returns get full slots, with their letters.
        let (slot, width) = slot_layout(3, 118.0);
        assert_eq!(slot, theme::size::SEND_SLOT);
        assert_eq!(width, 4.0 + 3.0 * 18.0 + 2.0);
        // A wider column shows letters for twelve.
        let (slot, _) = slot_layout(12, 194.0);
        assert!(slot >= theme::size::SEND_SLOT_LETTER);
        // Never narrower than a few points, never nothing.
        assert_eq!(slot_layout(12, 10.0).0, SLOT_MIN);
        assert_eq!(slot_layout(0, 118.0), (0.0, 0.0));
    }
}
