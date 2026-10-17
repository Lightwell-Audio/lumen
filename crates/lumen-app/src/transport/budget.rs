//! The audio panel's block budget (ADR-0075): where each audio block's time
//! goes. Lumen's own work, then each plugin: the part of the wait the plugin
//! spent working, and the rest of the wait, the trip between processes. Taken
//! once a second from the audio status and the hosts, then drawn by
//! `budget_view`.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use lumen_audio::BlockTime;
use lumen_project::model::Id;

use crate::hosting::PluginCost;

/// How often the numbers are taken: they are means over this stretch.
const EVERY: Duration = Duration::from_secs(1);

/// Missed blocks are counted over this window.
pub const MISS_WINDOW: Duration = Duration::from_secs(60);

/// One plugin's share of an average block.
#[derive(Debug, Clone, PartialEq)]
pub struct Share {
    pub device: Id,
    pub name: String,
    pub place: String,
    /// The plugin's own work per audio block.
    pub work: Duration,
    /// The audio thread's wait for it per block. An instrument handed its
    /// block early works while Lumen does, so its wait can be shorter than
    /// its work.
    pub waited: Duration,
    /// Blocks it missed within [`MISS_WINDOW`].
    pub missed: u32,
}

impl Share {
    /// The part of the wait the plugin was working: the bar's solid part.
    pub fn working(&self) -> Duration {
        self.work.min(self.waited)
    }

    /// The rest of the wait: the trip between processes, the host being
    /// woken. The bar's hatched part.
    pub fn trip(&self) -> Duration {
        self.waited.saturating_sub(self.working())
    }
}

/// An average block of the last stretch.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// How long a block lasts: the time each callback has.
    pub block: Duration,
    /// How long a callback took.
    pub used: Duration,
    /// When plugins must have answered, from the callback's start.
    pub deadline: Duration,
    /// The callback's time not spent waiting for plugins.
    pub lumen: Duration,
    /// Every running plugin, the costliest first.
    pub plugins: Vec<Share>,
}

impl Snapshot {
    /// Plugins that missed blocks within the window.
    pub fn missing(&self) -> usize {
        self.plugins.iter().filter(|p| p.missed > 0).count()
    }
}

/// `per` blocks of `mean` each, spread over `blocks` callbacks.
fn spread(mean: Duration, per: u32, blocks: u32) -> Duration {
    if blocks == 0 {
        return Duration::ZERO;
    }
    mean.mul_f64(f64::from(per) / f64::from(blocks))
}

/// The average block of a stretch: `time` from the audio status, `costs`
/// from the hosts, `missed` each plugin's misses in the window. `None` while
/// no audio ran.
pub fn snapshot(
    time: BlockTime,
    costs: &[PluginCost],
    missed: impl Fn(Id) -> u32,
) -> Option<Snapshot> {
    if time.blocks == 0 {
        return None;
    }
    let mut plugins: Vec<Share> = costs
        .iter()
        .map(|c| Share {
            device: c.device,
            name: c.name.clone(),
            place: c.place.clone(),
            work: spread(c.cost.work_mean, c.cost.answers, time.blocks),
            waited: spread(c.cost.wait_mean, c.cost.blocks, time.blocks),
            missed: missed(c.device),
        })
        .collect();
    // Stable: equal plugins keep the project's order.
    plugins.sort_by(|a, b| b.waited.cmp(&a.waited).then(b.work.cmp(&a.work)));
    let waited: Duration = plugins.iter().map(|p| p.waited).sum();
    let (part, whole) = lumen_engine::HOST_SHARE;
    Some(Snapshot {
        block: time.block,
        used: time.used,
        deadline: time.block.mul_f64(part as f64 / whole as f64),
        lumen: time.used.saturating_sub(waited),
        plugins,
    })
}

/// One plugin's missed-block total over time, to count the window's.
#[derive(Debug, Default)]
struct Misses {
    /// (when, total) samples; the first is the window's base.
    log: VecDeque<(Instant, u32)>,
}

impl Misses {
    /// Records the total at `now` and returns the misses in the window.
    fn note(&mut self, now: Instant, total: u32) -> u32 {
        if self.log.back().is_some_and(|&(_, last)| total < last) {
            // A new host counts from zero.
            self.log.clear();
        }
        if self.log.is_empty() {
            // A host is first seen within a stretch of starting: its misses
            // so far are in the window.
            self.log.push_back((now, 0));
        }
        self.log.push_back((now, total));
        while self.log.len() > 2 && now.duration_since(self.log[1].0) >= MISS_WINDOW {
            self.log.pop_front();
        }
        total - self.log.front().map_or(0, |&(_, base)| base)
    }
}

/// The budget the panel shows, refreshed once a second.
#[derive(Debug, Default)]
pub struct Budget {
    taken: Option<Instant>,
    misses: HashMap<Id, Misses>,
    pub shown: Option<Snapshot>,
    /// The plugin row under the pointer, last frame: its segment is lit.
    pub hover: Option<usize>,
    /// The height the plugin list may take this frame: what the window
    /// leaves after the rest of the panel, at most `size::BUDGET_LIST`.
    pub list_room: f32,
    /// The list's height and the rest of the panel's, last frame.
    pub list_height: f32,
    pub outside: f32,
}

impl Budget {
    /// Whether a second has passed since the numbers were last taken.
    pub fn due(&self, now: Instant) -> bool {
        self.taken.is_none_or(|t| now.duration_since(t) >= EVERY)
    }

    /// Shows `snapshot` and holds it for an hour: screenshots.
    pub fn pretend(&mut self, snapshot: Snapshot) {
        self.taken = Some(Instant::now() + Duration::from_secs(3600));
        self.shown = Some(snapshot);
    }

    /// Takes a stretch's numbers.
    pub fn update(&mut self, now: Instant, time: BlockTime, costs: &[PluginCost]) {
        self.taken = Some(now);
        self.misses
            .retain(|device, _| costs.iter().any(|c| c.device == *device));
        let missed: HashMap<Id, u32> = costs
            .iter()
            .map(|c| {
                let log = self.misses.entry(c.device).or_default();
                (c.device, log.note(now, c.late))
            })
            .collect();
        self.shown = snapshot(time, costs, |d| missed.get(&d).copied().unwrap_or(0));
    }
}

#[cfg(test)]
mod tests {
    use lumen_devices::Cost;

    use super::*;

    const US: fn(u64) -> Duration = Duration::from_micros;

    fn plugin(id: u64, work: u64, wait: u64, blocks: u32) -> PluginCost {
        PluginCost {
            device: Id(id),
            name: format!("Plugin {id}"),
            place: "Track · effect 1".into(),
            cost: Cost {
                blocks,
                answers: blocks,
                work_mean: US(work),
                work_peak: US(work),
                wait_mean: US(wait),
                wait_peak: US(wait),
            },
            late: 0,
        }
    }

    fn time(blocks: u32, used: u64) -> BlockTime {
        BlockTime {
            blocks,
            used: US(used),
            block: US(2_900),
        }
    }

    #[test]
    fn plugins_are_spread_over_every_block_and_lumen_is_the_rest() {
        // A synth that waited 500 µs every block, an effect waited 800 µs
        // on one block in two.
        let costs = [plugin(2, 800, 800, 50), plugin(1, 300, 500, 100)];
        let s = snapshot(time(100, 1_700), &costs, |_| 0).unwrap();
        assert_eq!(
            s.plugins[0].device,
            Id(1),
            "costliest first: 500 µs a block"
        );
        assert_eq!(s.plugins[1].waited, US(400));
        assert_eq!(s.plugins[1].work, US(400));
        assert_eq!(s.lumen, US(800), "1,700 less the two waits");
        assert_eq!(s.deadline, US(2_175), "three quarters of the block");
    }

    #[test]
    fn the_wait_is_split_into_work_and_trip_and_early_work_is_no_trip() {
        let costs = [plugin(1, 300, 500, 10), plugin(2, 900, 200, 10)];
        let s = snapshot(time(10, 1_000), &costs, |_| 0).unwrap();
        let effect = &s.plugins[0];
        assert_eq!((effect.working(), effect.trip()), (US(300), US(200)));
        // An instrument handed its block early: its work overlapped Lumen's.
        let early = &s.plugins[1];
        assert_eq!((early.working(), early.trip()), (US(200), Duration::ZERO));
        assert_eq!(early.work, US(900), "the list still shows its work");
    }

    #[test]
    fn nothing_is_shown_while_no_audio_runs() {
        assert_eq!(snapshot(time(0, 0), &[plugin(1, 1, 1, 0)], |_| 0), None);
    }

    #[test]
    fn misses_count_over_the_last_minute_and_a_new_host_starts_again() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let mut m = Misses::default();
        assert_eq!(m.note(at(0), 2), 2, "misses since the host started");
        assert_eq!(m.note(at(30), 5), 5);
        assert_eq!(m.note(at(61), 5), 3, "the first two are over a minute old");
        assert_eq!(m.note(at(120), 5), 0);
        assert_eq!(m.note(at(121), 1), 1, "a restarted host counts afresh");
    }

    #[test]
    fn the_budget_is_taken_once_a_second_and_forgets_removed_plugins() {
        let now = Instant::now();
        let mut budget = Budget::default();
        assert!(budget.due(now));
        let mut late = plugin(1, 100, 100, 10);
        late.late = 4;
        budget.update(now, time(10, 500), &[late]);
        assert!(!budget.due(now + Duration::from_millis(999)));
        assert!(budget.due(now + EVERY));
        let shown = budget.shown.as_ref().unwrap();
        assert_eq!((shown.missing(), shown.plugins[0].missed), (1, 4));
        budget.update(now + EVERY, time(10, 500), &[]);
        assert!(budget.misses.is_empty());
        assert!(budget.shown.as_ref().unwrap().plugins.is_empty());
    }
}
