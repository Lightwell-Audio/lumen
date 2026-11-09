//! Transient and tempo detection (M9.5 S5a): where an audio file's sounds
//! start, and the tempo it was most likely played at.
//!
//! The audio is fed in blocks ([`Detector::add`]), so a file is never held
//! whole; the work is for a background thread. Three steps:
//!
//! 1. **Rise.** The file is cut into hops of 5 ms. For each, the level of
//!    the first difference of the signal (which favours the sharp start of a
//!    sound over a held note) in dB. A hop's *rise* is how far its level
//!    stands above the two hops before it.
//! 2. **Transients.** A hop whose rise is the largest within 30 ms either way
//!    (and at least [`FLOOR`] dB) is a transient. Its place is found to the
//!    sample inside the hop, and its *strength* is its rise in dB over
//!    [`SPAN`], from 0 to 1. Every one is kept: [`Analysis::transients_at`]
//!    chooses by a sensitivity afterwards, so a change of sensitivity never
//!    asks for a new scan (D5).
//! 3. **Tempo.** The autocorrelation of the rise, summed with its multiples
//!    (a beat repeats at two and four beats too), peaks at the beat's period.
//!    When the file lasts a whole number of beats at that tempo, the tempo is
//!    set to the one that makes it exact, since loops are cut to bars.
//!
//! Tempo detection is wrong by a factor of two now and then; the editor shows
//! the tempo and offers ×2 and ÷2 (plan §6, risk 6).

use std::sync::atomic::{AtomicBool, Ordering};

use crate::MediaError;

/// The shortest hop, in seconds, a transient's place is found within.
const HOP_SECONDS: f64 = 0.005;
/// Hops either side of a transient in which no other has a larger rise: 30 ms.
const SPACING: usize = 6;
/// The smallest rise, in dB, that is a transient at all.
pub const FLOOR: f32 = 3.0;
/// The rise, in dB, that is strength 1.
pub const SPAN: f32 = 30.0;
/// The level, in dB, of silence.
const SILENCE_DB: f32 = -90.0;
/// A transient is sharper than this many dB to count towards a tempo.
const COUNTED_DB: f32 = 6.0;
/// Transients a file needs before a tempo is guessed from them.
const MOST_NEEDED: usize = 4;
/// The slowest and fastest tempo detection looks for.
const SLOWEST: f64 = 60.0;
const FASTEST: f64 = 200.0;
/// The seconds of the file the tempo is worked out from.
const TEMPO_SECONDS: f64 = 120.0;
/// A tempo guess is kept above this confidence.
const WEAKEST: f32 = 0.15;
/// A tempo guess at least this confident is one to act on without asking
/// (a recorded take or a loop that lasts whole bars).
pub const CONFIDENT: f32 = 0.4;
/// How far, as a fraction, the tempo may be moved to make the file last a
/// whole number of beats.
const SNAP: f64 = 0.012;
/// Beats in a bar, for the "whole bars" test of a loop.
const BEATS_PER_BAR: u32 = 4;
/// The most bars a loop is taken to be.
const MOST_BARS: u32 = 32;

/// A moment a sound starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transient {
    /// Seconds from the start of the file.
    pub seconds: f64,
    /// From 0 (barely a change) to 1 (a sharp start out of near silence).
    pub strength: f32,
}

/// The tempo a file seems to have.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub bpm: f64,
    /// From 0 to 1: how regular the file's rhythm is. Not a probability.
    pub confidence: f32,
    /// The beats the file lasts at `bpm`, when it lasts a whole number of
    /// them (so `bpm` is the one that makes it exact).
    pub beats: Option<u32>,
}

impl Tempo {
    /// Whether the file is a loop of whole bars (1 to 32) at this tempo.
    pub fn is_loop(&self) -> bool {
        self.beats.is_some_and(|b| {
            b % BEATS_PER_BAR == 0 && (1..=MOST_BARS).contains(&(b / BEATS_PER_BAR))
        })
    }
}

/// What detection found in one file.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// How long the file lasts.
    pub seconds: f64,
    /// Every transient, in time order, whatever its strength.
    pub transients: Vec<Transient>,
    pub tempo: Option<Tempo>,
}

/// The strength a transient needs at `sensitivity` (0 to 100): all of them
/// at 100, only the sharpest at 0. 50 asks for 15 dB.
pub fn threshold(sensitivity: u8) -> f32 {
    let s = f32::from(sensitivity.min(100)) / 100.0;
    FLOOR / SPAN + (0.9 - FLOOR / SPAN) * (1.0 - s)
}

impl Analysis {
    /// The transients to offer at `sensitivity` (0 to 100).
    pub fn transients_at(&self, sensitivity: u8) -> impl Iterator<Item = &Transient> {
        let least = threshold(sensitivity);
        self.transients.iter().filter(move |t| t.strength >= least)
    }

    /// The transient nearest `seconds` that [`Analysis::transients_at`]
    /// offers, within `within` seconds.
    pub fn nearest(&self, seconds: f64, sensitivity: u8, within: f64) -> Option<Transient> {
        self.transients_at(sensitivity)
            .copied()
            .filter(|t| (t.seconds - seconds).abs() <= within)
            .min_by(|a, b| {
                (a.seconds - seconds)
                    .abs()
                    .total_cmp(&(b.seconds - seconds).abs())
            })
    }
}

/// The marker that begins a stored analysis.
const MAGIC: &[u8; 4] = b"LMNB";
const VERSION: u8 = 1;

impl Analysis {
    /// The analysis as the cache keeps it.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.transients.len() * 12);
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&self.seconds.to_le_bytes());
        out.extend_from_slice(&(self.transients.len() as u32).to_le_bytes());
        for t in &self.transients {
            out.extend_from_slice(&t.seconds.to_le_bytes());
            out.extend_from_slice(&t.strength.to_le_bytes());
        }
        match self.tempo {
            None => out.push(0),
            Some(t) => {
                out.push(1);
                out.extend_from_slice(&t.bpm.to_le_bytes());
                out.extend_from_slice(&t.confidence.to_le_bytes());
                out.extend_from_slice(&t.beats.unwrap_or(0).to_le_bytes());
            }
        }
        out
    }

    /// The analysis [`Analysis::to_bytes`] wrote, or `None` for anything else
    /// (a cut-off file, another version).
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut at = Cursor { bytes, at: 0 };
        if at.take(4)? != MAGIC || at.take(1)? != [VERSION] {
            return None;
        }
        let seconds = at.f64()?;
        let count = at.u32()? as usize;
        // A count the bytes cannot hold is a damaged file, not an allocation.
        if count > bytes.len() / 12 {
            return None;
        }
        let mut transients = Vec::with_capacity(count);
        for _ in 0..count {
            transients.push(Transient {
                seconds: at.f64()?,
                strength: at.f32()?,
            });
        }
        let tempo = match at.take(1)?[0] {
            0 => None,
            1 => {
                let bpm = at.f64()?;
                let confidence = at.f32()?;
                let beats = at.u32()?;
                Some(Tempo {
                    bpm,
                    confidence,
                    beats: (beats > 0).then_some(beats),
                })
            }
            _ => return None,
        };
        (at.at == bytes.len()).then_some(Self {
            seconds,
            transients,
            tempo,
        })
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let part = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(part)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
}

/// Detection in progress: audio goes in with [`Detector::add`], and
/// [`Detector::finish`] returns the analysis.
#[derive(Debug)]
pub struct Detector {
    rate: u32,
    /// Frames in a hop.
    hop: usize,
    /// The previous sample, for the first difference.
    last: f32,
    /// The first differences of the hop being filled.
    diffs: Vec<f32>,
    /// Each finished hop's level in dB and where, in the hop, its sound
    /// starts.
    level: Vec<f32>,
    start: Vec<u16>,
    frames: u64,
}

impl Detector {
    pub fn new(rate: u32) -> Self {
        let hop = ((f64::from(rate) * HOP_SECONDS).round() as usize).max(8);
        Self {
            rate,
            hop,
            last: 0.0,
            diffs: Vec::with_capacity(hop),
            level: Vec::new(),
            start: Vec::new(),
            frames: 0,
        }
    }

    /// Adds the next frames of the file: the left channel, and the right when
    /// there is one (the two are averaged).
    pub fn add(&mut self, left: &[f32], right: Option<&[f32]>) {
        for (i, &l) in left.iter().enumerate() {
            let x = match right {
                Some(r) => 0.5 * (l + r.get(i).copied().unwrap_or(0.0)),
                None => l,
            };
            self.diffs.push(x - self.last);
            self.last = x;
            if self.diffs.len() == self.hop {
                self.close_hop();
            }
        }
        self.frames += left.len() as u64;
    }

    fn close_hop(&mut self) {
        let mean_square =
            self.diffs.iter().map(|d| f64::from(d * d)).sum::<f64>() / self.diffs.len() as f64;
        let db = (10.0 * (mean_square + 1e-18).log10()) as f32;
        self.level.push(db.max(SILENCE_DB));
        let loudest = self.diffs.iter().fold(0.0_f32, |m, d| m.max(d.abs()));
        let at = self
            .diffs
            .iter()
            .position(|d| d.abs() >= 0.25 * loudest)
            .unwrap_or(0);
        self.start.push(at as u16);
        self.diffs.clear();
    }

    /// The analysis of everything added. A last hop that did not fill is
    /// read as it stands, so a sound in the file's final milliseconds is not
    /// lost.
    pub fn finish(mut self) -> Analysis {
        if !self.diffs.is_empty() {
            self.close_hop();
        }
        let seconds = self.frames as f64 / f64::from(self.rate.max(1));
        let rise = rise(&self.level);
        let picked = pick(&rise);
        let hop_seconds = self.hop as f64 / f64::from(self.rate.max(1));
        let transients: Vec<Transient> = picked
            .iter()
            .map(|p| Transient {
                seconds: (p.hop * self.hop + usize::from(self.start[p.hop])) as f64
                    / f64::from(self.rate),
                strength: (p.rise / SPAN).clamp(0.0, 1.0),
            })
            .collect();
        let counted = picked.iter().filter(|p| p.rise >= COUNTED_DB).count();
        let tempo = if counted >= MOST_NEEDED {
            tempo(&rise, hop_seconds, seconds)
        } else {
            None
        };
        Analysis {
            seconds,
            transients,
            tempo,
        }
    }
}

/// How far each hop's level stands above the two hops before it, in dB.
fn rise(level: &[f32]) -> Vec<f32> {
    (0..level.len())
        .map(|n| {
            let before = match n {
                0 => SILENCE_DB,
                1 => level[0],
                _ => level[n - 1].max(level[n - 2]),
            };
            (level[n] - before).max(0.0)
        })
        .collect()
}

/// A hop chosen as holding a transient.
struct Picked {
    /// The hop the sound starts in.
    hop: usize,
    rise: f32,
}

/// The hops with the largest rise within [`SPACING`] hops either way (the
/// earlier of equals), at least [`FLOOR`]. A sound that starts at the end of
/// a hop rises most in the next one, so the hop before is taken when it holds
/// a good part of the rise.
fn pick(rise: &[f32]) -> Vec<Picked> {
    let mut out = Vec::new();
    for n in 0..rise.len() {
        let r = rise[n];
        if r < FLOOR {
            continue;
        }
        let low = n.saturating_sub(SPACING);
        let high = (n + SPACING + 1).min(rise.len());
        let largest =
            rise[low..n].iter().all(|&o| o < r) && rise[n + 1..high].iter().all(|&o| o <= r);
        if !largest {
            continue;
        }
        let early = n.checked_sub(1).filter(|&p| rise[p] >= 0.3 * r);
        match early {
            Some(p) => out.push(Picked {
                hop: p,
                rise: r + rise[p],
            }),
            None => out.push(Picked { hop: n, rise: r }),
        }
    }
    out
}

/// The tempo of a file from its rise (one value per hop of `hop_seconds`),
/// lasting `seconds`.
fn tempo(rise: &[f32], hop_seconds: f64, seconds: f64) -> Option<Tempo> {
    let take = ((TEMPO_SECONDS / hop_seconds) as usize).min(rise.len());
    let rise = &rise[..take];
    let n = rise.len();
    let shortest = (60.0 / FASTEST / hop_seconds).floor() as usize;
    // A beat is looked for no longer than half the file (two periods of it
    // are needed for anything to repeat), so a one-bar loop of under two
    // seconds still has its tempo found; the slowest tempo stays the limit
    // for a longer file.
    let longest = ((60.0 / SLOWEST / hop_seconds).ceil() as usize).min(n / 2);
    if shortest < 2 || n < shortest * 4 || longest <= shortest + 1 {
        return None;
    }
    let mean = rise.iter().map(|&r| f64::from(r)).sum::<f64>() / n as f64;
    let centred: Vec<f64> = rise.iter().map(|&r| f64::from(r) - mean).collect();
    let variance = centred.iter().map(|c| c * c).sum::<f64>() / n as f64;
    if variance < 1e-9 {
        return None;
    }
    // The correlation at a lag, from 0 to about 1 for a perfectly regular
    // file: terms are averaged, so a lag near the file's length is not
    // favoured or penalised for having fewer.
    let correlation = |lag: usize| -> f64 {
        let terms = n - lag;
        let sum: f64 = (0..terms).map(|i| centred[i] * centred[i + lag]).sum();
        sum / terms as f64 / variance
    };
    let mut scores = vec![0.0; longest + 2];
    for (lag, slot) in scores
        .iter_mut()
        .enumerate()
        .take(longest + 1)
        .skip(shortest)
    {
        let mut sum = 0.0;
        let mut weights = 0.0;
        for (multiple, weight) in [(1, 1.0), (2, 0.5), (4, 0.25)] {
            let at = lag * multiple;
            // Half the file at least must overlap.
            if at * 2 <= n {
                sum += weight * correlation(at);
                weights += weight;
            }
        }
        *slot = if weights > 0.0 { sum / weights } else { 0.0 };
    }
    // A mild preference for the middle of the range settles a tie between a
    // tempo and its double.
    let prior = |lag: usize| {
        let bpm = 60.0 / (lag as f64 * hop_seconds);
        let octaves = (bpm / 120.0).ln() / std::f64::consts::LN_2;
        (-0.5 * (octaves / 0.9).powi(2)).exp()
    };
    let (best, _) = (shortest..=longest)
        .map(|lag| (lag, scores[lag] * prior(lag)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let peak = scores[best];
    if peak <= 0.0 {
        return None;
    }
    // Between hops: the parabola through the neighbours.
    let (a, b, c) = (scores[best - 1], scores[best], scores[best + 1]);
    let denominator = a - 2.0 * b + c;
    let shift = if denominator.abs() > 1e-12 {
        (0.5 * (a - c) / denominator).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let mut bpm = 60.0 / ((best as f64 + shift) * hop_seconds);
    let mut confidence = (peak as f32).clamp(0.0, 1.0);
    // A file that lasts a whole number of beats at about this tempo was cut
    // to them: the tempo that makes it exact is the file's.
    let mut beats = None;
    let count = (seconds * bpm / 60.0).round();
    if count >= 2.0 {
        let exact = 60.0 * count / seconds;
        if (exact - bpm).abs() / bpm <= SNAP {
            bpm = exact;
            beats = Some(count as u32);
        }
    }
    let found = Tempo {
        bpm,
        confidence,
        beats,
    };
    if found.is_loop() {
        // Whole bars from a loop's own length are evidence of their own.
        confidence = (confidence + 0.2).min(1.0);
    }
    (confidence >= WEAKEST).then_some(Tempo {
        confidence,
        ..found
    })
}

/// Detects over a whole stream file read in blocks. `progress` gets 0 to 1;
/// `None` if `cancel` was set.
pub fn analyse(
    mut read: impl FnMut(u64, &mut [&mut [f32]]) -> Result<usize, MediaError>,
    rate: u32,
    frames: u64,
    channels: usize,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<Option<Analysis>, MediaError> {
    const BLOCK: usize = 65_536;
    let mut detector = Detector::new(rate);
    let mut blocks = vec![vec![0.0; BLOCK]; channels.clamp(1, 2)];
    let mut at = 0;
    while at < frames {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let mut parts: Vec<&mut [f32]> = blocks.iter_mut().map(Vec::as_mut_slice).collect();
        let got = read(at, &mut parts)?;
        if got == 0 {
            break;
        }
        detector.add(&blocks[0][..got], blocks.get(1).map(|r| &r[..got]));
        at += got as u64;
        progress(at as f32 / frames as f32);
    }
    Ok(Some(detector.finish()))
}

#[cfg(test)]
mod tests;
