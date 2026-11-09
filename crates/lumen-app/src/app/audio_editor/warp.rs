//! The Warp chip of the audio editor's strip (M9.5 S5a) and the detection that
//! feeds it. The chip sits beside the File chip and opens a popover: the warp
//! switch, the mode, the tempo the clip's file was made at (typed, doubled or
//! halved, or taken from what detection found), the sensitivity of the
//! transients shown on the wave, Set project tempo from clip and Warp from
//! here. Every control edits the opened clip, as Gain does, and is one undo
//! step; the chip is a Tab stop with an accessible name, opened by a click,
//! Enter or Space and closed by Esc.
//!
//! Detection (`audio_editor::detect`) starts when the opened clip's file is
//! ready, runs on a worker and is kept in the cache; this module starts it,
//! takes its answer each frame, and says what it found.

use eframe::egui::{self, FontId, Rect, RichText, Sense, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_media::detect::{Analysis, CONFIDENT};
use lumen_project::edit::warp::Change;
use lumen_project::model::{Clip, ClipContent, WarpMode};

use super::strip_chips::{CHEVRON, text_width};
use crate::app::clip_card::{CARD_WIDTH, row as card_row};
use crate::app::clip_file::ClipFile;
use crate::app::clip_warp::Found;
use crate::audio_editor::AudioEditor;
use crate::audio_editor::detect::{State, file_key, in_view};
use crate::audio_editor::paint;
use crate::audio_editor::source::WholeFile;
use crate::commands::Command;
use crate::timeview::TimeView;
use crate::widgets::{self, Dropdown, Menu, SliderKind, raised};
use crate::{App, theme};

/// The words for a warp mode in the dropdown.
pub(crate) fn mode_name(mode: WarpMode) -> &'static str {
    match mode {
        WarpMode::Repitch => "Repitch",
        WarpMode::Rhythmic => "Rhythmic",
        WarpMode::Melodic => "Melodic",
        WarpMode::FullMix => "Full mix",
    }
}

/// What the Warp chip says: "Warp" when off, "Warp 93.0" when on.
pub(super) fn chip_text(clip: &Clip) -> String {
    match &clip.warp {
        Some(warp) => format!("Warp {:.1}", warp.original_bpm),
        None => "Warp".to_owned(),
    }
}

/// How wide the Warp chip is for `text`. The text's width is rounded up to
/// a whole point and given one more, so a chip given exactly this width
/// never cuts its label to "Wa…" for a fraction of a pixel.
pub(super) fn chip_width(ui: &Ui, text: &str) -> f32 {
    let s = theme::space::S;
    let text = text_width(ui, text, FontId::proportional(theme::size::BODY)).ceil() + 1.0;
    s + text + s + CHEVRON + s
}

/// The tempo `analysis` found, in words.
fn detected_words(analysis: &Analysis) -> Option<String> {
    let tempo = analysis.tempo?;
    Some(if tempo.confidence >= CONFIDENT {
        format!("Detected {:.1} BPM", tempo.bpm)
    } else {
        format!("Detected about {:.1} BPM (not sure)", tempo.bpm)
    })
}

impl App {
    /// Starts detection for the opened clip's file when it has not been read
    /// yet, takes the worker's answer, and keeps the screen coming while it
    /// works. Called once a frame before the strip is drawn.
    pub(super) fn drive_detection(
        &mut self,
        ctx: &egui::Context,
        clip: &Clip,
        ed: &mut AudioEditor,
    ) {
        let (ClipContent::Audio { media, .. }, Some(key)) = (&clip.content, file_key(clip)) else {
            return;
        };
        if ed.detect.poll() {
            ctx.request_repaint();
        }
        if self.clip_file_state(*media) == ClipFile::Ready
            && let Some(item) = self.lanes.library.get(media.0)
            && ed.detect.wants(key, &item.path)
        {
            ed.detect.start(
                key,
                (item.path.clone(), self.lanes.rate()),
                self.cache_dir(),
                ctx,
            );
        }
        if ed.detect.working(key).is_some() {
            ctx.request_repaint_after(crate::motion::PROGRESS_FRAME);
        }
    }

    /// What detection found for `clip`'s file, with the sensitivity.
    fn editor_found(ed: &AudioEditor, clip: &Clip) -> Found {
        let key = file_key(clip)?;
        ed.detect
            .analysis(key)
            .map(|a| (key, std::sync::Arc::clone(a), ed.detect.sensitivity))
    }

    /// Faint ticks on the wave where detection found the sound starting,
    /// more of them at a higher sensitivity. Only the ticks in view are
    /// looked at.
    pub(super) fn paint_transients(
        painter: &egui::Painter,
        body: Rect,
        file: &WholeFile,
        (time, rate): (TimeView, u32),
        (analysis, sensitivity): (&Analysis, u8),
    ) {
        let Some((a, b)) = paint::visible_frames(file, time, body) else {
            return;
        };
        let rate = f64::from(rate);
        let accent = theme::p().accent;
        let inside = painter.with_clip_rect(body.intersect(painter.clip_rect()));
        for t in in_view(analysis, sensitivity, (a as f64 / rate, b as f64 / rate)) {
            let Some(beat) = file.beat_of_frame(t.seconds * rate) else {
                continue;
            };
            let x = time.x(body.left(), beat).round() + 0.5;
            let ink = accent.gamma_multiply(0.25 + 0.5 * t.strength.clamp(0.0, 1.0));
            inside.vline(
                x,
                body.top()..=body.bottom(),
                egui::Stroke::new(theme::stroke::HAIRLINE, ink),
            );
        }
    }

    /// The Warp chip in `rect`: a Tab stop that opens the popover. True when
    /// it has the keyboard.
    pub(super) fn warp_chip(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        clip: &Clip,
        ed: &mut AudioEditor,
    ) -> bool {
        let palette = *theme::p();
        let warped = clip.warp.is_some();
        let text = chip_text(clip);
        let response = ui.interact(
            rect,
            ui.id().with(("audio strip warp", clip.id.0)),
            Sense::click(),
        );
        response.widget_info(|| {
            let state = match &clip.warp {
                Some(warp) => format!(
                    "Warp: on, {} at {:.1} BPM",
                    mode_name(warp.mode),
                    warp.original_bpm
                ),
                None => "Warp: off".to_owned(),
            };
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, state)
        });
        if ui.is_rect_visible(rect) {
            let face = raised::face_of(ui, &response);
            let rect = rect.round_to_pixels(ui.pixels_per_point());
            let body = raised::paint(
                ui.painter(),
                rect,
                theme::radius::for_height(rect.height()),
                &face,
            );
            let mid = body.center().y.round();
            let painter = ui.painter();
            let ink = if warped { palette.accent } else { face.text() };
            let s = theme::space::S;
            let room = (body.width() - 3.0 * s - CHEVRON).max(0.0);
            let font = FontId::proportional(theme::size::BODY);
            let shown = widgets::fit(painter, &text, font.clone(), room);
            let galley = painter.layout_no_wrap(shown, font, ink);
            painter.galley(
                pos2(
                    (body.left() + s).round(),
                    mid - (galley.size().y / 2.0).round(),
                ),
                galley,
                ink,
            );
            super::strip_chips::chevron(
                painter,
                pos2((body.right() - s - CHEVRON / 2.0).round(), mid),
                palette.text_hint,
            );
            raised::focus_ring(painter, &response, rect);
        }
        let response = widgets::tip(
            response,
            "Warp",
            None,
            Some("Make this clip follow the song's tempo, and find its tempo and transients"),
        );
        Menu::below(&response).staying().show(|ui| {
            ui.set_width(CARD_WIDTH);
            self.warp_popover(ui, clip, ed);
        });
        response.has_focus()
    }

    /// What the Warp chip opens.
    pub(super) fn warp_popover(&mut self, ui: &mut Ui, clip: &Clip, ed: &mut AudioEditor) {
        let palette = *theme::p();
        let ctx = ui.ctx().clone();
        let found = Self::editor_found(ed, clip);
        let fixed = self.strip_fixed(clip.id);
        let editable = fixed.why().is_none();
        let warp = clip.warp.clone();
        let base = ui.id().with(("warp popover", clip.id.0));

        let mut on = warp.is_some();
        let switch = ui
            .add_enabled_ui(editable, |ui| {
                widgets::switch(ui, &mut on, "Warp clip", editable)
            })
            .inner;
        let keys = self.keymap.keys(Command::WarpClips);
        let switch = widgets::tip(
            switch,
            "Warp clip",
            keys.as_ref(),
            Some("The clip follows the song's tempo instead of playing at its own speed"),
        );
        if let Some(why) = fixed.why() {
            switch.on_disabled_hover_text(why);
        } else if switch.changed() {
            self.select_opened_clip(clip.id);
            self.toggle_clip_warp_with(&found);
        }

        ui.add_space(theme::space::XS);
        let warped = editable && warp.is_some();
        ui.add_enabled_ui(warped, |ui| {
            card_row(ui, "Mode", |ui| {
                let mode = warp.as_ref().map_or(WarpMode::Repitch, |w| w.mode);
                let chosen = WarpMode::ALL.iter().position(|m| *m == mode);
                let names: Vec<&str> = WarpMode::ALL.iter().map(|m| mode_name(*m)).collect();
                if let Some(i) = Dropdown::new("Warp mode", mode_name(mode), theme::size::WARP_MODE)
                    .show(ui, base.with("mode"), &names, chosen)
                {
                    self.document
                        .set_warp(&[clip.id], &Change::Mode(WarpMode::ALL[i]));
                }
            });
            card_row(ui, "Tempo", |ui| {
                let bpm = warp.as_ref().map_or(120.0, |w| w.original_bpm) as f32;
                let (rect, _) = ui.allocate_exact_size(
                    vec2(theme::size::WARP_FIELD, theme::size::CONTROL_SMALL),
                    Sense::hover(),
                );
                let next = widgets::value_field(
                    ui,
                    rect,
                    base.with("tempo"),
                    bpm,
                    SliderKind::Bpm,
                    "Clip tempo",
                    theme::controls(false),
                );
                if (next - bpm).abs() > f32::EPSILON {
                    self.set_clip_tempo(clip.id, f64::from(next));
                }
                for (label, factor, tip) in [
                    (
                        "\u{00d7}2",
                        2.0,
                        "Double the tempo: the file was made at twice this",
                    ),
                    (
                        "\u{00f7}2",
                        0.5,
                        "Halve the tempo: the file was made at half this",
                    ),
                ] {
                    let button = raised::button(ui, label);
                    if widgets::tip(button, tip, None, None).clicked() {
                        self.select_opened_clip(clip.id);
                        self.scale_clip_tempo(factor);
                    }
                }
            });
        });
        if matches!(warp.as_ref().map(|w| w.mode), Some(WarpMode::Rhythmic)) {
            widgets::hint(ui, "Rhythmic plays as Repitch until its slicing arrives");
        }
        if warped && !self.settings.playback.from_disk {
            // A warped clip is played from a stream the disk thread prepares,
            // so with "Play audio from disk" off the wave is drawn warped and
            // the clip is heard as it was recorded.
            widgets::hint(
                ui,
                "Play audio from disk is off, so this clip is heard at its own speed. \
                 Warped clips need it on.",
            );
            if raised::button(ui, "Turn on Play audio from disk").clicked() {
                self.settings.playback.from_disk = true;
                self.settings_changed = Some(std::time::Instant::now());
            }
        }

        ui.separator();
        self.detection_rows(ui, clip, ed, (&found, warped), base);

        ui.separator();
        ui.horizontal(|ui| {
            let set = ui.add_enabled_ui(warped, |ui| {
                raised::button(ui, "Set project tempo from clip")
            });
            let set = widgets::tip(
                set.inner,
                "Set project tempo from clip",
                self.keymap.keys(Command::SetTempoFromClip).as_ref(),
                Some("The song takes the tempo this clip's file was made at"),
            );
            if !warped {
                set.clone().on_disabled_hover_text("Warp the clip first");
            }
            if set.clicked() && warped {
                self.select_opened_clip(clip.id);
                self.set_tempo_from_clip();
            }
        });
        ui.horizontal(|ui| {
            let here = ui.add_enabled_ui(editable, |ui| raised::button(ui, "Warp from here"));
            let here = widgets::tip(
                here.inner,
                "Warp from here",
                self.keymap.keys(Command::WarpFromHere).as_ref(),
                Some(
                    "The sound at the edit cursor (else the playhead) becomes the nearest bar line",
                ),
            );
            if let Some(why) = fixed.why() {
                here.clone().on_disabled_hover_text(why);
            }
            if here.clicked() && editable {
                self.select_opened_clip(clip.id);
                self.warp_from_here(clip, ed.cursor, &found);
                ctx.request_repaint();
            }
        });
        if let Some(why) = fixed.why() {
            ui.label(RichText::new(why).small().color(palette.text_hint));
        }
    }

    /// The popover's detection rows: what was found, and the sensitivity.
    fn detection_rows(
        &mut self,
        ui: &mut Ui,
        clip: &Clip,
        ed: &mut AudioEditor,
        (found, warped): (&Found, bool),
        base: egui::Id,
    ) {
        let palette = *theme::p();
        let Some(key) = file_key(clip) else {
            return;
        };
        if let Some(job) = ed.detect.working(key) {
            let percent = (job.fraction() * 100.0).round() as u32;
            ui.label(
                RichText::new(format!("Listening to the file\u{2026} {percent}%"))
                    .color(palette.text_secondary),
            );
            return;
        }
        if let State::Failed(why) = &ed.detect.state
            && ed.detect.media == Some(key)
        {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("Could not listen to the file: {why}"))
                        .color(palette.warning),
                )
                .wrap(),
            );
            if raised::button(ui, "Try again").clicked() {
                ed.detect.clear();
            }
            return;
        }
        let Some((_, analysis, sensitivity)) = found else {
            ui.label(RichText::new("Waiting for the file").color(palette.text_secondary));
            return;
        };
        match (detected_words(analysis), analysis.tempo) {
            (Some(words), Some(tempo)) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(words).color(palette.text_secondary));
                    let differs = clip
                        .warp
                        .as_ref()
                        .is_some_and(|w| (w.original_bpm - tempo.bpm).abs() > 0.005);
                    let use_it =
                        ui.add_enabled_ui(warped && differs, |ui| raised::button(ui, "Use"));
                    let use_it = widgets::tip(
                        use_it.inner,
                        "Use the detected tempo",
                        None,
                        Some("Set the tempo this clip's file was made at to what was found"),
                    );
                    if use_it.clicked() && warped && differs {
                        self.set_clip_tempo(clip.id, tempo.bpm);
                    }
                });
            }
            _ => {
                ui.label(RichText::new("No clear tempo found").color(palette.text_secondary));
            }
        }
        card_row(ui, "Sensitivity", |ui| {
            let (rect, _) = ui.allocate_exact_size(
                vec2(theme::size::WARP_SLIDER, theme::size::CONTROL_SMALL),
                Sense::hover(),
            );
            let was = f32::from(*sensitivity) / 100.0;
            let next = widgets::value_slider(
                ui,
                rect,
                base.with("sensitivity"),
                was,
                SliderKind::Percent { default: 0.5 },
                "Transient sensitivity",
                theme::controls(false),
            );
            if (next - was).abs() > f32::EPSILON {
                ed.detect.sensitivity = (next * 100.0).round().clamp(0.0, 100.0) as u8;
            }
        });
        let count = analysis.transients_at(ed.detect.sensitivity).count();
        widgets::hint(
            ui,
            &match count {
                0 => "No transients at this sensitivity".to_owned(),
                1 => "1 transient shown on the wave".to_owned(),
                n => format!("{n} transients shown on the wave"),
            },
        );
    }
}
