//! The Pitch chip of the audio editor's strip (M9.5 S6, ADR-0137). It sits
//! beside the Warp chip and opens a popover: the clip's transpose in semitones
//! and cents, and a switch that shows the pitch envelope over the wave (the
//! header's Pitch pill is the same switch). Every control edits the opened
//! clip, as Gain does, and is one undo step; the chip is a Tab stop with an
//! accessible name, opened by a click, Enter or Space and closed by Esc.
//!
//! Pitch is heard only while the clip is warped in a mode that transposes
//! (`WarpMode::transposes`): Melodic and Full mix shift the pitch and keep the
//! speed, Rhythmic plays each hit like a sampler, and Repitch has the pitch
//! the tempo gives it. The popover says which of these the clip is in.

use eframe::egui::{Rect, Sense, Ui, vec2};
use lumen_project::envelope::Kind;
use lumen_project::model::{Clip, WarpMode};

use super::strip_chips::{CHEVRON, popover_chip, text_width};
use crate::app::clip_card::{CARD_WIDTH, row as card_row};
use crate::audio_editor::AudioEditor;
use crate::commands::Command;
use crate::widgets::{self, Menu, SliderKind};
use crate::{App, theme};

/// The transpose in words ("+2 st"), or `None` when there is none.
fn transposed(clip: &Clip) -> Option<String> {
    let cents = clip.warp.as_ref()?.transpose_cents;
    (cents != 0).then(|| SliderKind::Semitones.text(cents as f32 / 100.0))
}

/// What the Pitch chip says: "Pitch", "Pitch +2 st", "Pitch env" (an envelope
/// and no transpose) or "Pitch +2 st env".
pub(super) fn chip_text(clip: &Clip) -> String {
    let mut text = "Pitch".to_owned();
    if let Some(shift) = transposed(clip) {
        text.push(' ');
        text.push_str(&shift);
    }
    if clip.envelopes.pitch.is_some() {
        text.push_str(" env");
    }
    text
}

/// The chip's accessible name: what it is and what is set.
fn chip_name(clip: &Clip) -> String {
    let mut parts = Vec::new();
    if let Some(shift) = transposed(clip) {
        parts.push(format!("transposed {shift}"));
    }
    if clip.envelopes.pitch.is_some() {
        parts.push("with a pitch envelope".to_owned());
    }
    if parts.is_empty() {
        "Pitch: not changed".to_owned()
    } else {
        format!("Pitch: {}", parts.join(", "))
    }
}

/// How wide the Pitch chip is for `text`: the label, rounded up to a whole
/// point and given one more, so a chip given exactly this width never cuts
/// its label for a fraction of a pixel.
pub(super) fn chip_width(ui: &Ui, text: &str) -> f32 {
    let s = theme::space::S;
    let font = eframe::egui::FontId::proportional(theme::size::BODY);
    s + text_width(ui, text, font).ceil() + 1.0 + s + CHEVRON + s
}

/// Why the clip's pitch cannot be heard, in words, or `None` when it can.
pub(super) fn why_silent(clip: &Clip) -> Option<&'static str> {
    match clip.warp.as_ref().map(|w| w.mode) {
        None => Some("Warp the clip first: its pitch changes in Melodic, Full mix or Rhythmic."),
        Some(WarpMode::Repitch) => Some(
            "Set by tempo in Repitch. Pick Melodic or Full mix to change the pitch and keep the \
             speed.",
        ),
        Some(_) => None,
    }
}

impl App {
    /// The Pitch chip in `rect`: a Tab stop that opens the popover. True when
    /// it has the keyboard.
    pub(super) fn pitch_chip(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        clip: &Clip,
        ed: &mut AudioEditor,
    ) -> bool {
        let set = transposed(clip).is_some() || clip.envelopes.pitch.is_some();
        let response = popover_chip(
            ui,
            rect,
            ui.id().with(("audio strip pitch", clip.id.0)),
            &chip_text(clip),
            (set, chip_name(clip)),
        );
        let response = widgets::tip(
            response,
            "Pitch",
            None,
            Some("Transpose the clip, and draw how its pitch moves"),
        );
        Menu::below(&response).staying().show(|ui| {
            ui.set_width(CARD_WIDTH);
            self.pitch_popover(ui, clip, ed);
        });
        response.has_focus()
    }

    /// What the Pitch chip opens.
    pub(super) fn pitch_popover(&mut self, ui: &mut Ui, clip: &Clip, ed: &mut AudioEditor) {
        let fixed = self.strip_fixed(clip.id);
        let editable = fixed.why().is_none();
        let transposes = clip
            .warp
            .as_ref()
            .is_some_and(|warp| warp.mode.transposes());
        let base = ui.id().with(("pitch popover", clip.id.0));
        ui.add_enabled_ui(editable && transposes, |ui| {
            card_row(ui, "Transpose", |ui| {
                let cents = clip.warp.as_ref().map_or(0, |w| w.transpose_cents);
                let semitones = cents as f32 / 100.0;
                let (rect, _) = ui.allocate_exact_size(
                    vec2(theme::size::WARP_SLIDER, theme::size::CONTROL_SMALL),
                    Sense::hover(),
                );
                let next = widgets::value_slider(
                    ui,
                    rect,
                    base.with("transpose"),
                    semitones,
                    SliderKind::Semitones,
                    "Transpose",
                    theme::controls(false),
                );
                let next_cents = (next * 100.0).round() as i32;
                if next_cents != cents {
                    self.select_opened_clip(clip.id);
                    self.set_clip_transpose(clip.id, next_cents);
                }
            });
        });
        let mut on = ed.pill == Some(Kind::Pitch);
        let keys = self.keymap.keys(Command::AudioEditorPitchEnvelope);
        let switch = widgets::switch(ui, &mut on, "Pitch envelope", true);
        let switch = widgets::tip(
            switch,
            "Pitch envelope",
            keys.as_ref(),
            Some("Show the pitch envelope over the wave, to add and move its points"),
        );
        if switch.changed() {
            ed.toggle_pill(Kind::Pitch);
        }
        if let Some(why) = why_silent(clip) {
            widgets::hint(ui, why);
        } else if clip
            .warp
            .as_ref()
            .is_some_and(|w| w.mode == WarpMode::Rhythmic)
        {
            widgets::hint(
                ui,
                "Rhythmic plays each hit like a sampler: a higher pitch also makes it shorter.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::automation::{Lane, Point};
    use lumen_core::time::Ticks;
    use lumen_project::model::{Clip, ClipContent, Id, Warp};

    use super::*;

    fn clip() -> Clip {
        Clip {
            id: Id(1),
            name: "c".into(),
            start: Ticks::ZERO,
            length: Ticks::quarters(4),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            crossfade: false,
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Ticks::ZERO,
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            envelopes: Default::default(),
            takes: None,
            warp: None,
            content: ClipContent::Audio {
                media: Id(2),
                offset: 0,
                gain_db: 0.0,
            },
        }
    }

    #[test]
    fn the_chip_says_what_is_set() {
        let mut c = clip();
        assert_eq!(chip_text(&c), "Pitch");
        assert_eq!(chip_name(&c), "Pitch: not changed");
        let mut warp = Warp::new(120.0, 0.0);
        warp.mode = WarpMode::Melodic;
        warp.transpose_cents = 250;
        c.warp = Some(warp);
        assert_eq!(chip_text(&c), "Pitch +2.5 st");
        c.envelopes.pitch = Some(Lane::new(vec![Point::new(Ticks::ZERO, 1.0)]).unwrap());
        assert_eq!(chip_text(&c), "Pitch +2.5 st env");
        assert_eq!(
            chip_name(&c),
            "Pitch: transposed +2.5 st, with a pitch envelope"
        );
        c.warp.as_mut().unwrap().transpose_cents = 0;
        assert_eq!(chip_text(&c), "Pitch env");
    }

    #[test]
    fn pitch_is_silent_until_the_clip_is_warped_in_a_mode_that_transposes() {
        let mut c = clip();
        assert!(why_silent(&c).is_some_and(|w| w.starts_with("Warp the clip")));
        let mut warp = Warp::new(120.0, 0.0);
        c.warp = Some(warp.clone());
        assert!(why_silent(&c).is_some_and(|w| w.starts_with("Set by tempo")));
        for mode in [WarpMode::Melodic, WarpMode::FullMix, WarpMode::Rhythmic] {
            warp.mode = mode;
            c.warp = Some(warp.clone());
            assert!(why_silent(&c).is_none(), "{mode:?}");
        }
    }
}
