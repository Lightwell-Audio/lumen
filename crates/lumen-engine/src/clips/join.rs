//! Rendering of the source tail for a clip join crossfade.

use super::{BUS_CHANNELS, Buses, Clip, GLIDE_PER_SECOND, Ramp, Work, read};
use crate::RenderContext;

pub(super) struct JoinTail<'a> {
    pub clip: &'a Clip,
    pub level: &'a mut f32,
    pub source_anchor: i64,
    pub frames: i64,
    pub curve: Option<&'a [f32]>,
    pub start: i64,
    pub ramp: Ramp,
}

/// Adds the predecessor's available tail under the incoming clip.
pub(super) fn play_tail(
    tail: JoinTail<'_>,
    cx: &RenderContext<'_>,
    work: &mut Work<'_>,
    buses: &mut Buses<'_>,
) {
    let JoinTail {
        clip,
        level: kept,
        source_anchor,
        frames,
        curve,
        start,
        ramp,
    } = tail;
    if frames <= 0 || clip.audio.sample_rate != cx.sample_rate || clip.length <= 0 {
        return;
    }
    let tail_start = start + clip.length;
    let a = work.block.from.max(tail_start);
    let b = work.block.to.min(tail_start + frames);
    if a >= b {
        return;
    }
    let Some(mut bus) = buses.track(clip.track) else {
        return;
    };
    if clip.audio.channels.is_empty() {
        return;
    }
    let mut scratch = *kept;
    let level = if cx.tail { &mut scratch } else { kept };
    if level.is_nan() {
        *level = clip.gain;
    }
    let enveloped = !clip.envelope.is_empty();
    if enveloped {
        work.gains.fill(
            &clip.envelope,
            (
                start - clip.shape.phase,
                clip.shape.period.filter(|p| *p > 0),
            ),
            (work.block.from, a, b),
            cx.sample_rate,
        );
    }
    let glide = GLIDE_PER_SECOND / cx.sample_rate as f32;
    for t in a..b {
        *level = if (clip.gain - *level).abs() <= glide {
            clip.gain
        } else {
            *level + glide.copysign(clip.gain - *level)
        };
        let tail_pos = t - tail_start;
        for side in 0..BUS_CHANNELS {
            let samples = &clip.audio.channels[side.min(clip.audio.channels.len() - 1)];
            let Some(&sample) = usize::try_from(clip.offset + source_anchor + tail_pos)
                .ok()
                .and_then(|i| samples.get(i))
            else {
                continue;
            };
            let progress = ((tail_pos as f32 + 0.5) / frames as f32).clamp(0.0, 1.0);
            let bend = curve.map_or(progress, |table| read(table, progress));
            let envelope = if enveloped {
                work.gains.side(side)[(t - work.block.from) as usize]
            } else {
                1.0
            };
            bus.channel(side)[(t - work.block.from) as usize] += sample
                * *level
                * envelope
                * ramp.gain(t)
                * (bend * std::f32::consts::FRAC_PI_2).cos();
        }
    }
}
