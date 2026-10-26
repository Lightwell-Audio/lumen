//! The arrangement's frame-time benchmark (M3 slice 4b; the quality bar's
//! "60 fps minimum, 120 target with 200 tracks and 20,000 clips").
//!
//! `--stress` opens the generated quality-bar project. `--bench-frames
//! <out.json>` then runs with vsync off and drives the view: a warm-up,
//! then horizontal scrolling, vertical scrolling and zooming, recording the
//! time between frames in each phase. Last it sits idle for three seconds
//! and counts repaints (near-zero idle CPU). It writes the report, prints
//! it, and exits with status 3 if any phase misses 60 fps at the 99th
//! percentile. CI has no GPU, so this runs on the development machine
//! (ADR-0011); results are recorded in `docs/PROGRESS.md`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Frames per measured phase.
const FRAMES: usize = 600;
/// Frames rendered before measuring (pipelines, caches, text layout).
const WARM_UP: usize = 120;
/// How long the idle phase measures.
const IDLE: Duration = Duration::from_secs(3);
/// Idle is measured from this long after the last driven frame: once what
/// the scrolling started has finished (M4.5: the scroll bar lingers 0.8 s,
/// then fades; with vsync off its fade alone was 30 to 140 frames).
const SETTLE: Duration = Duration::from_millis(1500);
/// The floor: 60 fps.
const FLOOR_MS: f64 = 1000.0 / 60.0;

/// Notes in the stress project's dense clip (the quality bar's 100,000
/// notes, M5 slice 3b).
pub const DENSE_NOTES: usize = 100_000;

/// Gives every track of the stress project a volume lane of 200 points
/// (`--bench-lanes`), a slow wave across 400 beats, for the lanes' drawing
/// to be measured with all 200 tracks showing one.
pub fn add_lanes(project: &mut lumen_project::model::Project) {
    use lumen_core::automation::{Lane, Point};
    use lumen_core::time::Ticks;
    use lumen_project::automation::{AutomationLane, Target};
    use std::sync::Arc;

    let points = (0..200)
        .map(|i| {
            let wave = (i as f32 * 0.35).sin();
            Point::new(Ticks::quarters(i * 2), -12.0 + 10.0 * wave)
        })
        .collect();
    let lane = Lane::new(points).expect("sorted, finite points");
    for track in &mut project.tracks {
        Arc::make_mut(track)
            .automation
            .push(AutomationLane::new(Target::Volume, lane.clone()));
    }
}

/// Adds the stress project's dense MIDI track: one clip of
/// [`DENSE_NOTES`] notes across 400 beats over a six-octave range, for the
/// arrangement's mini notes and the piano roll to be measured on.
pub fn add_dense_midi(project: &mut lumen_project::model::Project) {
    use lumen_core::time::{PPQ, Ticks};
    use lumen_project::model::{Clip, ClipContent, Id, Note};
    use std::sync::Arc;

    const BEATS: i64 = 400;
    let Some(template) = project
        .tracks
        .iter()
        .find(|t| t.kind == lumen_project::model::TrackKind::Midi)
    else {
        return;
    };
    let mut track = (**template).clone();
    track.id = Id(900_000_001);
    track.name = "Dense".into();
    track.parent = None;
    track.devices = Vec::new();
    let step = BEATS * PPQ / DENSE_NOTES as i64;
    let notes: Vec<Note> = (0..DENSE_NOTES as i64)
        .map(|i| {
            Note::new(
                Ticks(i * step),
                Ticks(step * 3),
                36 + ((i * 7) % 72) as u8,
                40 + ((i * 13) % 80) as u8,
            )
        })
        .collect();
    track.clips = vec![Arc::new(Clip {
        id: Id(900_000_002),
        name: "Dense".into(),
        start: Ticks::ZERO,
        length: Ticks::quarters(BEATS),
        muted: false,
        color: None,
        locked: false,
        crossfade: false,
        fade_in: Default::default(),
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Default::default(),
        envelopes: Default::default(),
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        takes: None,
        content: ClipContent::Midi {
            notes,
            events: Vec::new(),
        },
    })];
    project.tracks.push(Arc::new(track));
}

/// Ten seconds of drum-like stereo audio at `rate` for the stress
/// project's files: a kick on every beat at 120 BPM, a hat between, and a
/// slow swell, so its waveforms have real detail at every zoom.
pub fn stress_audio(rate: u32) -> Vec<Vec<f32>> {
    let rate_f = rate as f32;
    let mut noise = 0x2545_F491_u32;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for i in 0..rate as usize * 10 {
        let t = i as f32 / rate_f;
        let beat = t % 0.5;
        let kick = (std::f32::consts::TAU * (50.0 + 120.0 * (-beat * 30.0).exp()) * beat).sin()
            * (-beat * 8.0).exp()
            * 0.9;
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        let hat = (noise as f32 / u32::MAX as f32 * 2.0 - 1.0)
            * (-((t + 0.25) % 0.5) * 60.0).exp()
            * 0.35;
        let swell = (std::f32::consts::TAU * 220.0 * t).sin()
            * 0.3
            * (0.5 - 0.5 * (std::f32::consts::TAU * t / 10.0).cos());
        left.push((kick + hat + swell).clamp(-1.0, 1.0));
        right.push((kick * 0.7 + swell * 1.3 - hat).clamp(-1.0, 1.0));
    }
    vec![left, right]
}

/// What a measured phase does to the view.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    WarmUp,
    ScrollTime,
    ScrollTracks,
    Zoom,
    Idle,
}

impl Phase {
    const MEASURED: [Phase; 3] = [Phase::ScrollTime, Phase::ScrollTracks, Phase::Zoom];

    fn key(self) -> &'static str {
        match self {
            Phase::WarmUp => "warm_up",
            Phase::ScrollTime => "scroll_time",
            Phase::ScrollTracks => "scroll_tracks",
            Phase::Zoom => "zoom",
            Phase::Idle => "idle",
        }
    }

    fn next(self) -> Option<Phase> {
        match self {
            Phase::WarmUp => Some(Phase::ScrollTime),
            Phase::ScrollTime => Some(Phase::ScrollTracks),
            Phase::ScrollTracks => Some(Phase::Zoom),
            Phase::Zoom => Some(Phase::Idle),
            Phase::Idle => None,
        }
    }
}

/// Where the benchmark puts the view this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub left_beat: f32,
    pub px_per_beat: f32,
    /// Vertical scroll as a fraction of the scrollable height.
    pub top: f32,
}

/// What the app does this frame.
pub enum Step {
    /// Show this view and draw the next frame at once.
    Drive(View),
    /// Leave the view alone and do not ask for frames.
    Idle,
    /// Finished: the report, and whether every phase met the floor.
    Done(String, bool),
}

pub struct Bench {
    out: PathBuf,
    phase: Phase,
    frame: usize,
    last: Option<Instant>,
    /// Milliseconds between frames, per measured phase.
    times: Vec<(Phase, Vec<f64>)>,
    idle_since: Option<Instant>,
    idle_frames: u32,
    /// The current frame is a counted idle repaint.
    counting: bool,
    /// What asked for each idle repaint ("file:line reason"), with counts.
    idle_causes: std::collections::BTreeMap<String, u32>,
    /// `--bench-editor`: the dense clip is open in the clip editor, which
    /// is driven to the arrangement's scroll and zoom and scrolls its pitches
    /// with the tracks (M5 slice 3; the roll has its own view, ADR-0048).
    editor: bool,
}

impl Bench {
    pub fn new(out: PathBuf) -> Self {
        Self {
            out,
            phase: Phase::WarmUp,
            frame: 0,
            last: None,
            times: Phase::MEASURED
                .iter()
                .map(|&p| (p, Vec::with_capacity(FRAMES)))
                .collect(),
            idle_since: None,
            idle_frames: 0,
            counting: false,
            idle_causes: std::collections::BTreeMap::new(),
            editor: false,
        }
    }

    /// Measures with the dense clip open in the clip editor.
    pub fn with_editor(mut self, editor: bool) -> Self {
        self.editor = editor;
        self
    }

    /// The dense clip is to be open in the clip editor.
    pub fn editor(&self) -> bool {
        self.editor
    }

    /// Notes what caused an idle repaint (egui's repaint causes).
    pub fn idle_cause(&mut self, cause: String) {
        if !self.counting {
            return;
        }
        *self.idle_causes.entry(cause).or_default() += 1;
    }

    pub fn out(&self) -> &std::path::Path {
        &self.out
    }

    /// Call once at the start of every frame.
    pub fn step(&mut self, now: Instant) -> Step {
        let length = match self.phase {
            Phase::WarmUp => WARM_UP,
            Phase::Idle => usize::MAX,
            _ => FRAMES,
        };
        if self.frame >= length
            && let Some(next) = self.phase.next()
        {
            self.phase = next;
            self.frame = 0;
        }
        // The time since the previous frame belongs to this frame's phase.
        if let Some(last) = self.last.replace(now)
            && let Some((_, times)) = self.times.iter_mut().find(|(p, _)| *p == self.phase)
        {
            times.push((now - last).as_secs_f64() * 1000.0);
        }
        if self.phase == Phase::Idle {
            let since = *self.idle_since.get_or_insert(now);
            let at = now - since;
            if at >= SETTLE + IDLE {
                return self.finish();
            }
            // The frames just after the last driven one settle it (egui
            // lays out twice after a change); idle is measured after that.
            self.counting = at >= SETTLE;
            if self.counting {
                self.idle_frames += 1;
            }
            return Step::Idle;
        }
        let t = self.frame as f32 / FRAMES as f32;
        self.frame += 1;
        Step::Drive(view(self.phase, t))
    }

    /// When to wake for the frame that ends the idle phase: once, a little
    /// late, since timers can fire early and a wake that is too early
    /// would schedule another (and be counted).
    pub fn idle_left(&self, now: Instant) -> Duration {
        let end = SETTLE + IDLE + Duration::from_millis(20);
        self.idle_since
            .map_or(end, |since| end.saturating_sub(now - since))
    }

    fn finish(&self) -> Step {
        let mut pass = true;
        let mut phases = serde_json::Map::new();
        let mut lines = Vec::new();
        for (phase, times) in &self.times {
            let s = stats(times);
            pass &= s.p99 <= FLOOR_MS;
            lines.push(format!(
                "{:<14} p50 {:>6.2} ms ({:>4.0} fps)  p95 {:>6.2}  p99 {:>6.2}  worst {:>6.2}",
                phase.key(),
                s.p50,
                1000.0 / s.p50,
                s.p95,
                s.p99,
                s.max
            ));
            phases.insert(
                phase.key().into(),
                serde_json::json!({
                    "frames": times.len(),
                    "p50_ms": s.p50, "p95_ms": s.p95, "p99_ms": s.p99, "max_ms": s.max,
                    "fps_p50": 1000.0 / s.p50,
                }),
            );
        }
        lines.push(format!(
            "idle           {} repaints in {} s",
            self.idle_frames,
            IDLE.as_secs()
        ));
        for (cause, n) in &self.idle_causes {
            lines.push(format!("               {n} x {cause}"));
        }
        lines.push(format!(
            "{} (floor: 60 fps at p99; target 120)",
            if pass { "PASS" } else { "FAIL" }
        ));
        let report = serde_json::json!({
            "phases": phases,
            "idle_repaints": self.idle_frames,
            "idle_causes": self.idle_causes,
            "idle_seconds": IDLE.as_secs(),
            "pass": pass,
        });
        if let Err(e) = std::fs::write(
            &self.out,
            serde_json::to_string_pretty(&report).unwrap_or_default(),
        ) {
            lines.push(format!("could not write {}: {e}", self.out.display()));
        }
        Step::Done(lines.join("\n"), pass)
    }
}

/// The view at fraction `t` of a phase.
fn view(phase: Phase, t: f32) -> View {
    let wave = 0.5 - 0.5 * (t * std::f32::consts::TAU).cos();
    match phase {
        // The project spans 400 beats; sweep across most of it and back.
        Phase::ScrollTime => View {
            left_beat: 340.0 * wave,
            px_per_beat: 24.0,
            top: 0.3,
        },
        Phase::ScrollTracks => View {
            left_beat: 40.0,
            px_per_beat: 24.0,
            top: wave,
        },
        // From the closest zoom out to the whole project and back.
        Phase::Zoom => View {
            left_beat: 0.0,
            px_per_beat: 400.0 * (2.0_f32 / 400.0).powf(wave),
            top: 0.3,
        },
        Phase::WarmUp | Phase::Idle => View {
            left_beat: 0.0,
            px_per_beat: 24.0,
            top: 0.0,
        },
    }
}

#[derive(Debug, PartialEq)]
struct Stats {
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

fn stats(times: &[f64]) -> Stats {
    let mut sorted = times.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = |q: f64| {
        sorted
            .get(((sorted.len() as f64 - 1.0) * q).round() as usize)
            .copied()
            .unwrap_or(f64::NAN)
    };
    Stats {
        p50: at(0.50),
        p95: at(0.95),
        p99: at(0.99),
        max: at(1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_come_from_the_sorted_times() {
        let times: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        let s = stats(&times);
        assert_eq!((s.p50, s.p95, s.p99, s.max), (51.0, 95.0, 99.0, 100.0));
    }

    #[test]
    fn the_phases_run_in_order_then_idle() {
        let mut bench = Bench::new(PathBuf::from("unused.json"));
        let start = Instant::now();
        let mut seen = Vec::new();
        for i in 0..(WARM_UP + 3 * FRAMES + 2) {
            let now = start + Duration::from_millis(i as u64);
            if let Step::Idle = bench.step(now) {
                seen.push(Phase::Idle);
                break;
            }
            if seen.last() != Some(&bench.phase) {
                seen.push(bench.phase);
            }
        }
        assert_eq!(
            seen,
            [
                Phase::WarmUp,
                Phase::ScrollTime,
                Phase::ScrollTracks,
                Phase::Zoom,
                Phase::Idle
            ]
        );
        // Every measured phase recorded a frame time for each frame.
        for (phase, times) in &bench.times {
            assert_eq!(times.len(), FRAMES, "{phase:?}");
        }
    }
}
