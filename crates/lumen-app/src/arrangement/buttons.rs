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

const HOLLOW_MUTE: f32 = 0.14;
const HOLLOW_SOLO: f32 = 0.12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Toggle {
    Mute,
    Solo,
    Cue,
}

impl Arrangement {
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
        let additive = self.additive_solo;
        let cueing = self.cue.cueing();
        let cued = self.cued.contains(&track.id);
        let muted = super::follow::muted(self.shown(
            doc,
            track,
            Target::Mute,
            f32::from(u8::from(track.mute)),
        ));
        let mute_over = self.taken_over(doc, track, Target::Mute);
        let toggle = |ui: &mut Ui, r: Rect, button: Toggle, tone: Tone| {
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
            let lit = motion::hover(ui.ctx(), id.with(what).with("lit"), on);
            let face = raised::Face::new(look)
                .hover(hover)
                .pressed(response.is_pointer_button_down_on())
                .lit(lit, tone);
            let r = r.round_to_pixels(ui.pixels_per_point());
            let radius = well::PIECE_RADIUS;
            let ink = well::paint_piece(ui.painter(), r, &face);
            if mute_over && button == Toggle::Mute {
                super::follow::ring(ui, r);
            }
            let p = ui.painter();
            let mut text = ink;
            if implied && !on {
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
