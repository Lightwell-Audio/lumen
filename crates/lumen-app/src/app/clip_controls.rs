//! The clip controls that more than one place draws (M9.5 S3, D37): the
//! floating properties card and the audio editor's strip both show the gain
//! and the fades through these, so the two edit the same fields in the same
//! way, with the same undo steps.

use eframe::egui::{self, Rect, Ui, vec2};
use lumen_core::fade::FadeShape;
use lumen_core::time::{TempoMap, Ticks};
use lumen_project::edit::Edge;
use lumen_project::model::{Clip, ClipContent};

use crate::widgets::{self, SliderKind};
use crate::{App, theme};

/// How long the fade at `edge` of `clip` lasts, in milliseconds.
pub(crate) fn fade_ms(tempo: &TempoMap, clip: &Clip, edge: Edge) -> f32 {
    let end = clip.start + clip.length;
    let (a, b) = match edge {
        Edge::Start => (clip.start, clip.start + clip.fade_in.length),
        Edge::End => (end - clip.fade_out.length, end),
    };
    ((tempo.seconds_at(b) - tempo.seconds_at(a)) * 1000.0) as f32
}

/// How long `clip` lasts, in milliseconds.
pub(crate) fn clip_ms(tempo: &TempoMap, clip: &Clip) -> f32 {
    let end = clip.start + clip.length;
    ((tempo.seconds_at(end) - tempo.seconds_at(clip.start)) * 1000.0) as f32
}

/// The length of a fade at `edge` of `clip` that lasts `ms`.
pub(crate) fn fade_ticks(tempo: &TempoMap, clip: &Clip, edge: Edge, ms: f32) -> Ticks {
    let seconds = f64::from(ms.clamp(0.0, clip_ms(tempo, clip))) / 1000.0;
    let end = clip.start + clip.length;
    match edge {
        Edge::Start => tempo.ticks_at_seconds(tempo.seconds_at(clip.start) + seconds) - clip.start,
        Edge::End => end - tempo.ticks_at_seconds(tempo.seconds_at(end) - seconds),
    }
}

/// The name of a fade's controls: "Fade in" or "Fade out".
pub(crate) fn fade_label(edge: Edge) -> &'static str {
    match edge {
        Edge::Start => "Fade in",
        Edge::End => "Fade out",
    }
}

impl App {
    /// The gain slider of an audio `clip` in `rect`; one undo step per drag.
    /// `id` must be unique to the place that draws it.
    pub(crate) fn clip_gain_slider(&mut self, ui: &mut Ui, rect: Rect, clip: &Clip, id: egui::Id) {
        let ClipContent::Audio { gain_db, .. } = &clip.content else {
            return;
        };
        let db = widgets::value_slider(
            ui,
            rect,
            id,
            *gain_db,
            SliderKind::ClipGain,
            "Clip gain",
            theme::controls(false),
        );
        if (db - gain_db).abs() > 1e-4 {
            self.document
                .set_clip_gain(&[clip.id], db, Some("card gain"));
        }
    }

    /// The length slider of a fade in `rect`, in milliseconds; one undo
    /// step per drag. `id` must be unique to the place that draws it.
    pub(crate) fn clip_fade_slider(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        (clip, tempo): (&Clip, &TempoMap),
        edge: Edge,
        id: egui::Id,
    ) {
        let ms = fade_ms(tempo, clip, edge);
        let set = widgets::value_slider(
            ui,
            rect,
            id,
            ms,
            SliderKind::Millis {
                max_ms: clip_ms(tempo, clip),
            },
            fade_label(edge),
            theme::controls(false),
        );
        if (set - ms).abs() > 0.05 {
            let length = fade_ticks(tempo, clip, edge, set);
            self.document
                .set_fade_length(clip.id, edge, length, Some("card fade"));
        }
    }

    /// The shape dropdown of a fade, `width` wide. `id` must be unique to the
    /// place that draws it.
    pub(crate) fn clip_fade_shape(
        &mut self,
        ui: &mut Ui,
        clip: &Clip,
        edge: Edge,
        (id, width): (egui::Id, f32),
    ) {
        let fade = match edge {
            Edge::Start => &clip.fade_in,
            Edge::End => &clip.fade_out,
        };
        let now = FadeShape::of(&fade.curve);
        let names: Vec<&str> = FadeShape::ALL.iter().map(|shape| shape.name()).collect();
        let chosen = now.and_then(|now| FadeShape::ALL.iter().position(|s| *s == now));
        let shown = now.map_or("Custom", FadeShape::name);
        let name = format!("{} shape", fade_label(edge));
        if let Some(i) = widgets::Dropdown::new(&name, shown, width).show(ui, id, &names, chosen) {
            self.document
                .set_fade_curve(&[clip.id], edge, FadeShape::ALL[i].curve(), None);
        }
    }

    /// A fade's slider and shape side by side in the room `ui` has: the
    /// shape takes `shape`, the slider the rest.
    pub(crate) fn clip_fade_row(
        &mut self,
        ui: &mut Ui,
        (clip, tempo): (&Clip, &TempoMap),
        edge: Edge,
        (id, shape): (egui::Id, f32),
    ) {
        let width = (ui.available_width() - shape - ui.spacing().item_spacing.x).max(0.0);
        let (rect, _) = ui.allocate_exact_size(
            vec2(width, theme::size::CONTROL_SMALL),
            egui::Sense::hover(),
        );
        self.clip_fade_slider(ui, rect, (clip, tempo), edge, id.with("length"));
        self.clip_fade_shape(ui, clip, edge, (id.with("shape"), shape));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::fade::Fade;
    use lumen_project::model::Id;

    fn clip(start: i64, quarters: i64) -> Clip {
        Clip {
            id: Id(1),
            name: "Kick".into(),
            start: Ticks::quarters(start),
            length: Ticks::quarters(quarters),
            muted: false,
            color: None,
            locked: false,
            crossfade: false,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            takes: None,
            warp: None,
            content: ClipContent::midi(),
        }
    }

    #[test]
    fn fade_lengths_read_and_set_in_milliseconds() {
        // 120 BPM: a quarter is 500 ms.
        let tempo = TempoMap::constant(120.0).expect("a tempo");
        let mut c = clip(4, 8);
        assert_eq!(clip_ms(&tempo, &c), 4000.0);
        c.fade_in.length = fade_ticks(&tempo, &c, Edge::Start, 250.0);
        c.fade_out.length = fade_ticks(&tempo, &c, Edge::End, 1000.0);
        assert_eq!(c.fade_in.length, Ticks(Ticks::quarters(1).0 / 2));
        assert_eq!(c.fade_out.length, Ticks::quarters(2));
        assert!((fade_ms(&tempo, &c, Edge::Start) - 250.0).abs() < 0.01);
        assert!((fade_ms(&tempo, &c, Edge::End) - 1000.0).abs() < 0.01);
        // Never longer than the clip.
        assert_eq!(fade_ticks(&tempo, &c, Edge::End, 1e9), c.length);
    }
}
