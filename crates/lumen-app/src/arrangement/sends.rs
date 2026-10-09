//! Sends on track headers (M4 slice 4, the owner's K1 comet): on an
//! unfolded audio, MIDI or group track, a knob per return, lettered and
//! coloured as the return is. Hovering a knob lights its return's header,
//! and hovering a return lights every knob that feeds it.

use eframe::egui::{Align2, Color32, FontId, Id as EguiId, Rect, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{Level, Track, TrackKind};
use lumen_project::solo::Hearing;

use super::{Arrangement, FOLDED, STEP, color_of, pinned};
use crate::document::Document;
use crate::theme;
use crate::widgets::knob::{SendKnob, send_knob};

/// A send knob's size, and the gap between knobs.
const KNOB: f32 = 24.0;
const GAP: f32 = 5.0;

/// Whether a track of `kind` shows sends: returns and the master do not.
pub(super) fn has_sends(kind: TrackKind) -> bool {
    matches!(kind, TrackKind::Audio | TrackKind::Midi | TrackKind::Group)
}

/// The height a header needs to show its sends: the folded row, volume
/// and pan, then two steps for the knobs (such tracks unfold this tall).
pub(super) const SENDS_FROM: f32 = FOLDED + 3.0 * STEP;

impl Arrangement {
    /// The row of send knobs under volume and pan.
    pub(super) fn sends_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        rect: Rect,
        id: EguiId,
        hears: Hearing,
    ) {
        let palette = *theme::p();
        let returns: Vec<(lumen_project::model::Id, String, String, Color32)> = doc
            .project()
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Return)
            .enumerate()
            .map(|(n, t)| (t.id, pinned::letter(n), t.name.clone(), color_of(t.color)))
            .collect();
        if returns.is_empty() {
            return;
        }
        // Under volume and pan, with room to breathe above the row's edge.
        let center_y = rect.top() + FOLDED + 2.0 * STEP - 2.0;
        let left = rect.left() + 30.0;
        let right = rect.right() - 8.0;
        let fits = (((right - left) + GAP) / (KNOB + GAP)).floor().max(0.0) as usize;
        // Room for a "+N" after the knobs when they do not all fit.
        let shown = if returns.len() > fits {
            fits.saturating_sub(1)
        } else {
            returns.len()
        };
        let ppp = ui.ctx().pixels_per_point();
        for (n, (to, letter, name, color)) in returns.iter().take(shown).enumerate() {
            let x = left + n as f32 * (KNOB + GAP);
            let knob_rect = Rect::from_min_size(pos2(x, center_y - KNOB / 2.0), vec2(KNOB, KNOB))
                .round_to_pixels(ppp);
            let send = track.sends.iter().find(|s| s.to == *to);
            let label = format!("Send {} to {letter} {name}", track.name);
            let out = send_knob(
                ui,
                knob_rect,
                id.with(("send", to.0)),
                SendKnob {
                    letter,
                    color: *color,
                    db: send.map_or(f32::NEG_INFINITY, |s| s.level.0),
                    pre_fader: send.is_some_and(|s| s.pre_fader),
                    heard: hears.sends,
                    linked: self.hot_return == Some(*to),
                    name: &label,
                },
            );
            if out.hovered {
                self.hot_return_next = Some(*to);
            }
            if let Some(db) = out.db {
                let level = if db.is_finite() {
                    Level(db)
                } else {
                    Level::SILENT
                };
                if let Err(refused) = doc.set_send(track.id, *to, level) {
                    self.note = Some(refused.reason());
                }
            }
            if let Some(pre) = out.pre_fader
                && let Err(refused) = doc.set_send_pre_fader(track.id, *to, pre)
            {
                self.note = Some(refused.reason());
            }
        }
        if shown < returns.len() {
            let x = left + shown as f32 * (KNOB + GAP);
            ui.painter().text(
                pos2(x + 2.0, center_y),
                Align2::LEFT_CENTER,
                format!("+{}", returns.len() - shown),
                FontId::proportional(theme::size::CAPTION),
                palette.text_hint,
            );
        }
    }
}
