//! The callback benchmark (M6 slice 7c): how long the audio callback takes
//! with a project's plugins running, against the time each block has.
//!
//! `lumen --bench-callback <out.json> <project.lumen>` opens the project,
//! waits for its plugins to load, plays it from the start for
//! `--bench-seconds` (30 by default) after a short warm-up, and samples what
//! the audio layer already measures every 100 ms: the callbacks' mean time,
//! the worst callback, overloads and dropouts, and what each plugin's blocks
//! cost. It writes a report, prints it and exits with status 0 when no block
//! overran, 3 when one did, and 2 when it could not run (a plugin did not
//! load, the audio is off). The buffer size and the device are the ones in
//! the settings, so the numbers are the user's machine at the user's buffer
//! (ADR-0069 asks for 64 and 128 frames). Nothing is sent by this mode.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lumen_audio::BlockTime;

use crate::hosting::PluginCost;

/// How often the audio layer is asked.
pub const WINDOW: Duration = Duration::from_millis(100);
/// Measured before measuring starts: first blocks of a plugin are slow.
pub const WARM_UP: Duration = Duration::from_secs(3);
/// How long plugins may take to load before the run gives up.
pub const LOAD_LIMIT: Duration = Duration::from_secs(120);
/// The quality bar: a callback comfortably under half of its block, and none
/// over three quarters.
const MEAN_BAR: f64 = 0.5;
const WORST_BAR: f64 = 0.75;
/// Frames everything must stay up before the project counts as loaded.
const CALM_FRAMES: u32 = 10;
/// Plugins named in the printed report.
const SHOWN: usize = 8;

/// Where the run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for the project and its plugins.
    Loading,
    /// Playing, not yet measured.
    WarmUp,
    /// Measured.
    Measuring,
    Done,
}

/// What one plugin's blocks cost over the whole run.
#[derive(Debug, Clone, Default)]
struct Plugin {
    name: String,
    place: String,
    blocks: u64,
    answers: u64,
    wait_sum_ns: u128,
    work_sum_ns: u128,
    wait_peak: Duration,
    work_peak: Duration,
    late_at_start: u32,
    late_now: u32,
}

/// Where the numbers came from.
#[derive(Debug, Clone, Default)]
pub struct Setup {
    pub project: String,
    pub driver: String,
    pub device: String,
    pub rate: u32,
    pub frames: u32,
    pub hosts: usize,
}

pub struct Callback {
    out: PathBuf,
    /// End the process with the report's status (tests keep the app alive).
    exit: bool,
    seconds: f64,
    phase: Phase,
    since: Instant,
    next: Instant,
    /// Frames in a row that everything has been up.
    calm: u32,
    /// Mean share of the block, per 100 ms window with callbacks in it.
    windows: Vec<f64>,
    /// Sum of callback time and of block time, in nanoseconds, over all.
    used_ns: u128,
    block_ns: u128,
    callbacks: u64,
    worst: f64,
    plugins: BTreeMap<u64, Plugin>,
}

impl Callback {
    pub fn new(out: PathBuf, seconds: f64, now: Instant) -> Self {
        Self {
            out,
            exit: true,
            seconds: seconds.clamp(1.0, 24.0 * 3600.0),
            phase: Phase::Loading,
            since: now,
            next: now,
            calm: 0,
            windows: Vec::new(),
            used_ns: 0,
            block_ns: 0,
            callbacks: 0,
            worst: 0.0,
            plugins: BTreeMap::new(),
        }
    }

    /// The app stays open after the report (tests).
    pub fn keeping_alive(mut self) -> Self {
        self.exit = false;
        self
    }

    pub fn exits(&self) -> bool {
        self.exit
    }

    pub fn out(&self) -> &Path {
        &self.out
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether things have been `up` for long enough, in frames, to be
    /// believed: a plugin shows as missing for a frame or two before its host
    /// is asked for.
    pub fn settled(&mut self, up: bool) -> bool {
        self.calm = if up { self.calm + 1 } else { 0 };
        self.calm >= CALM_FRAMES
    }

    /// Whether loading has taken too long.
    pub fn load_expired(&self, now: Instant) -> bool {
        self.phase == Phase::Loading && now.duration_since(self.since) > LOAD_LIMIT
    }

    /// Everything is loaded and the project is playing: the warm-up begins.
    pub fn play_started(&mut self, now: Instant) {
        self.phase = Phase::WarmUp;
        self.since = now;
    }

    /// Whether the warm-up is over.
    pub fn warmed(&self, now: Instant) -> bool {
        self.phase == Phase::WarmUp && now.duration_since(self.since) >= WARM_UP
    }

    /// Measuring begins. `costs` are what the plugins' counters held just
    /// now (they count from here).
    pub fn measure_started(&mut self, now: Instant, costs: &[PluginCost]) {
        self.phase = Phase::Measuring;
        self.since = now;
        self.next = now + WINDOW;
        for cost in costs {
            let plugin = self.plugins.entry(cost.device.0).or_default();
            plugin.name.clone_from(&cost.name);
            plugin.place.clone_from(&cost.place);
            plugin.late_at_start = cost.late;
            plugin.late_now = cost.late;
        }
    }

    /// Whether a sample is due, or the measuring is over.
    pub fn due(&self, now: Instant) -> bool {
        self.phase == Phase::Measuring && now >= self.next
    }

    pub fn over(&self, now: Instant) -> bool {
        self.phase == Phase::Measuring
            && now.duration_since(self.since).as_secs_f64() >= self.seconds
    }

    pub fn next_in(&self, now: Instant) -> Duration {
        self.next.saturating_duration_since(now)
    }

    /// Takes one window: the callbacks' times, the worst callback's share
    /// of its block, and what the plugins' blocks cost.
    pub fn sample(&mut self, now: Instant, time: BlockTime, worst: f32, costs: &[PluginCost]) {
        self.next = now + WINDOW;
        if time.blocks > 0 && !time.block.is_zero() {
            self.windows
                .push(time.used.as_secs_f64() / time.block.as_secs_f64());
            let blocks = u128::from(time.blocks);
            self.used_ns += time.used.as_nanos() * blocks;
            self.block_ns += time.block.as_nanos() * blocks;
            self.callbacks += u64::from(time.blocks);
        }
        self.worst = self.worst.max(f64::from(worst));
        for cost in costs {
            let plugin = self.plugins.entry(cost.device.0).or_default();
            plugin.name.clone_from(&cost.name);
            plugin.place.clone_from(&cost.place);
            let (blocks, answers) = (u64::from(cost.cost.blocks), u64::from(cost.cost.answers));
            plugin.blocks += blocks;
            plugin.answers += answers;
            plugin.wait_sum_ns += cost.cost.wait_mean.as_nanos() * u128::from(blocks);
            plugin.work_sum_ns += cost.cost.work_mean.as_nanos() * u128::from(answers);
            plugin.wait_peak = plugin.wait_peak.max(cost.cost.wait_peak);
            plugin.work_peak = plugin.work_peak.max(cost.cost.work_peak);
            plugin.late_now = cost.late;
        }
    }

    pub fn finish(&mut self, setup: Setup, overloads: u64, dropouts: u64) -> Report {
        self.phase = Phase::Done;
        let mut sorted = self.windows.clone();
        sorted.sort_by(f64::total_cmp);
        let at = |share: f64| {
            sorted
                .get(((sorted.len().saturating_sub(1)) as f64 * share).round() as usize)
                .copied()
                .unwrap_or(0.0)
        };
        let mean = if self.block_ns == 0 {
            0.0
        } else {
            self.used_ns as f64 / self.block_ns as f64
        };
        let mut plugins: Vec<PluginReport> = self
            .plugins
            .values()
            .map(|p| PluginReport {
                name: p.name.clone(),
                place: p.place.clone(),
                blocks: p.blocks,
                wait_mean: mean_of(p.wait_sum_ns, p.blocks),
                wait_peak: p.wait_peak,
                work_mean: mean_of(p.work_sum_ns, p.answers),
                work_peak: p.work_peak,
                late: p.late_now.saturating_sub(p.late_at_start),
            })
            .collect();
        plugins.sort_by_key(|p| std::cmp::Reverse(p.wait_mean));
        Report {
            setup,
            seconds: self.seconds,
            callbacks: self.callbacks,
            mean,
            median: at(0.5),
            p99: at(0.99),
            worst: self.worst,
            overloads,
            dropouts,
            plugins,
        }
    }
}

fn mean_of(sum_ns: u128, count: u64) -> Duration {
    if count == 0 {
        return Duration::ZERO;
    }
    Duration::from_nanos(u64::try_from(sum_ns / u128::from(count)).unwrap_or(u64::MAX))
}

#[derive(Debug, Clone)]
pub struct PluginReport {
    pub name: String,
    pub place: String,
    pub blocks: u64,
    pub wait_mean: Duration,
    pub wait_peak: Duration,
    pub work_mean: Duration,
    pub work_peak: Duration,
    pub late: u32,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub setup: Setup,
    pub seconds: f64,
    pub callbacks: u64,
    /// Shares of the block's time (1.0 is all of it).
    pub mean: f64,
    pub median: f64,
    pub p99: f64,
    pub worst: f64,
    pub overloads: u64,
    pub dropouts: u64,
    pub plugins: Vec<PluginReport>,
}

impl Report {
    /// No block overran and nothing was lost.
    pub fn pass(&self) -> bool {
        self.overloads == 0 && self.dropouts == 0 && self.callbacks > 0
    }

    /// The quality bar's wording: processing comfortably under half of the
    /// time budget.
    pub fn quality_bar(&self) -> bool {
        self.pass() && self.mean < MEAN_BAR && self.worst < WORST_BAR
    }

    pub fn text(&self) -> String {
        let s = &self.setup;
        let block_ms = f64::from(s.frames) * 1000.0 / f64::from(s.rate.max(1));
        let percent = |share: f64| format!("{:.1}%", share * 100.0);
        let ms = |d: Duration| format!("{:.3} ms", d.as_secs_f64() * 1000.0);
        let mut text = format!(
            "Callback benchmark: {} on {} ({}), {} Hz, {} frames ({block_ms:.2} ms a block), {} plugin hosts\n\
             {:.0} s measured, {} callbacks\n\
             callback time, share of the block: mean {}, median {}, p99 of 100 ms windows {}, worst callback {}\n\
             overloads {}, dropouts {}\n",
            s.project,
            s.device,
            s.driver,
            s.rate,
            s.frames,
            s.hosts,
            self.seconds,
            self.callbacks,
            percent(self.mean),
            percent(self.median),
            percent(self.p99),
            percent(self.worst),
            self.overloads,
            self.dropouts
        );
        for p in self.plugins.iter().take(SHOWN) {
            text.push_str(&format!(
                "  {} · {}: wait {} mean {} peak, work {} mean {} peak, {} late of {} blocks\n",
                p.place,
                p.name,
                ms(p.wait_mean),
                ms(p.wait_peak),
                ms(p.work_mean),
                ms(p.work_peak),
                p.late,
                p.blocks
            ));
        }
        if self.plugins.len() > SHOWN {
            text.push_str(&format!("  and {} more\n", self.plugins.len() - SHOWN));
        }
        text.push_str(&format!(
            "no block overran and nothing was lost: {}\n\
             quality bar (mean under {:.0}%, worst under {:.0}%): {}\n",
            yes(self.pass()),
            MEAN_BAR * 100.0,
            WORST_BAR * 100.0,
            yes(self.quality_bar())
        ));
        text
    }

    pub fn json(&self) -> String {
        let s = &self.setup;
        let plugin = |p: &PluginReport| {
            serde_json::json!({
                "place": p.place, "name": p.name, "blocks": p.blocks,
                "wait_mean_us": p.wait_mean.as_micros() as u64,
                "wait_peak_us": p.wait_peak.as_micros() as u64,
                "work_mean_us": p.work_mean.as_micros() as u64,
                "work_peak_us": p.work_peak.as_micros() as u64,
                "late": p.late,
            })
        };
        let value = serde_json::json!({
            "project": s.project, "driver": s.driver, "device": s.device,
            "rate": s.rate, "frames": s.frames, "hosts": s.hosts,
            "seconds": self.seconds, "callbacks": self.callbacks,
            "load": {
                "mean": self.mean, "median": self.median,
                "p99": self.p99, "worst": self.worst,
            },
            "overloads": self.overloads, "dropouts": self.dropouts,
            "pass": self.pass(), "quality_bar": self.quality_bar(),
            "plugins": self.plugins.iter().map(plugin).collect::<Vec<_>>(),
        });
        serde_json::to_string_pretty(&value).unwrap_or_default()
    }
}

fn yes(met: bool) -> &'static str {
    if met { "yes" } else { "NO" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_devices::Cost;
    use lumen_project::model::Id;

    fn time(blocks: u32, used_us: u64, block_us: u64) -> BlockTime {
        BlockTime {
            blocks,
            used: Duration::from_micros(used_us),
            block: Duration::from_micros(block_us),
        }
    }

    fn cost(device: u64, wait_us: u64, late: u32) -> PluginCost {
        PluginCost {
            device: Id(device),
            name: format!("Plugin {device}"),
            place: format!("Track {device} · instrument"),
            cost: Cost {
                blocks: 10,
                answers: 10,
                work_mean: Duration::from_micros(wait_us / 2),
                work_peak: Duration::from_micros(wait_us),
                wait_mean: Duration::from_micros(wait_us),
                wait_peak: Duration::from_micros(wait_us * 2),
            },
            late,
        }
    }

    fn run(f: impl FnOnce(&mut Callback, Instant)) -> Report {
        let start = Instant::now();
        let mut bench = Callback::new("out.json".into(), 10.0, start);
        bench.play_started(start);
        bench.measure_started(start, &[cost(1, 100, 4)]);
        f(&mut bench, start);
        bench.finish(
            Setup {
                project: "p".into(),
                rate: 48_000,
                frames: 128,
                ..Setup::default()
            },
            0,
            0,
        )
    }

    #[test]
    fn the_mean_is_weighted_by_blocks_and_the_percentiles_are_of_windows() {
        let report = run(|bench, start| {
            // 100 windows of 10% and one of 80% with a tenth of the blocks.
            for _ in 0..100 {
                bench.sample(start, time(37, 267, 2_667), 0.2, &[]);
            }
            bench.sample(start, time(4, 2_133, 2_667), 0.9, &[]);
        });
        assert!((report.median - 0.1).abs() < 0.001, "{}", report.median);
        assert!(
            (report.p99 - 0.1).abs() < 0.001,
            "the one slow window is not p99"
        );
        assert!((report.worst - 0.9).abs() < 1e-6);
        // 3700 blocks at 10% and 4 at 80%.
        let expected = (3700.0 * 0.1 + 4.0 * 0.8) / 3704.0;
        assert!((report.mean - expected).abs() < 0.001, "{}", report.mean);
        assert_eq!(report.callbacks, 3704);
    }

    #[test]
    fn windows_with_no_callbacks_count_for_nothing() {
        let report = run(|bench, start| {
            bench.sample(start, BlockTime::default(), 0.0, &[]);
            bench.sample(start, time(10, 500, 2_667), 0.3, &[]);
        });
        assert_eq!(report.callbacks, 10);
        assert!((report.median - 500.0 / 2667.0).abs() < 1e-6);
    }

    #[test]
    fn a_plugin_is_summed_over_the_run_and_late_blocks_count_from_the_start() {
        let report = run(|bench, start| {
            bench.sample(
                start,
                time(10, 500, 2_667),
                0.1,
                &[cost(1, 100, 6), cost(2, 300, 0)],
            );
            bench.sample(
                start,
                time(10, 500, 2_667),
                0.1,
                &[cost(1, 200, 9), cost(2, 500, 0)],
            );
        });
        // Costliest wait first.
        assert_eq!(report.plugins[0].name, "Plugin 2");
        assert_eq!(report.plugins[0].wait_mean, Duration::from_micros(400));
        assert_eq!(report.plugins[0].wait_peak, Duration::from_micros(1_000));
        assert_eq!(report.plugins[0].blocks, 20);
        // It was 4 late blocks when measuring began and 9 at the end.
        let one = report
            .plugins
            .iter()
            .find(|p| p.name == "Plugin 1")
            .unwrap();
        assert_eq!(one.late, 5);
    }

    #[test]
    fn a_run_passes_only_if_nothing_overran_and_something_played() {
        let mut report = run(|bench, start| {
            bench.sample(start, time(10, 500, 2_667), 0.2, &[]);
        });
        assert!(report.pass() && report.quality_bar());
        report.overloads = 1;
        assert!(!report.pass() && !report.quality_bar());
        report.overloads = 0;
        report.worst = 0.8;
        assert!(report.pass(), "a callback at 80% did not overrun");
        assert!(!report.quality_bar());
        let empty = run(|_, _| {});
        assert!(!empty.pass(), "no callbacks is not a pass");
    }

    #[test]
    fn loading_is_believed_only_after_a_run_of_calm_frames() {
        let mut bench = Callback::new("out.json".into(), 5.0, Instant::now());
        for _ in 0..CALM_FRAMES - 1 {
            assert!(!bench.settled(true));
        }
        assert!(!bench.settled(false), "a frame that is not up starts again");
        for _ in 0..CALM_FRAMES - 1 {
            assert!(!bench.settled(true));
        }
        assert!(bench.settled(true));
    }

    #[test]
    fn the_phases_follow_the_clock() {
        let start = Instant::now();
        let mut bench = Callback::new("out.json".into(), 5.0, start);
        assert!(!bench.load_expired(start + Duration::from_secs(60)));
        assert!(bench.load_expired(start + LOAD_LIMIT + Duration::from_secs(1)));
        bench.play_started(start);
        assert!(!bench.warmed(start + WARM_UP - Duration::from_millis(1)));
        assert!(bench.warmed(start + WARM_UP));
        let t0 = start + WARM_UP;
        bench.measure_started(t0, &[]);
        assert!(!bench.due(t0 + WINDOW - Duration::from_millis(1)));
        assert!(bench.due(t0 + WINDOW));
        assert!(!bench.over(t0 + Duration::from_secs(4)));
        assert!(bench.over(t0 + Duration::from_secs(5)));
    }

    #[test]
    fn the_report_reads_and_the_json_holds_the_same_numbers() {
        let report = run(|bench, start| {
            bench.sample(start, time(10, 500, 2_667), 0.2, &[cost(1, 100, 4)]);
        });
        let text = report.text();
        assert!(text.contains("128 frames (2.67 ms a block)"), "{text}");
        assert!(text.contains("Track 1 · instrument · Plugin 1"), "{text}");
        assert!(text.contains("quality bar (mean under 50%, worst under 75%): yes"));
        let json: serde_json::Value = serde_json::from_str(&report.json()).unwrap();
        assert_eq!(json["frames"], 128);
        assert_eq!(json["pass"], true);
        assert_eq!(json["plugins"][0]["name"], "Plugin 1");
    }
}
