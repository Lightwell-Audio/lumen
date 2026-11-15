use eframe::egui;

use crate::App;
use crate::app::audio_editor::gestures::WaveArea;
use crate::app::audio_editor::range::ticks_of;
use crate::audio_editor::AudioEditor;
use crate::audio_editor::warp_strip::Ghost;

impl App {
    pub(super) fn snaps_here(&self, ctx: &egui::Context) -> bool {
        self.grid.snap && !ctx.input(|i| i.modifiers.alt)
    }

    pub(super) fn ghost_on_the_grid(
        &self,
        ctx: &egui::Context,
        g: Ghost,
        (area, left): (&WaveArea<'_>, f32),
        ed: &AudioEditor,
    ) -> Ghost {
        if g.on_transient || !self.snaps_here(ctx) {
            return g;
        }
        let line = self.snapped_at_zoom(ticks_of(ed.time.beat_at(left, g.x)), false, ed.time.zoom);
        let beat = line.0 as f64 / lumen_core::time::PPQ as f64;
        let Some(frame) = area.file.frame_at(beat) else {
            return g;
        };
        Ghost {
            x: ed.time.x(left, beat),
            source: frame / f64::from(area.rate.max(1)),
            ..g
        }
    }
}
