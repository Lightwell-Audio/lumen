//! Where a warp marker's ghost would be made (M9.5 S5b): on the transient it
//! sits near, or on the grid line nearest the pointer, unless Alt or the grid
//! turns snapping off. Pure geometry over the editor's view.

use eframe::egui;

use crate::App;
use crate::app::audio_editor::gestures::WaveArea;
use crate::app::audio_editor::range::ticks_of;
use crate::audio_editor::AudioEditor;
use crate::audio_editor::warp_strip::Ghost;

impl App {
    /// Whether a pointer gesture on the strip goes to grid lines now: the
    /// grid snaps and Alt is not held.
    pub(super) fn snaps_here(&self, ctx: &egui::Context) -> bool {
        self.grid.snap && !ctx.input(|i| i.modifiers.alt)
    }

    /// The ghost `g` where it would be made: one that is not on a transient
    /// sits on the grid line nearest the pointer, so a click makes a marker
    /// on the line it is beside. A ghost on a transient stays on its hit.
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
