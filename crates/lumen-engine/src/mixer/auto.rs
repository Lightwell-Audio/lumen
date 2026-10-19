//! Automation in the mixer (M7 slice A0, ADR-0081): lanes baked to the
//! device's sample clock, and the stages that play them.
//!
//! The interface bakes a track's lanes (ticks, in the project's units) into
//! an [`AutoLane`] when it builds a routing, with the tempo map and the
//! device rate; the audio thread then only reads. A lane is a pure function
//! of the timeline sample, so a loop wrap, a locate or a tail rendered ahead
//! always read the same values.
//!
//! **Time.** Points land on their exact sample. Inside a straight or curved
//! segment the value is worked out every [`KNOT`] frames, counted from the
//! segment's start, and the gain runs straight between: the same numbers at
//! any block size, eight evaluations a 256-frame chunk. The step from one
//! held value to the next ramps over 1 ms from its point, so a mute or a
//! stepped fader never clicks.
//!
//! **Units.** A lane holds the target's own units. A level lane is in dB and
//! is turned into a gain at the knots (silent at the floor), so a fade
//! between two dB values is a straight line in dB.

use std::ops::Range;
use std::sync::Arc;

use lumen_core::automation::{FLOOR_DB, Shape};
use lumen_core::pan::PanLaw;

use super::plan::CHUNK;

/// Frames between two evaluations inside a straight or curved segment
/// (D4). A gain straight between two knots is within 0.001 of the curve for
/// any shape a segment can draw, and within 0.02 (0.17 dB) at the one bend a
/// pan law has, at its centre.
pub const KNOT: i64 = 32;

/// A step from one held value to the next ramps over this long.
const STEP_SECONDS: f64 = 0.001;

/// A change of plan (a lane edited while playing) glides the output from
/// where it was to where the new lane puts it, over this long.
const FOLLOW_SECONDS: f32 = 0.02;

/// One point of a lane on the sample clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoPoint {
    /// The timeline sample.
    pub at: i64,
    pub value: f32,
    /// The segment leaving the point.
    pub shape: Shape,
}

/// A lane baked to samples: at least one point, in time order, one to a
/// sample. Cloning shares the points.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoLane {
    points: Arc<[AutoPoint]>,
}

impl AutoLane {
    /// The lane of `points`; `None` when none of them can be played. Points
    /// that are not finite are dropped, and where several fall on one
    /// sample the last one wins.
    pub fn new(mut points: Vec<AutoPoint>) -> Option<Self> {
        points.retain(|p| p.value.is_finite());
        points.sort_by_key(|p| p.at);
        let mut kept: Vec<AutoPoint> = Vec::with_capacity(points.len());
        for point in points {
            match kept.last_mut() {
                Some(last) if last.at == point.at => *last = point,
                _ => kept.push(point),
            }
        }
        (!kept.is_empty()).then(|| Self {
            points: kept.into(),
        })
    }

    pub fn points(&self) -> &[AutoPoint] {
        &self.points
    }

    /// The raw value at sample `at`, without the ramp between steps and the
    /// knots: what `lumen_core::automation::Lane::value_at` gives at the
    /// same moment.
    pub fn value_at(&self, at: i64) -> f32 {
        let after = self.points.partition_point(|p| p.at <= at);
        let Some(before) = after.checked_sub(1) else {
            return self.points[0].value;
        };
        self.segment(before, at)
    }

    /// The value of the segment leaving point `j` at sample `at`.
    fn segment(&self, j: usize, at: i64) -> f32 {
        let a = self.points[j];
        let Some(b) = self.points.get(j + 1) else {
            return a.value;
        };
        let x = (at - a.at) as f64 / (b.at - a.at) as f64;
        a.shape.between(a.value, b.value, x as f32)
    }

    /// Calls `emit` for each stretch of the `frames` frames from timeline
    /// sample `start`: the frames it covers, and the raw value at the two
    /// knots (`k0` to `k1`, in samples) it runs between in a straight line.
    /// A held stretch has `k0 == k1`.
    fn runs(&self, start: i64, frames: usize, rate: u32, mut emit: impl FnMut(Run)) {
        let end = start + frames as i64;
        let ramp = ((f64::from(rate) * STEP_SECONDS) as i64).max(1);
        let points = &self.points;
        let mut x = start;
        while x < end {
            let j = points.partition_point(|p| p.at <= x);
            let Some(i) = j.checked_sub(1) else {
                // Before the first point: the first value is held.
                let stop = points[0].at.min(end);
                emit(Run::held(start, x..stop, points[0].value));
                x = stop;
                continue;
            };
            let a = points[i];
            let seg_end = points.get(j).map(|b| b.at);
            let stop = seg_end.map_or(end, |e| e.min(end));
            let held = a.shape == Shape::Step || seg_end.is_none();
            // A held value that differs from the one before it is reached by
            // a ramp from its point, not by a jump.
            if i > 0 && x < a.at + ramp {
                let before = points[i - 1];
                if before.shape == Shape::Step && before.value != a.value {
                    let ramp_end = (a.at + ramp).min(stop);
                    while x < ramp_end {
                        let w = (x - a.at) as f32 / ramp as f32;
                        let now = if held { a.value } else { self.segment(i, x) };
                        emit(Run::held(
                            start,
                            x..x + 1,
                            before.value + (now - before.value) * w,
                        ));
                        x += 1;
                    }
                    continue;
                }
            }
            if held {
                emit(Run::held(start, x..stop, a.value));
                x = stop;
                continue;
            }
            while x < stop {
                let k0 = a.at + KNOT * ((x - a.at) / KNOT);
                let k1 = (k0 + KNOT).min(seg_end.unwrap_or(i64::MAX));
                let to = k1.min(stop);
                emit(Run {
                    frames: (x - start) as usize..(to - start) as usize,
                    k0,
                    k1,
                    v0: self.segment(i, k0),
                    v1: self.segment(i, k1),
                });
                x = to;
            }
        }
    }
}

/// A stretch of frames inside one chunk, between two knots.
struct Run {
    frames: Range<usize>,
    k0: i64,
    k1: i64,
    v0: f32,
    v1: f32,
}

impl Run {
    fn held(start: i64, at: Range<i64>, value: f32) -> Self {
        Self {
            frames: (at.start - start) as usize..(at.end - start) as usize,
            k0: at.start,
            k1: at.start,
            v0: value,
            v1: value,
        }
    }
}

/// What a lane's value becomes in the audio.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    /// A level in dB, silent at [`FLOOR_DB`].
    Db,
    /// Mute, 0 (heard) to 1 (muted): the gain is what is left.
    Mute,
    /// Pan, −1 to 1: the left and right gains of the law.
    Pan(PanLaw),
}

impl Kind {
    fn gains(self, value: f32) -> [f32; 2] {
        match self {
            Kind::Db => [db_gain(value), 0.0],
            Kind::Mute => [1.0 - value.clamp(0.0, 1.0), 0.0],
            Kind::Pan(law) => law.gains(value),
        }
    }

    fn outputs(self) -> usize {
        match self {
            Kind::Pan(_) => 2,
            _ => 1,
        }
    }
}

/// The linear gain of `db`: silence at the floor and below.
pub fn db_gain(db: f32) -> f32 {
    if db <= FLOOR_DB {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

/// One lane playing: its gains for the chunk in hand, and where it was when
/// the plan before it stopped.
#[derive(Debug, Clone)]
pub(super) struct Stage {
    lane: AutoLane,
    kind: Kind,
    /// Gains for the chunk: `CHUNK` frames for the first output, then for
    /// the second (pan only).
    out: Box<[f32]>,
    /// The last gains played.
    last: Option<[f32; 2]>,
    /// Where the plan before it left off, until this plan's first block
    /// works the difference out.
    carried: Option<[f32; 2]>,
    /// The difference being glided away, and the frames left of the glide.
    offset: [f32; 2],
    left: u32,
}

impl Stage {
    fn new(lane: AutoLane, kind: Kind) -> Self {
        Self {
            lane,
            kind,
            out: vec![0.0; 2 * CHUNK].into(),
            last: None,
            carried: None,
            offset: [0.0; 2],
            left: 0,
        }
    }

    /// The gains for output `side` of the last [`Stage::render`].
    pub(super) fn gains(&self, side: usize, frames: usize) -> &[f32] {
        &self.out[side * CHUNK..side * CHUNK + frames]
    }

    /// Works out the gains for `frames` frames from timeline sample
    /// `start`. With `advance` false (a tail rendered ahead, or a look
    /// ahead of the real block) nothing about the stage moves on.
    pub(super) fn render(&mut self, start: i64, frames: usize, rate: u32, advance: bool) {
        let kind = self.kind;
        let outputs = kind.outputs();
        let (first, second) = self.out.split_at_mut(CHUNK);
        let sides = [&mut first[..frames], &mut second[..frames]];
        let [a, b] = sides;
        self.lane.runs(start, frames, rate, |run| {
            let (g0, g1) = (kind.gains(run.v0), kind.gains(run.v1));
            let span = (run.k1 - run.k0) as f32;
            for f in run.frames {
                let x = if run.k0 == run.k1 {
                    0.0
                } else {
                    (start + f as i64 - run.k0) as f32 / span
                };
                a[f] = g0[0] + (g1[0] - g0[0]) * x;
                if outputs == 2 {
                    b[f] = g0[1] + (g1[1] - g0[1]) * x;
                }
            }
        });
        // A lane that changed under the playhead glides from where the old
        // one was to where this one is, compared at the frame before.
        let (mut offset, mut left) = (self.offset, self.left);
        if let Some(carried) = self.carried {
            let mut before = [0.0f32; 2];
            self.lane.runs(start - 1, 1, rate, |run| {
                let (g0, g1) = (kind.gains(run.v0), kind.gains(run.v1));
                let x = if run.k0 == run.k1 {
                    0.0
                } else {
                    ((start - 1) - run.k0) as f32 / (run.k1 - run.k0) as f32
                };
                before = [g0[0] + (g1[0] - g0[0]) * x, g0[1] + (g1[1] - g0[1]) * x];
            });
            offset = [carried[0] - before[0], carried[1] - before[1]];
            left = (rate as f32 * FOLLOW_SECONDS).max(1.0) as u32;
        }
        let total = (rate as f32 * FOLLOW_SECONDS).max(1.0);
        if left > 0 {
            let (first, second) = self.out.split_at_mut(CHUNK);
            for (side, samples) in [first, second].into_iter().take(outputs).enumerate() {
                for (k, g) in samples[..frames].iter_mut().enumerate() {
                    let remaining = left.saturating_sub(k as u32) as f32 / total;
                    *g += offset[side] * remaining;
                }
            }
        }
        if advance && frames > 0 {
            self.carried = None;
            self.offset = offset;
            self.left = left.saturating_sub(frames as u32);
            self.last = Some([
                self.out[frames - 1],
                if outputs == 2 {
                    self.out[CHUNK + frames - 1]
                } else {
                    0.0
                },
            ]);
        }
    }

    /// Takes over where `old` left off, so an edited lane glides in.
    fn carry(&mut self, old: &Stage) {
        self.carried = old.last;
        self.offset = [0.0; 2];
        self.left = 0;
    }

    /// The playhead jumped (and the engine crossfades it): start at the
    /// lane's value.
    fn settle(&mut self) {
        self.carried = None;
        self.offset = [0.0; 2];
        self.left = 0;
    }

    /// Multiplies `gain` by this stage's first output.
    fn scale(&mut self, start: i64, rate: u32, advance: bool, gain: &mut [f32]) {
        self.render(start, gain.len(), rate, advance);
        for (g, s) in gain.iter_mut().zip(&self.out[..CHUNK]) {
            *g *= s;
        }
    }
}

/// What a node's lanes bake to, as the routing hands them over. Each is the
/// lane of that target, if it has one.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodeAutomation {
    /// Input trim, in dB.
    pub trim: Option<AutoLane>,
    /// Mute, 0 or 1.
    pub mute: Option<AutoLane>,
    /// The fader, in dB.
    pub volume: Option<AutoLane>,
    /// Pan, −1 to 1.
    pub pan: Option<AutoLane>,
    /// Each send's level in dB, by the send's place in the node's `sends`.
    pub sends: Vec<Option<AutoLane>>,
}

impl NodeAutomation {
    pub fn is_empty(&self) -> bool {
        self.trim.is_none()
            && self.mute.is_none()
            && self.volume.is_none()
            && self.pan.is_none()
            && self.sends.iter().all(Option::is_none)
    }

    /// Which lanes there are, for the plan's shape: a plan with another set
    /// crossfades in.
    pub(super) fn shape(&self) -> [bool; 4] {
        [
            self.trim.is_some(),
            self.mute.is_some(),
            self.volume.is_some(),
            self.pan.is_some(),
        ]
    }
}

/// A node's stages, with how far its input trails the timeline before and
/// after its devices (each lane is read at the time of the audio it acts
/// on).
#[derive(Debug, Clone)]
pub(super) struct NodeStages {
    lag_pre: i64,
    lag_post: i64,
    trim: Option<Stage>,
    mute: Option<Stage>,
    volume: Option<Stage>,
    pan: Option<Stage>,
    sends: Vec<Option<Stage>>,
}

impl NodeStages {
    pub(super) fn new(lanes: &NodeAutomation, law: PanLaw, lag_pre: u32, lag_post: u32) -> Self {
        let stage = |lane: &Option<AutoLane>, kind| lane.clone().map(|l| Stage::new(l, kind));
        Self {
            lag_pre: i64::from(lag_pre),
            lag_post: i64::from(lag_post),
            trim: stage(&lanes.trim, Kind::Db),
            mute: stage(&lanes.mute, Kind::Mute),
            volume: stage(&lanes.volume, Kind::Db),
            pan: stage(&lanes.pan, Kind::Pan(law)),
            sends: lanes.sends.iter().map(|l| stage(l, Kind::Db)).collect(),
        }
    }

    pub(super) fn has_mute(&self) -> bool {
        self.mute.is_some()
    }

    /// Multiplies the input gain by the trim lane, and by the mute lane
    /// when mute comes with the input (a node with no devices).
    pub(super) fn input(
        &mut self,
        start: i64,
        rate: u32,
        advance: bool,
        split: bool,
        gain: &mut [f32],
    ) {
        let at = start - self.lag_pre;
        if let Some(trim) = self.trim.as_mut() {
            trim.scale(at, rate, advance, gain);
        }
        if !split && let Some(mute) = self.mute.as_mut() {
            mute.scale(at, rate, advance, gain);
        }
    }

    /// Multiplies the cue gain by the trim lane.
    pub(super) fn cue(&mut self, start: i64, rate: u32, advance: bool, gain: &mut [f32]) {
        let at = start - self.lag_pre;
        if let Some(trim) = self.trim.as_mut() {
            trim.scale(at, rate, advance, gain);
        }
    }

    /// Multiplies the mute gain that follows the devices by the mute lane.
    pub(super) fn heard(&mut self, start: i64, rate: u32, advance: bool, gain: &mut [f32]) {
        let at = start - self.lag_post;
        if let Some(mute) = self.mute.as_mut() {
            mute.scale(at, rate, advance, gain);
        }
    }

    /// Applies the fader and pan lanes to the node's sound.
    pub(super) fn mix(
        &mut self,
        start: i64,
        rate: u32,
        advance: bool,
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let at = start - self.lag_post;
        let frames = left.len();
        if let Some(volume) = self.volume.as_mut() {
            volume.render(at, frames, rate, advance);
            let gains = volume.gains(0, frames);
            for (s, g) in left.iter_mut().zip(gains) {
                *s *= g;
            }
            for (s, g) in right.iter_mut().zip(gains) {
                *s *= g;
            }
        }
        if let Some(pan) = self.pan.as_mut() {
            pan.render(at, frames, rate, advance);
            for (s, g) in left.iter_mut().zip(pan.gains(0, frames)) {
                *s *= g;
            }
            for (s, g) in right.iter_mut().zip(pan.gains(1, frames)) {
                *s *= g;
            }
        }
    }

    /// Multiplies send `index`'s gain by its lane.
    pub(super) fn send(
        &mut self,
        index: usize,
        start: i64,
        rate: u32,
        advance: bool,
        gain: &mut [f32],
    ) {
        let at = start - self.lag_post;
        if let Some(Some(stage)) = self.sends.get_mut(index) {
            stage.scale(at, rate, advance, gain);
        }
    }

    /// Takes over where the same node in the plan before left off.
    pub(super) fn carry(&mut self, old: &NodeStages) {
        fn pair(new: &mut Option<Stage>, old: &Option<Stage>) {
            if let (Some(new), Some(old)) = (new.as_mut(), old.as_ref()) {
                new.carry(old);
            }
        }
        pair(&mut self.trim, &old.trim);
        pair(&mut self.mute, &old.mute);
        pair(&mut self.volume, &old.volume);
        pair(&mut self.pan, &old.pan);
        for (new, old) in self.sends.iter_mut().zip(&old.sends) {
            pair(new, old);
        }
    }

    pub(super) fn settle(&mut self) {
        let stages = [
            &mut self.trim,
            &mut self.mute,
            &mut self.volume,
            &mut self.pan,
        ];
        for stage in stages.into_iter().chain(self.sends.iter_mut()).flatten() {
            stage.settle();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::curve::Curve;

    fn point(at: i64, value: f32, shape: Shape) -> AutoPoint {
        AutoPoint { at, value, shape }
    }

    fn lane(points: &[(i64, f32, Shape)]) -> AutoLane {
        AutoLane::new(points.iter().map(|&(a, v, s)| point(a, v, s)).collect()).unwrap()
    }

    /// The gains `stage` plays for `frames` frames from `start`, a chunk at a
    /// time as the plan asks for them.
    fn play(stage: &mut Stage, start: i64, frames: usize, side: usize) -> Vec<f32> {
        let mut out = Vec::new();
        let mut done = 0;
        while done < frames {
            let n = CHUNK.min(frames - done);
            stage.render(start + done as i64, n, 48_000, true);
            out.extend_from_slice(stage.gains(side, n));
            done += n;
        }
        out
    }

    /// The value a mute lane holds, as played.
    fn values(lane: &AutoLane, start: i64, frames: usize) -> Vec<f32> {
        let mut stage = Stage::new(lane.clone(), Kind::Mute);
        play(&mut stage, start, frames, 0)
            .iter()
            .map(|g| 1.0 - g)
            .collect()
    }

    #[test]
    fn a_lane_without_a_usable_point_is_none_and_the_last_point_on_a_sample_wins() {
        assert!(AutoLane::new(vec![]).is_none());
        assert!(AutoLane::new(vec![point(0, f32::NAN, Shape::Linear)]).is_none());
        let l = lane(&[
            (10, 0.2, Shape::Step),
            (10, 0.7, Shape::Step),
            (0, 0.1, Shape::Step),
        ]);
        assert_eq!(l.points().len(), 2);
        assert_eq!(l.value_at(10), 0.7);
    }

    #[test]
    fn it_holds_the_first_value_before_and_the_last_after() {
        let l = lane(&[(100, 0.25, Shape::Linear), (200, 0.75, Shape::Linear)]);
        let v = values(&l, 0, 300);
        assert!(v[..=100].iter().all(|&x| (x - 0.25).abs() < 1e-6));
        assert!(v[200..].iter().all(|&x| (x - 0.75).abs() < 1e-6));
    }

    #[test]
    fn a_straight_segment_reaches_each_end_on_its_exact_sample() {
        let l = lane(&[(1000, 0.0, Shape::Linear), (1100, 1.0, Shape::Linear)]);
        let v = values(&l, 990, 130);
        assert_eq!(v[10], 0.0, "the first point's own sample");
        assert!(
            (v[10 + 100] - 1.0).abs() < 1e-6,
            "the next point's own sample"
        );
        assert!((v[10 + 50] - 0.5).abs() < 1e-5, "halfway");
    }

    #[test]
    fn the_curve_between_knots_stays_close_to_the_curve() {
        for curve in [Curve::SLOW, Curve::FAST, Curve::S_CURVE] {
            let l = lane(&[(0, 0.0, Shape::Curve(curve)), (48_000, 1.0, Shape::Linear)]);
            let v = values(&l, 0, 48_000);
            let worst = (0..48_000)
                .map(|k| (v[k] - l.value_at(k as i64)).abs())
                .fold(0.0, f32::max);
            assert!(worst < 1e-3, "{curve:?}: {worst}");
        }
    }

    #[test]
    fn a_step_holds_and_then_ramps_over_a_millisecond_from_its_point() {
        let l = lane(&[(1000, 0.0, Shape::Step), (2000, 1.0, Shape::Step)]);
        let v = values(&l, 900, 1300);
        assert_eq!(v[1099 - 900], 0.0);
        // Up to the point, the old value; from it, a ramp of 48 frames.
        assert_eq!(v[2000 - 900 - 1], 0.0);
        assert_eq!(v[2000 - 900], 0.0, "the ramp starts at the point");
        let mid = v[2000 - 900 + 24];
        assert!((mid - 0.5).abs() < 0.02, "{mid}");
        assert_eq!(v[2000 - 900 + 48], 1.0);
        assert!(v[2000 - 900 + 100..].iter().all(|&x| x == 1.0));
    }

    #[test]
    fn the_same_values_at_any_block_size() {
        let l = lane(&[
            (50, 0.0, Shape::Curve(Curve::S_CURVE)),
            (700, 1.0, Shape::Step),
            (900, 0.3, Shape::Linear),
            (1500, 0.9, Shape::Linear),
        ]);
        let whole = values(&l, 0, 2000);
        for block in [1usize, 7, 31, 32, 33, 64, 100, 256] {
            let mut parts = Vec::new();
            let mut at = 0;
            while at < 2000 {
                let n = block.min(2000 - at);
                parts.extend(values(&l, at as i64, n));
                at += n;
            }
            assert_eq!(parts, whole, "block of {block}");
        }
    }

    #[test]
    fn a_pan_lane_plays_both_sides_of_the_law() {
        let l = lane(&[(0, -1.0, Shape::Linear), (1000, 1.0, Shape::Linear)]);
        let mut stage = Stage::new(l.clone(), Kind::Pan(PanLaw::Balance));
        let left = play(&mut stage, 0, 1001, 0);
        let mut stage = Stage::new(l, Kind::Pan(PanLaw::Balance));
        let right = play(&mut stage, 0, 1001, 1);
        assert!((left[0] - 1.0).abs() < 1e-6 && right[0].abs() < 1e-6);
        assert!(left[1000].abs() < 1e-6 && (right[1000] - 1.0).abs() < 1e-6);
        // The balance law bends at the centre, which falls between two
        // knots: a gain straight between them is within 0.02 of the law
        // (0.17 dB).
        assert!((left[500] - 1.0).abs() < 0.02 && (right[500] - 1.0).abs() < 0.02);
    }

    #[test]
    fn a_level_lane_is_straight_in_db_and_silent_at_the_floor() {
        assert_eq!(db_gain(FLOOR_DB), 0.0);
        assert_eq!(db_gain(-200.0), 0.0);
        assert!((db_gain(0.0) - 1.0).abs() < 1e-6);
        assert!((db_gain(-6.0) - 0.501_187).abs() < 1e-5);
        let l = lane(&[(0, FLOOR_DB, Shape::Linear), (48_000, 0.0, Shape::Linear)]);
        let mut stage = Stage::new(l.clone(), Kind::Db);
        stage.render(0, 256, 48_000, true);
        assert_eq!(stage.gains(0, 256)[0], 0.0);
        // Halfway through, −50 dB.
        let mut stage = Stage::new(l, Kind::Db);
        stage.render(24_000, 256, 48_000, true);
        let g = stage.gains(0, 256)[0];
        assert!((g - db_gain(-50.0)).abs() < 1e-4, "{g}");
    }

    #[test]
    fn an_edited_lane_glides_from_where_the_old_one_was() {
        let old = lane(&[(0, 0.0, Shape::Step)]);
        let new = lane(&[(0, 1.0, Shape::Step)]);
        let mut before = Stage::new(old, Kind::Mute);
        before.render(0, 128, 48_000, true);
        let mut after = Stage::new(new, Kind::Mute);
        after.carry(&before);
        // Mute 0 gives gain 1; mute 1 gives gain 0.
        after.render(128, 256, 48_000, true);
        let g = after.gains(0, 256);
        assert!(g[0] > 0.99, "starts where it was: {}", g[0]);
        assert!(g[255] < g[0]);
        let mut later = Vec::new();
        for k in 0..5 {
            after.render(384 + k * 256, 256, 48_000, true);
            later.extend_from_slice(after.gains(0, 256));
        }
        assert_eq!(*later.last().unwrap(), 0.0, "ends on the new lane");
        assert!(later.windows(2).all(|w| w[1] <= w[0] + 1e-6), "monotone");
    }

    #[test]
    fn an_unchanged_lane_carries_over_without_a_glide() {
        let l = lane(&[(0, 0.0, Shape::Linear), (48_000, 1.0, Shape::Linear)]);
        let mut a = Stage::new(l.clone(), Kind::Mute);
        a.render(0, 256, 48_000, true);
        let mut b = Stage::new(l.clone(), Kind::Mute);
        b.carry(&a);
        b.render(256, 256, 48_000, true);
        let mut c = Stage::new(l, Kind::Mute);
        c.render(256, 256, 48_000, true);
        assert_eq!(b.gains(0, 256), c.gains(0, 256));
    }

    #[test]
    fn a_tail_read_ahead_moves_nothing() {
        let l = lane(&[(0, 0.0, Shape::Linear), (1000, 1.0, Shape::Linear)]);
        let mut stage = Stage::new(l, Kind::Mute);
        stage.render(0, 256, 48_000, false);
        assert!(stage.last.is_none());
    }
}
