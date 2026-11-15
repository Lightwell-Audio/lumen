use super::Clip;
use super::crossfades::Ramp;
use super::fades::Fades;
use super::pages::Samples;

pub(super) struct SideMix<'a> {
    pub(super) clip: &'a Clip,
    pub(super) at: Placement<'a>,
    pub(super) span: std::ops::Range<i64>,
    pub(super) from: f32,
    pub(super) goal: f32,
    pub(super) glide: f32,
    pub(super) ramp: Ramp,
    pub(super) plain: bool,
}

impl SideMix<'_> {
    #[inline(always)]
    pub(super) fn run<S: Samples>(&self, target: &mut [f32], samples: S) -> f32 {
        let mix: fn(&mut [f32], S, &Clip, Placement<'_>, std::ops::Range<i64>, f32) =
            if self.plain { mix } else { mix_shaped };
        let (clip, at, ramp, goal) = (self.clip, self.at, self.ramp, self.goal);
        let mut gain = self.from;
        let mut t = self.span.start;
        while t < self.span.end && (gain != goal || ramp.active(t)) {
            gain = if (goal - gain).abs() <= self.glide {
                goal
            } else {
                gain + self.glide.copysign(goal - gain)
            };
            mix(target, samples, clip, at, t..t + 1, gain * ramp.gain(t));
            t += 1;
        }
        mix(target, samples, clip, at, t..self.span.end, gain);
        gain
    }
}

#[derive(Clone, Copy)]
pub(super) struct Placement<'a> {
    pub(super) start: i64,
    pub(super) from: i64,
    pub(super) edge: i64,
    pub(super) fades: &'a Fades,
    pub(super) env: Option<&'a [f32]>,
    pub(super) seamless: [bool; 2],
    pub(super) seam_lead: i64,
    pub(super) seam_total: i64,
}

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
        assert_eq!(edge_gain(1, 4, 4, 10, [false; 2], 0), 1.0);
        assert_eq!(edge_gain(0, 4, 4, 10, [false; 2], 0), 0.5);
        assert_eq!(edge_gain(0, 1_000, 1_000, 10, [true, false], 10), 1.0);
        assert_eq!(edge_gain(999, 1_000, 1_000, 10, [false, true], 0), 1.0);
        assert_eq!(
            edge_gain(0, 1_000, 1_000, 96, [true, false], 24),
            25.0 / 96.0
        );
    }
}
