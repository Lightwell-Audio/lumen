//! Mixing one clip into its track's bus: where it sits in the block, the
//! plain path and the shaped one (fades, repeats and envelopes).

use super::Clip;
use super::crossfades::Ramp;
use super::fades::Fades;
use super::pages::Samples;

/// One side of a clip in one block.
pub(super) struct SideMix<'a> {
    pub(super) clip: &'a Clip,
    pub(super) at: Placement<'a>,
    pub(super) span: std::ops::Range<i64>,
    /// The level it starts at and the level it goes to.
    pub(super) from: f32,
    pub(super) goal: f32,
    pub(super) glide: f32,
    pub(super) ramp: Ramp,
    /// Without fades, repeats or envelopes.
    pub(super) plain: bool,
}

impl SideMix<'_> {
    /// Mixes the side into `target`; returns the level it ends at.
    #[inline(always)]
    pub(super) fn run<S: Samples>(&self, target: &mut [f32], samples: S) -> f32 {
        let mix: fn(&mut [f32], S, &Clip, Placement<'_>, std::ops::Range<i64>, f32) =
            if self.plain { mix } else { mix_shaped };
        let (clip, at, ramp, goal) = (self.clip, self.at, self.ramp, self.goal);
        let mut gain = self.from;
        let mut t = self.span.start;
        // Gliding or crossfading: a new gain every frame until both settle.
        while t < self.span.end && (gain != goal || ramp.active(t)) {
            gain = if (goal - gain).abs() <= self.glide {
                goal
            } else {
                gain + self.glide.copysign(goal - gain)
            };
            mix(target, samples, clip, at, t..t + 1, gain * ramp.gain(t));
            t += 1;
        }
        // Steady: the common case, kept as tight as before.
        mix(target, samples, clip, at, t..self.span.end, gain);
        gain
    }
}

/// Where a clip sits in the block being rendered.
#[derive(Clone, Copy)]
pub(super) struct Placement<'a> {
    /// Timeline sample of the clip's first frame.
    pub(super) start: i64,
    /// Timeline sample of the block's first frame.
    pub(super) from: i64,
    /// Edge fade length, in frames.
    pub(super) edge: i64,
    /// The clip's fade tables.
    pub(super) fades: &'a Fades,
    /// The envelope gains for this side, indexed by the frame's place in the
    /// block; none for a clip without an envelope.
    pub(super) env: Option<&'a [f32]>,
    /// Suppress only the artificial 2 ms edge dip at verified split seams.
    pub(super) seamless: [bool; 2],
    /// Position in the first clip's preserved outer edge fade.
    pub(super) seam_lead: i64,
    /// Full contiguous span used to avoid shortening its edge fade.
    pub(super) seam_total: i64,
}

/// Adds `clip`'s audio at `gain` for the timeline samples `frames` into
/// `target`: the plain path, for clips without fades or repeats.
#[inline(always)]
fn mix<S: Samples>(
    target: &mut [f32],
    samples: S,
    clip: &Clip,
    at: Placement<'_>,
    frames: std::ops::Range<i64>,
    gain: f32,
) {
    for t in frames {
        let pos = t - at.start;
        let Some(sample) = usize::try_from(clip.offset + pos)
            .ok()
            .and_then(|i| samples.at(i))
        else {
            continue;
        };
        let edge_cap = if at.seamless[0] || at.seamless[1] {
            at.seam_total
        } else {
            clip.length
        };
        target[(t - at.from) as usize] += sample
            * gain
            * edge_gain(
                pos,
                clip.length,
                edge_cap,
                at.edge,
                at.seamless,
                at.seam_lead,
            );
    }
}

/// [`mix`] for a clip with fades or repeats: each repeat plays the content
/// from the offset again, with its own edge fades.
#[inline(always)]
fn mix_shaped<S: Samples>(
    target: &mut [f32],
    samples: S,
    clip: &Clip,
    at: Placement<'_>,
    frames: std::ops::Range<i64>,
    gain: f32,
) {
    let shape = &clip.shape;
    for t in frames {
        let pos = t - at.start;
        let (inner, span, repeat, looping) = match shape.period {
            Some(period) if period > 0 => {
                let progressed = pos + clip.shape.phase;
                let repeat = progressed.div_euclid(period) * period;
                (
                    progressed - repeat,
                    period.min(clip.length + clip.shape.phase - repeat),
                    repeat,
                    true,
                )
            }
            _ => (pos, clip.length, 0, false),
        };
        let Some(sample) = usize::try_from(clip.offset + inner)
            .ok()
            .and_then(|i| samples.at(i))
        else {
            continue;
        };
        let seamless = if looping {
            [
                false,
                at.seamless[1] && repeat + span >= clip.length + clip.shape.phase,
            ]
        } else {
            at.seamless
        };
        let fade =
            shape.fade_in_gain(at.fades, pos) * shape.fade_out_gain(at.fades, pos, clip.length);
        let shaped = edge_gain(inner, span, span, at.edge, seamless, at.seam_lead) * fade;
        let slot = (t - at.from) as usize;
        let env = at.env.map_or(1.0, |e| e[slot]);
        target[slot] += sample * gain * shaped * env;
    }
}

/// The edge fade for frame `pos` of a clip `length` frames long.
fn edge_gain(
    pos: i64,
    length: i64,
    edge_span: i64,
    edge: i64,
    seamless: [bool; 2],
    seam_lead: i64,
) -> f32 {
    let edge = edge.min(edge_span / 2).max(1);
    let from_start = if seamless[0] {
        (pos + seam_lead + 1).min(edge)
    } else {
        (pos + 1).min(edge)
    };
    let to_end = if seamless[1] {
        edge
    } else {
        (length - pos).min(edge)
    };
    (from_start.min(to_end) as f32 / edge as f32).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_fade_over_the_edge_length() {
        assert_eq!(edge_gain(0, 1_000, 1_000, 10, [false; 2], 0), 0.1);
        assert_eq!(edge_gain(9, 1_000, 1_000, 10, [false; 2], 0), 1.0);
        assert_eq!(edge_gain(500, 1_000, 1_000, 10, [false; 2], 0), 1.0);
        assert_eq!(edge_gain(999, 1_000, 1_000, 10, [false; 2], 0), 0.1);
        // Short clips use half their length.
        assert_eq!(edge_gain(1, 4, 4, 10, [false; 2], 0), 1.0);
        assert_eq!(edge_gain(0, 4, 4, 10, [false; 2], 0), 0.5);
        assert_eq!(edge_gain(0, 1_000, 1_000, 10, [true, false], 10), 1.0);
        assert_eq!(edge_gain(999, 1_000, 1_000, 10, [false, true], 0), 1.0);
        // A split 24 frames into the outer 96-frame fade carries the
        // remaining fade into the second piece instead of snapping to unity.
        assert_eq!(
            edge_gain(0, 1_000, 1_000, 96, [true, false], 24),
            25.0 / 96.0
        );
    }
}
