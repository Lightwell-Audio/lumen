//! The M and S buttons (S is a headphone button on Cue), shared by track
//! headers and mixer strips (M4 slice 5): pieces of one sunk well on
//! headers and strips (M9 slice R1b, ADR-0101; the interface audit's X1);
//! lit when set on that track, a hollow ring when the track is silenced by
//! another's solo or a muted group (M), or heard through a soloed group (S)
//! (the owner's choice, slice 4).

use eframe::egui::{self, Align2, Id as EguiId, Rect, Sense, Stroke, Ui};
use eframe::emath::GuiRounding as _;
use lumen_project::automation::Target;
use lumen_project::model::Track;
use lumen_project::solo::Hearing;

use super::Arrangement;
use super::control_menu::{control_menu_for, waiting_ring};
use crate::document::Document;
use crate::theme::{self, Tone};
use crate::widgets::{raised, well};
use crate::{fonts, motion};

/// How strongly a hollow M and S fill with their colour (the design
/// system's `.is-hollow-m` and `.is-hollow-s`).
const HOLLOW_MUTE: f32 = 0.14;
const HOLLOW_SOLO: f32 = 0.12;

/// The M and S buttons; S is a headphone button on Cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Toggle {
    Mute,
    Solo,
    Cue,
}

impl Arrangement {
    /// Draws `track`'s M and S buttons in `mute_rect` and `solo_rect` and
    /// applies their clicks (to the selection when the track is in it).
    /// `place` ends their accessible names ("" on headers, " in the mixer"
    /// on strips), so each control has a name of its own.
    pub(super) fn mute_solo(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        hears: Hearing,
        (mute_rect, solo_rect): (Rect, Rect),
        (id, look, place): (EguiId, theme::Controls, &str),
    ) {
        let palette = *theme::p();
        let modifiers = ui.input(|i| i.modifiers);
        // M and S: lit when set on this track; a hollow ring when the
        // track is silenced by another's solo or a muted group (M), or
        // heard through a soloed group (S) (the owner's choice, slice 4).
        let additive = self.additive_solo;
        // On Cue (the master's switch), the S button cues the track to
        // the headphones instead (the owner's C1).
        let cueing = self.cue.cueing();
        let cued = self.cued.contains(&track.id);
        // A mute lane that plays decides what M shows (M7 slice A4a).
        let muted = super::follow::muted(self.shown(
            doc,
            track,
            Target::Mute,
            f32::from(u8::from(track.mute)),
        ));
        let mute_over = self.taken_over(doc, track, Target::Mute);
        let toggle = |ui: &mut Ui, r: Rect, button: Toggle, tone: Tone| {
            // The ring and letter of a hollow button: mute's grey for M, the
            // accent for S and cue; its fill the same colour, faint.
            let (accent, faint) = match button {
                Toggle::Mute => (palette.mute, HOLLOW_MUTE),
                Toggle::Solo | Toggle::Cue => (palette.accent, HOLLOW_SOLO),
            };
            let (what, label, on, implied) = match button {
                Toggle::Mute => ("mute", "Mute", muted, hears.silenced),
                Toggle::Solo => ("solo", "Solo", track.solo, hears.soloed_through),
                Toggle::Cue => ("cue", "Cue", cued, false),
            };
            let tip = match (button, additive) {
                (Toggle::Cue, false) => {
                    "Cue to the headphones, before the fader: click for this track only, Ctrl+click to add it"
                }
                (Toggle::Cue, true) => {
                    "Cue to the headphones, before the fader: click to add this track, Ctrl+click for it only"
                }
                (Toggle::Mute, _) if implied => {
                    "Mute (0). Silent now: another track is soloed, or a group it plays through is muted"
                }
                (Toggle::Mute, _) => "Mute (0)",
                (Toggle::Solo, _) if implied => "Solo. Heard now through a soloed group",
                (Toggle::Solo, false) => "Solo: click for this track only, Ctrl+click to add it",
                (Toggle::Solo, true) => "Solo: click to add this track, Ctrl+click for it only",
            };
            let response = ui
                .interact(r, id.with(what), Sense::click())
                .on_hover_text(tip);
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    true,
                    on,
                    match (implied && !on, button) {
                        (true, Toggle::Mute) => format!("Mute {}{place} (silenced)", track.name),
                        (true, Toggle::Solo) => {
                            format!("Solo {}{place} (through its group)", track.name)
                        }
                        _ => format!("{} {}{place}", label, track.name),
                    },
                )
            });
            let hover = motion::hover(ui.ctx(), id.with(what).with("hover"), response.hovered());
            // Lighting up and going out cross-fade (`FAST`, M4.5).
            let lit = motion::hover(ui.ctx(), id.with(what).with("lit"), on);
            // The design system's TrackButtons (ADR-0052): a lit M is
            // `mute`, a lit S and the lit cue are filled accent.
            let face = raised::Face::new(look)
                .hover(hover)
                .pressed(response.is_pointer_button_down_on())
                .lit(lit, tone);
            let r = r.round_to_pixels(ui.pixels_per_point());
            // Pieces of one sunk well, on headers (ADR-0101) and mixer
            // strips (the interface audit's X1–X4).
            let radius = well::PIECE_RADIUS;
            let ink = well::paint_piece(ui.painter(), r, &face);
            if mute_over && button == Toggle::Mute {
                super::follow::ring(ui, r);
            }
            let p = ui.painter();
            let mut text = ink;
            if implied && !on {
                // Hollow: the state's colour as a ring, the letter in it.
                p.rect_filled(r, f32::from(radius), accent.gamma_multiply(faint));
                p.rect_stroke(
                    r,
                    f32::from(radius),
                    Stroke::new(theme::stroke::LIT, accent),
                    egui::StrokeKind::Inside,
                );
                text = accent;
            }
            if button == Toggle::Cue {
                crate::icons::draw_headphones(p, r.center(), text);
            } else {
                p.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    &label[..1],
                    fonts::medium(theme::size::CAPTION),
                    text,
                );
            }
            raised::focus_ring(p, &response, r);
            response.clicked()
        };
        if toggle(ui, mute_rect, Toggle::Mute, Tone::Mute) {
            doc.set_mute(&self.targets(track.id), !muted);
        }
        control_menu_for(
            ui,
            id.with("mute"),
            doc,
            &mut self.auto,
            track.id,
            Target::Mute,
        );
        waiting_ring(ui, &self.auto, mute_rect, track.id, Target::Mute);
        // Exclusive by default: a click solos (or cues) this track
        // alone, Ctrl+click adds it; the additive preference swaps them.
        let exclusive = modifiers.command == additive;
        if cueing {
            if toggle(ui, solo_rect, Toggle::Cue, Tone::Accent) {
                let targets = self.targets(track.id);
                if exclusive {
                    self.cued.retain(|id| targets.contains(id));
                }
                for id in targets {
                    if cued {
                        self.cued.remove(&id);
                    } else {
                        self.cued.insert(id);
                    }
                }
            }
        } else if toggle(ui, solo_rect, Toggle::Solo, Tone::Accent) {
            doc.set_solo(&self.targets(track.id), !track.solo, exclusive);
        }
    }
}
