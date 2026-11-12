//! A clip's fades and repeats, and the fade tables the audio thread reads:
//! one table per distinct curve, built on the UI side.

use std::sync::Arc;

use lumen_core::curve::Curve;

/// Fade curves are read from tables of this many steps: within about
/// 10⁻⁶ of the curve (far below hearing) for all but its steepest
/// first step.
pub(super) const TABLE_STEPS: usize = 1_024;
/// The table cache starts over past this many curves.
pub(super) const MAX_TABLES: usize = 256;

/// A fade curve's levels at `TABLE_STEPS + 1` evenly spaced points.
pub(super) type Table = Arc<[f32]>;

/// A clip's fades and repeats, in frames at the device rate.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClipShape {
    pub fade_in: FadeFrames,
    pub fade_out: FadeFrames,
    /// The content repeats every `period` frames from the clip's start.
    pub period: Option<i64>,
    /// Position within the repeated source when this clip begins.
    pub phase: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FadeFrames {
    pub frames: i64,
    /// Position within the fade's original curve window.
    pub offset: i64,
    /// Original fade curve window in frames; zero keeps legacy `frames` behavior.
    pub span: i64,
    /// From silence to full; a fade out plays it backwards.
    pub curve: Curve,
    /// This fade is one half of a crossfade with an overlapping clip: it
    /// follows an equal-power law (the other half is its mirror, so the two
    /// keep a steady level) and the clip's 2 ms edge fade is left out at this
    /// end, as the fade already covers it.
    pub blend: bool,
}

/// A clip's fade in and fade out tables, when it has fades.
#[derive(Clone, Default)]
pub(super) struct Fades(pub(super) [Option<Table>; 2]);

/// The level `x` (0 to 1) of the way through `table`.
#[inline]
pub(super) fn read(table: &[f32], x: f32) -> f32 {
    let p = x.clamp(0.0, 1.0) * TABLE_STEPS as f32;
    let i = (p as usize).min(TABLE_STEPS - 1);
    match (table.get(i), table.get(i + 1)) {
        (Some(&a), Some(&b)) => a + (b - a) * (p - i as f32),
        _ => 1.0,
    }
}

impl ClipShape {
    pub(super) fn plain(&self) -> bool {
        self.fade_in.frames <= 0 && self.fade_out.frames <= 0 && self.period.is_none()
    }

    #[inline]
    pub(super) fn fade_in_gain(&self, fades: &Fades, pos: i64) -> f32 {
        let fade_in = self.fade_in.frames;
        if pos < fade_in
            && let Some(table) = &fades.0[0]
        {
            let span = self.fade_in.span.max(fade_in);
            let level = read(
                table,
                (pos as f32 + self.fade_in.offset as f32 + 0.5) / span as f32,
            );
            return if self.fade_in.blend {
                (level * std::f32::consts::FRAC_PI_2).sin()
            } else {
                level
            };
        }
        1.0
    }

    #[inline]
    pub(super) fn fade_out_gain(&self, fades: &Fades, pos: i64, length: i64) -> f32 {
        let fade_out = self.fade_out.frames;
        let left = length - pos;
        if left <= fade_out
            && let Some(table) = &fades.0[1]
        {
            let span = self.fade_out.span.max(fade_out);
            let x = (left as f32 + self.fade_out.offset as f32 - 0.5) / span as f32;
            return if self.fade_out.blend {
                // The mirror of the fade-in: its curve read forwards.
                (read(table, 1.0 - x) * std::f32::consts::FRAC_PI_2).cos()
            } else {
                read(table, x)
            };
        }
        1.0
    }
}
