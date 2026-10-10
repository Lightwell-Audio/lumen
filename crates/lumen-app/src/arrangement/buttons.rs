//! The M and S buttons (S is a headphone button on Cue), shared by track
//! headers and mixer strips (M4 slice 5): lit when set on that track, a
//! hollow ring when the track is silenced by another's solo or a muted
//! group (M), or heard through a soloed group (S) (the owner's choice,
//! slice 4).

use eframe::egui::{self, Align2, Color32, Id as EguiId, Rect, Sense, Stroke, Ui};
use lumen_project::model::Track;
use lumen_project::solo::Hearing;

use super::Arrangement;
use crate::document::Document;
use crate::{fonts, motion, theme};

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
        let toggle = |ui: &mut Ui, r: Rect, button: Toggle, on_fill: Color32| {
            let (what, label, on, implied) = match button {
                Toggle::Mute => ("mute", "Mute", track.mute, hears.silenced),
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
            let rest = look.fill.lerp_to_gamma(look.hover, hover);
            let p = ui.painter();
            let text = if on {
                p.rect_filled(r, theme::radius::SMALL, on_fill);
                theme::text_on(on_fill)
            } else if implied {
                // Hollow: the state's colour as a ring, the letter in it.
                p.rect_filled(r, theme::radius::SMALL, rest.gamma_multiply(0.5));
                p.rect_stroke(
                    r.shrink(0.75),
                    theme::radius::SMALL,
                    Stroke::new(1.5, on_fill),
                    egui::StrokeKind::Middle,
                );
                on_fill
            } else {
                p.rect_filled(r, theme::radius::SMALL, rest);
                look.text
            };
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
            response.clicked()
        };
        if toggle(ui, mute_rect, Toggle::Mute, palette.start_marker) {
            doc.set_mute(&self.targets(track.id), !track.mute);
        }
        // Ableton's way by default: a click solos (or cues) this track
        // alone, Ctrl+click adds it; the additive preference swaps them.
        let exclusive = modifiers.command == additive;
        if cueing {
            if toggle(ui, solo_rect, Toggle::Cue, palette.accent) {
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
        } else if toggle(ui, solo_rect, Toggle::Solo, palette.accent) {
            doc.set_solo(&self.targets(track.id), !track.solo, exclusive);
        }
    }
}
