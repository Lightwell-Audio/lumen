use eframe::egui::{Rect, RichText, Sense, Ui, vec2};
use lumen_project::envelope::Kind;
use lumen_project::model::{Clip, WarpMode};

use super::strip_chips::{CHEVRON, popover_chip, text_width};
use crate::app::clip_card::{CARD_WIDTH, row as card_row};
use crate::audio_editor::AudioEditor;
use crate::commands::Command;
use crate::widgets::{self, Menu, SliderKind, raised};
use crate::{App, theme};

const WIDEST_TRANSPOSE: &str = "\u{2212}44.44 st";

fn transposed(clip: &Clip) -> Option<String> {
    let cents = clip.warp.as_ref()?.transpose_cents;
    (cents != 0).then(|| SliderKind::Semitones.text(cents as f32 / 100.0))
}

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

fn tip_text(clip: &Clip) -> &'static str {
    if clip.envelopes.pitch.is_some() {
        "Transpose and pitch envelope"
    } else {
        "Transpose the clip, and draw how its pitch moves"
    }
}

pub(super) fn sizing_text(clip: &Clip) -> String {
    let mut text = format!("Pitch {WIDEST_TRANSPOSE}");
    if clip.envelopes.pitch.is_some() {
        text.push_str(" env");
    }
    text
}

pub(super) fn chip_width(ui: &Ui, clip: &Clip) -> f32 {
    let s = theme::space::S;
    let font = eframe::egui::FontId::proportional(theme::size::BODY);
    s + text_width(ui, &sizing_text(clip), font).ceil() + 1.0 + s + CHEVRON + s
}

pub(super) fn envelope_range_text(clip: &Clip) -> Option<String> {
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for point in clip.envelopes.pitch.as_ref()?.points() {
        low = low.min(point.value);
        high = high.max(point.value);
    }
    if !low.is_finite() {
        return None;
    }
    let high_text = SliderKind::Semitones.text(high);
    if low >= high {
        return Some(high_text);
    }
    let low_text = SliderKind::Semitones.text(low);
    let low = low_text.strip_suffix(" st").unwrap_or(&low_text);
    Some(format!("{low} to {high_text}"))
}

pub(super) fn why_silent(clip: &Clip) -> Option<&'static str> {
    match clip.warp.as_ref().map(|w| w.mode) {
        None => Some("Warp the clip first: its pitch changes in Melodic, Full mix or Rhythmic."),
        Some(WarpMode::Repitch) => Some(
            "Repitch changes speed and pitch together, as the tempo asks. Use Melodic to change \
             the pitch and keep the speed.",
        ),
        Some(_) => None,
    }
}

impl App {
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
        let response = widgets::tip(response, "Pitch", None, Some(tip_text(clip)));
        Menu::below(&response).staying().show(|ui| {
            ui.set_width(CARD_WIDTH);
            self.pitch_popover(ui, clip, ed);
        });
        response.has_focus()
    }

    pub(super) fn pitch_popover(&mut self, ui: &mut Ui, clip: &Clip, ed: &mut AudioEditor) {
        let palette = *theme::p();
        let fixed = self.strip_fixed(clip.id);
        let editable = fixed.why().is_none();
        let mode = clip.warp.as_ref().map(|warp| warp.mode);
        let transposes = mode.is_some_and(WarpMode::transposes);
        let silent = why_silent(clip);
        let base = ui.id().with(("pitch popover", clip.id.0));

        if let Some(why) = silent {
            widgets::hint(ui, why);
            if mode == Some(WarpMode::Repitch) {
                let use_melodic = ui
                    .add_enabled_ui(editable, |ui| raised::button(ui, "Use Melodic"))
                    .inner;
                if let Some(reason) = fixed.why() {
                    use_melodic.clone().on_disabled_hover_text(reason);
                }
                if use_melodic.clicked() && editable {
                    self.select_opened_clip(clip.id);
                    self.set_clip_warp_mode(clip.id, WarpMode::Melodic);
                }
            }
        }

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

        if let Some(range) = envelope_range_text(clip) {
            card_row(ui, "Envelope", |ui| {
                ui.label(RichText::new(range).color(palette.text_secondary));
            });
            let remove = ui
                .add_enabled_ui(editable, |ui| raised::button(ui, "Remove envelope"))
                .inner;
            if let Some(reason) = fixed.why() {
                remove.clone().on_disabled_hover_text(reason);
            }
            if remove.clicked() && editable {
                self.select_opened_clip(clip.id);
                if let Err(why) = self.document.clear_clip_pitch_envelope(clip.id) {
                    self.say(why.reason().to_owned());
                }
            }
            if silent.is_some() {
                widgets::hint(
                    ui,
                    "Not heard until the clip is warped in a mode that changes pitch: \
                     Melodic, Full mix or Rhythmic.",
                );
            }
        }

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
        if silent.is_none() && mode == Some(WarpMode::Rhythmic) {
            widgets::hint(
                ui,
                "Rhythmic plays each hit like a sampler: a higher pitch also makes it shorter.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Context, RawInput};
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

    fn lane(points: &[f32]) -> Lane {
        Lane::new(
            points
                .iter()
                .enumerate()
                .map(|(i, &value)| Point::new(Ticks::quarters(i as i64), value))
                .collect(),
        )
        .unwrap()
    }

    fn context() -> Context {
        let ctx = Context::default();
        crate::fonts::install(&ctx);
        ctx.run_ui(RawInput::default(), |_| {})
            .textures_delta
            .clear();
        ctx
    }

    #[test]
    fn the_chip_says_what_is_set() {
        let mut c = clip();
        assert_eq!(chip_text(&c), "Pitch");
        assert_eq!(chip_name(&c), "Pitch: not changed");
        assert_eq!(
            tip_text(&c),
            "Transpose the clip, and draw how its pitch moves"
        );
        let mut warp = Warp::new(120.0, 0.0);
        warp.mode = WarpMode::Melodic;
        warp.transpose_cents = 250;
        c.warp = Some(warp);
        assert_eq!(chip_text(&c), "Pitch +2.5 st");
        c.envelopes.pitch = Some(lane(&[1.0]));
        assert_eq!(chip_text(&c), "Pitch +2.5 st env");
        assert_eq!(
            chip_name(&c),
            "Pitch: transposed +2.5 st, with a pitch envelope"
        );
        assert_eq!(tip_text(&c), "Transpose and pitch envelope");
        c.warp.as_mut().unwrap().transpose_cents = 0;
        assert_eq!(chip_text(&c), "Pitch env");
    }

    #[test]
    fn the_chip_keeps_one_width_whatever_the_transpose_is() {
        let ctx = context();
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            let mut c = clip();
            let mut warp = Warp::new(120.0, 0.0);
            warp.mode = WarpMode::Melodic;
            c.warp = Some(warp);
            let mut widths = Vec::new();
            for cents in [-2_400, -524, 0, 200, 2_400] {
                c.warp.as_mut().unwrap().transpose_cents = cents;
                widths.push(chip_width(ui, &c));
            }
            assert!(
                widths.windows(2).all(|pair| pair[0] == pair[1]),
                "one width through the drag: {widths:?}"
            );
            c.envelopes.pitch = Some(lane(&[2.0]));
            let with = chip_width(ui, &c);
            assert!(with > widths[0], "the envelope's words are measured too");
            for cents in [-2_400, -524, 0, 200, 2_400] {
                c.warp.as_mut().unwrap().transpose_cents = cents;
                assert_eq!(chip_width(ui, &c), with, "{cents}");
            }
        });
        out.textures_delta.clear();
    }

    #[test]
    fn every_transpose_label_fits_the_width_the_chip_is_given() {
        let ctx = context();
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            let mut c = clip();
            let mut warp = Warp::new(120.0, 0.0);
            warp.mode = WarpMode::Melodic;
            c.warp = Some(warp);
            let font = eframe::egui::FontId::proportional(theme::size::BODY);
            let width = chip_width(ui, &c);
            let widest = text_width(ui, &sizing_text(&c), font.clone());
            let room = width - 3.0 * theme::space::S - CHEVRON;
            let mut worst = 0.0_f32;
            for cents in (-4_800..=4_800).step_by(1) {
                c.warp.as_mut().unwrap().transpose_cents = cents;
                let text = chip_text(&c);
                let label = text_width(ui, &text, font.clone());
                assert!(label <= room, "{text} ({label}) in {room}");
                worst = worst.max(label);
            }
            assert!(
                worst <= widest,
                "the widest label the formatter makes is {worst}, laid out for {widest}"
            );
            c.envelopes.pitch = Some(lane(&[2.0]));
            let width = chip_width(ui, &c);
            let widest = text_width(ui, &sizing_text(&c), font.clone());
            assert!(
                widest > 0.0 && width > widest,
                "the envelope has room of its own"
            );
            for cents in [-4_444, 0, 4_444] {
                c.warp.as_mut().unwrap().transpose_cents = cents;
                let label = text_width(ui, &chip_text(&c), font.clone());
                assert!(
                    label <= width - 3.0 * theme::space::S - CHEVRON,
                    "{label} in {width}"
                );
            }
        });
        out.textures_delta.clear();
    }

    #[test]
    fn the_envelopes_range_reads_in_semitones() {
        let mut c = clip();
        assert_eq!(envelope_range_text(&c), None, "no envelope");
        c.envelopes.pitch = Some(lane(&[17.76]));
        assert_eq!(envelope_range_text(&c).as_deref(), Some("+17.76 st"));
        c.envelopes.pitch = Some(lane(&[17.76, -3.0, 4.0]));
        assert_eq!(
            envelope_range_text(&c).as_deref(),
            Some("\u{2212}3 to +17.76 st")
        );
        c.envelopes.pitch = Some(lane(&[-3.0, 2.0, 17.76]));
        assert_eq!(
            envelope_range_text(&c).as_deref(),
            Some("\u{2212}3 to +17.76 st"),
            "the order of the points does not matter"
        );
    }

    #[test]
    fn pitch_is_silent_until_the_clip_is_warped_in_a_mode_that_transposes() {
        let mut c = clip();
        assert!(why_silent(&c).is_some_and(|w| w.starts_with("Warp the clip")));
        let mut warp = Warp::new(120.0, 0.0);
        c.warp = Some(warp.clone());
        assert!(why_silent(&c).is_some_and(|w| w.starts_with("Repitch changes speed")));
        for mode in [WarpMode::Melodic, WarpMode::FullMix, WarpMode::Rhythmic] {
            warp.mode = mode;
            c.warp = Some(warp.clone());
            assert!(why_silent(&c).is_none(), "{mode:?}");
        }
    }
}
