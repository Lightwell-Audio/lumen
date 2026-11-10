//! Rhythmic warp: the file cut at its transients, each slice placed where the
//! warp map puts it (M9.5 S5b, the owner's M = A).
//!
//! A drum hit stretched by a general-purpose engine smears. Here nothing is
//! stretched: the file is cut into slices that each begin at a transient (or
//! at a warp marker), and each slice **starts where the map plays its first
//! frame and then plays at the file's own speed and pitch**. Where the song
//! runs faster than the file, the next slice arrives before this one is over
//! and ends it (the end fades out over [`FADE`] frames, under the new attack);
//! where it runs slower, the slice ends and a short silence follows its fade.
//! Attacks are never touched.
//!
//! Like [`crate::repitch`], **every output frame depends only on the map and
//! the file**, never on what was rendered before, so a run in one piece, in
//! pieces of any length or after a jump holds the same bits, and playback and
//! export agree. The map must not run backwards.
//!
//! A span at the file's own speed is a copy of the file (slices follow each
//! other without a seam), as D3 asks.

use crate::MediaError;
use crate::detect::Detector;
use crate::repitch::Source;

/// Frames over which the end of a cut-off or early-ending slice fades out.
pub const FADE: i64 = 192;

/// The least distance between a transient and the start beside it, in frames:
/// a closer transient is not a slice of its own. A warp marker is a start
/// whatever its distance.
pub const MIN_SLICE: i64 = 1_024;

/// How near a whole frame the position must come to have reached it. Positions
/// are worked out in floating point (a tempo map, a sum of spans), so a frame
/// the map plays exactly can come out a few parts in 10^13 short; without the
/// tolerance a slice would start a frame late and a span at the file's speed
/// would not be a copy.
const REACHED: f64 = 1.0e-6;

/// The sensitivity (0 to 100) the transients are read at: the middle.
pub const SENSITIVITY: u8 = 50;

/// Frames of the file read at a time while looking for transients.
const SCAN_BLOCK: usize = 16_384;

/// How far, in frames, the search for where a slice starts may go.
const REACH: i64 = 1 << 40;

/// The slice starts of a file: frame 0, each of `pinned` (the places warp
/// markers pin; every one is a start), and each of `transients` that is at
/// least [`MIN_SLICE`] from the start beside it, at `rate`, in order and
/// inside a file of `frames`.
pub fn starts(
    transients: impl IntoIterator<Item = f64>,
    pinned: impl IntoIterator<Item = f64>,
    rate: u32,
    frames: u64,
) -> Vec<i64> {
    let frame = |s: f64| (s * f64::from(rate)).round() as i64;
    let inside = |f: &i64| *f > 0 && (*f as u64) < frames;
    let mut out: std::collections::BTreeSet<i64> = pinned
        .into_iter()
        .filter(|s| s.is_finite())
        .map(frame)
        .filter(inside)
        .collect();
    out.insert(0);
    let mut found: Vec<i64> = transients
        .into_iter()
        .filter(|s| s.is_finite())
        .map(frame)
        .filter(inside)
        .collect();
    found.sort_unstable();
    for f in found {
        let before = out.range(..=f).next_back().copied();
        let after = out.range(f..).next().copied();
        if before.is_none_or(|b| f - b >= MIN_SLICE) && after.is_none_or(|a| a - f >= MIN_SLICE) {
            out.insert(f);
        }
    }
    out.into_iter().collect()
}

/// The transients of a whole file, in seconds, in order: [`crate::detect`]
/// over it at [`SENSITIVITY`]. Each channel is read on its own and the hits
/// joined (a stereo pair is not averaged first: a pair of opposite polarity
/// would cancel to nothing). Slow: it reads the file once.
pub fn scan(source: &mut dyn Source, rate: u32) -> Result<Vec<f64>, MediaError> {
    let channels = source.channels().clamp(1, 2);
    let frames = source.frames();
    let mut detectors: Vec<Detector> = (0..channels).map(|_| Detector::new(rate)).collect();
    let mut block = [vec![0.0_f32; SCAN_BLOCK], vec![0.0_f32; SCAN_BLOCK]];
    let mut at = 0_u64;
    while at < frames {
        let n = ((frames - at) as usize).min(SCAN_BLOCK);
        let [left, right] = &mut block;
        if channels >= 2 {
            source.read(at, &mut [&mut left[..n], &mut right[..n]])?;
        } else {
            source.read(at, &mut [&mut left[..n]])?;
        }
        for (detector, samples) in detectors.iter_mut().zip([&*left, &*right]) {
            detector.add(&samples[..n], None);
        }
        at += n as u64;
    }
    let mut hits: Vec<f64> = detectors
        .into_iter()
        .flat_map(|d| {
            d.finish()
                .transients_at(SENSITIVITY)
                .map(|t| t.seconds)
                .collect::<Vec<_>>()
        })
        .collect();
    hits.sort_by(f64::total_cmp);
    Ok(hits)
}

/// Renders runs of output frames. Holds the scratch a run needs, so make one
/// per worker.
#[derive(Default)]
pub struct Slicer {
    window: [Vec<f32>; 2],
}

/// The part of the song one slice plays.
struct Slice {
    /// First frame of the file.
    from: i64,
    /// Frames of the file it holds.
    len: i64,
    /// The song frame its first file frame plays at.
    at: i64,
    /// The song frame the next slice plays at; `i64::MAX` for the last.
    next: i64,
}

impl Slicer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Renders output frames `first..first + left.len()` into `left` and, for
    /// a file of two channels, `right` (both one length; a mono file leaves
    /// `right` alone). `position(k)` is the frame of the file, fractional,
    /// that plays at output frame `k`: it must not run backwards and must
    /// depend on nothing but `k`. `starts` are the slices' first frames
    /// ([`starts`]).
    pub fn render(
        &mut self,
        source: &mut dyn Source,
        (position, starts): (&dyn Fn(i64) -> f64, &[i64]),
        first: i64,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<(), MediaError> {
        let n = left.len();
        let stereo = source.channels() >= 2;
        left.fill(0.0);
        if stereo {
            right[..n].fill(0.0);
        }
        let frames = source.frames() as i64;
        let at_first = position(first);
        if n == 0 || starts.is_empty() || frames == 0 || !at_first.is_finite() {
            return Ok(());
        }
        let Some(end) = first.checked_add(n as i64) else {
            return Ok(());
        };
        let slice = |j: usize| {
            let from = starts[j];
            let stop = starts.get(j + 1).copied().unwrap_or(frames);
            Slice {
                from,
                len: stop - from,
                at: where_it_starts(position, from, first),
                next: starts
                    .get(j + 1)
                    .map_or(i64::MAX, |&s| where_it_starts(position, s, first)),
            }
        };
        // The slice playing at `first`, and back from it every slice whose
        // fade can still be sounding: one cut off by a slice that arrived
        // less than [`FADE`] frames before `first`. At a speed of many times
        // the file's that is more than the one before.
        let here = starts.partition_point(|&s| s as f64 <= at_first + REACHED);
        let mut j = here.saturating_sub(1);
        while j > 0 && where_it_starts(position, starts[j], first).saturating_add(FADE) > first {
            j -= 1;
        }
        while j < starts.len() {
            let s = slice(j);
            if s.at >= end {
                break;
            }
            self.play(source, &s, frames, (first, end), (left, right), stereo)?;
            j += 1;
        }
        Ok(())
    }

    /// Adds slice `s`, and the fade of its end, to the run.
    fn play(
        &mut self,
        source: &mut dyn Source,
        s: &Slice,
        frames: i64,
        (first, end): (i64, i64),
        (left, right): (&mut [f32], &mut [f32]),
        stereo: bool,
    ) -> Result<(), MediaError> {
        // The slice's own frames that play: all of them, unless the next
        // slice arrives first (then the rest fades under it).
        let room = s.next.saturating_sub(s.at);
        let cut = room < s.len;
        let gap = !cut && s.next != i64::MAX && room > s.len;
        let sounding = if cut { room + FADE } else { s.len };
        let fade_from = if cut {
            room
        } else if gap {
            (s.len - FADE).max(0)
        } else {
            i64::MAX
        };
        let fade_len = if cut {
            FADE
        } else {
            s.len - fade_from.min(s.len)
        };
        // The run's part of it, as offsets into the slice.
        let o0 = first.saturating_sub(s.at).max(0);
        let o1 = end.saturating_sub(s.at).min(sounding).min(frames - s.from);
        if o0 >= o1 {
            return Ok(());
        }
        let count = (o1 - o0) as usize;
        let out0 = (s.at + o0 - first) as usize;
        let [a, b] = &mut self.window;
        a.clear();
        a.resize(count, 0.0);
        b.clear();
        b.resize(count, 0.0);
        if stereo {
            source.read((s.from + o0) as u64, &mut [&mut a[..], &mut b[..]])?;
        } else {
            source.read((s.from + o0) as u64, &mut [&mut a[..]])?;
        }
        for i in 0..count {
            let o = o0 + i as i64;
            let gain = if o >= fade_from {
                let m = o - fade_from;
                1.0 - (m + 1) as f32 / (fade_len + 1) as f32
            } else {
                1.0
            };
            left[out0 + i] += a[i] * gain;
            if stereo {
                right[out0 + i] += b[i] * gain;
            }
        }
        Ok(())
    }
}

/// The first output frame at which `position` has reached file frame `from`,
/// searched out from `hint`. The same frame whatever the hint, since the
/// position runs forwards by at least a quarter of a frame per frame.
fn where_it_starts(position: &dyn Fn(i64) -> f64, from: i64, hint: i64) -> i64 {
    let target = from as f64;
    let reached = |k: i64| position(k) + REACHED >= target;
    let (mut lo, mut hi);
    if reached(hint) {
        hi = hint;
        let mut step = 1;
        loop {
            let k = hint.saturating_sub(step);
            if !reached(k) {
                lo = k;
                break;
            }
            hi = k;
            if step >= REACH {
                return hi;
            }
            step *= 2;
        }
    } else {
        lo = hint;
        let mut step = 1;
        loop {
            let k = hint.saturating_add(step);
            if reached(k) {
                hi = k;
                break;
            }
            lo = k;
            if step >= REACH {
                return i64::MAX;
            }
            step *= 2;
        }
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if reached(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

#[cfg(test)]
mod tests;
