//! Off-thread seam eligibility and allocation-free per-block seam resolution.

use super::{Clip, ClipSet, FadeFrames};
use crate::RenderContext;

/// How far into a fade a bare start is taken to be: past any fade.
pub(super) const BARE_LEAD: i64 = i64::MAX / 4;

pub(super) fn envelopes_meet(left: &Clip, right: &Clip) -> bool {
    if left.shape.period.is_some() {
        left.envelope == right.envelope
    } else {
        left.envelope.meets(&right.envelope, left.length)
    }
}

pub(super) fn source_meets(left: &Clip, right: &Clip) -> bool {
    match (left.shape.period, right.shape.period) {
        (None, None) => left.offset.checked_add(left.length) == Some(right.offset),
        (Some(a), Some(b)) if a > 0 && a == b && left.offset == right.offset => {
            left.envelope == right.envelope
                && (left.shape.phase + left.length).rem_euclid(a) == right.shape.phase.rem_euclid(a)
                && right.shape.phase.rem_euclid(a) != 0
        }
        _ => false,
    }
}

pub(super) fn fades_meet(left: &Clip, right: &Clip) -> bool {
    fn continuation(a: FadeFrames, b: FadeFrames, advance: i64) -> bool {
        a.frames > 0
            && b.frames > 0
            && a.curve == b.curve
            && a.span.max(a.frames) == b.span.max(b.frames)
            && a.offset.checked_add(advance) == Some(b.offset)
    }
    let left_in = left.shape.fade_in;
    let right_in = right.shape.fade_in;
    let left_out = left.shape.fade_out;
    let right_out = right.shape.fade_out;
    let fade_in = if right_in.frames > 0 {
        continuation(left_in, right_in, left.length)
    } else {
        left_in.frames < left.length
            || left_in.offset.saturating_add(left_in.frames) >= left_in.span.max(left_in.frames)
    };
    let fade_out = if left_out.frames > 0 {
        continuation(right_out, left_out, right.length)
    } else {
        true
    };
    fade_in && fade_out
}

pub(super) fn resolve(set: &mut ClipSet, cx: &RenderContext<'_>) {
    if set.adjacent.is_empty() {
        return;
    }
    set.seams.fill([false; 2]);
    set.seam_lead.fill(0);
    for (i, clip) in set.clips.iter().enumerate() {
        set.seam_root[i] = i;
        set.seam_total[i] = clip.length;
    }
    for &(left, right) in &set.adjacent {
        let a = &set.clips[left];
        let b = &set.clips[right];
        let a_start = cx.timeline.sample_at(a.start, cx.sample_rate);
        let b_start = cx.timeline.sample_at(b.start, cx.sample_rate);
        if a_start.checked_add(a.length) == Some(b_start) {
            set.seams[left][1] = true;
            if a.shape.period.is_none() {
                set.seams[right][0] = true;
                set.seam_lead[right] = set.seam_lead[left].saturating_add(a.length);
                let root = set.seam_root[left];
                set.seam_root[right] = root;
                set.seam_total[root] = set.seam_total[root].saturating_add(b.length);
            }
        }
    }
    for i in 0..set.clips.len() {
        let root = set.seam_root[i];
        set.seam_total[i] = set.seam_total[root];
    }
    set.apply_bare();
}
