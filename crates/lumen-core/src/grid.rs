//! The editing grid (M3 slice 5a): where positions snap and where grid
//! lines fall. One grid serves every place a position is chosen (the ruler,
//! dropped files, and later clip and note edits), so snapping behaves the
//! same everywhere.
//!
//! As in Ableton, the grid is **adaptive** (the finest musical step that is
//! at least a set width on screen, in five widths) or **fixed** (8 bars down
//! to 1/64), straight, triplet or dotted, and snapping can be switched off.
//! Steps finer than a bar are counted from each bar's start, so they stay
//! right across time-signature changes.

use serde::{Deserialize, Serialize};

use crate::meter::{Meter, MeterMap};
use crate::time::{PPQ, Ticks};

/// How wide an adaptive grid's steps are on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Width {
    Narrowest,
    Narrow,
    Medium,
    Wide,
    Widest,
}

impl Width {
    const ALL: [Width; 5] = [
        Width::Narrowest,
        Width::Narrow,
        Width::Medium,
        Width::Wide,
        Width::Widest,
    ];

    /// The narrowest a step may be, in points.
    pub fn min_points(self) -> f64 {
        match self {
            Width::Narrowest => 6.0,
            Width::Narrow => 12.0,
            Width::Medium => 20.0,
            Width::Wide => 36.0,
            Width::Widest => 64.0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Width::Narrowest => "Narrowest",
            Width::Narrow => "Narrow",
            Width::Medium => "Medium",
            Width::Wide => "Wide",
            Width::Widest => "Widest",
        }
    }

    pub fn all() -> [Width; 5] {
        Self::ALL
    }
}

/// A fixed grid's step: whole bars, or a note value (`Note(16)` is 1/16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Bars(u32),
    Note(u32),
}

impl Size {
    /// The fixed sizes, coarsest first.
    const LADDER: [Size; 10] = [
        Size::Bars(8),
        Size::Bars(4),
        Size::Bars(2),
        Size::Bars(1),
        Size::Note(2),
        Size::Note(4),
        Size::Note(8),
        Size::Note(16),
        Size::Note(32),
        Size::Note(64),
    ];

    pub fn all() -> [Size; 10] {
        Self::LADDER
    }

    pub fn name(self) -> String {
        match self {
            Size::Bars(1) => "1 bar".into(),
            Size::Bars(n) => format!("{n} bars"),
            Size::Note(n) => format!("1/{n}"),
        }
    }
}

/// Straight, triplet (two thirds of the step) or dotted (one and a half).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feel {
    #[default]
    Straight,
    Triplet,
    Dotted,
}

/// The grid as the user set it. Both the adaptive width and the fixed size
/// are kept, so switching between them returns to where each was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grid {
    pub fixed: bool,
    pub width: Width,
    pub size: Size,
    pub feel: Feel,
    /// Positions snap to the grid (Ctrl+4 turns it off; Alt bypasses it
    /// for one gesture).
    pub snap: bool,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            fixed: false,
            width: Width::Narrow,
            size: Size::Note(16),
            feel: Feel::Straight,
            snap: true,
        }
    }
}

/// A grid step: every `n` bars, or a length counted from each bar's start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Bars(u32),
    Ticks(Ticks),
}

/// How strong a grid line is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Bar,
    Beat,
    Sub,
}

/// A grid line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub at: Ticks,
    pub level: Level,
}

/// Lines are never drawn closer than this (points)...
pub const MIN_LINE_POINTS: f64 = 6.0;
/// ...and a grid finer than that shows lines this far apart instead (as
/// the Narrow adaptive grid), still snapping to its own steps.
pub const FALLBACK_LINE_POINTS: f64 = 12.0;

/// The longest bar step an adaptive grid uses.
const MAX_BARS: u32 = 1 << 16;

/// `1/n` of a whole note with `feel`, if it is whole ticks.
fn note(n: u32, feel: Feel) -> Option<Ticks> {
    let (num, den) = match feel {
        Feel::Straight => (1, i64::from(n)),
        Feel::Triplet => (2, 3 * i64::from(n)),
        Feel::Dotted => (3, 2 * i64::from(n)),
    };
    Ticks::whole_note_fraction(num, den)
}

impl Grid {
    /// The step for `meter` at a zoom of `points_per_quarter`.
    pub fn step(&self, meter: Meter, points_per_quarter: f64) -> Step {
        if self.fixed {
            return match self.size {
                Size::Bars(n) => Step::Bars(n.max(1)),
                Size::Note(n) => note(n, self.feel).map_or(Step::Bars(1), Step::Ticks),
            };
        }
        adaptive(
            meter,
            points_per_quarter,
            self.width.min_points(),
            self.feel,
        )
    }

    /// `t` moved to the nearest grid line, or `t` itself with snapping off.
    pub fn snap(&self, map: &MeterMap, t: Ticks, points_per_quarter: f64) -> Ticks {
        if !self.snap {
            return t;
        }
        let bar = map.bbt_at(t).bar;
        let start = map.bar_start(bar);
        let meter = map.meter_at(start);
        let (before, after) = match self.step(meter, points_per_quarter) {
            Step::Bars(n) => {
                let first = bar - bar.rem_euclid(i64::from(n));
                (map.bar_start(first), map.bar_start(first + i64::from(n)))
            }
            Step::Ticks(step) => {
                let next = map.bar_start(bar + 1);
                let k = (t - start).0.div_euclid(step.0);
                let before = start + Ticks(k * step.0);
                (before, (before + step).min(next))
            }
        };
        if (t - before).0 <= (after - t).0 {
            before
        } else {
            after
        }
    }

    /// The lines to draw in `from..to` (at most a few screens' worth):
    /// this grid's steps when they are at least [`MIN_LINE_POINTS`] apart,
    /// otherwise the finest straight steps that are.
    pub fn lines(
        &self,
        map: &MeterMap,
        from: Ticks,
        to: Ticks,
        points_per_quarter: f64,
    ) -> Vec<Line> {
        let mut lines = Vec::new();
        let mut bar = map.bbt_at(from.max(Ticks::ZERO)).bar;
        loop {
            let start = map.bar_start(bar);
            if start >= to {
                break;
            }
            let meter = map.meter_at(start);
            match self.display_step(meter, points_per_quarter) {
                Step::Bars(n) => {
                    let n = i64::from(n);
                    if bar.rem_euclid(n) == 0 && start >= from {
                        lines.push(Line {
                            at: start,
                            level: Level::Bar,
                        });
                    }
                    bar += n - bar.rem_euclid(n);
                }
                Step::Ticks(step) => {
                    let next = map.bar_start(bar + 1);
                    let beat = meter.beat().0;
                    let mut at = start;
                    while at < next && at < to {
                        if at >= from {
                            let into = (at - start).0;
                            let level = if into == 0 {
                                Level::Bar
                            } else if into % beat == 0 {
                                Level::Beat
                            } else {
                                Level::Sub
                            };
                            lines.push(Line { at, level });
                        }
                        at = at + step;
                    }
                    bar += 1;
                }
            }
        }
        lines
    }

    /// The step lines are drawn at in `meter`: this grid's step when it is
    /// at least [`MIN_LINE_POINTS`] wide, otherwise the finest straight step
    /// at least [`FALLBACK_LINE_POINTS`] wide. The lines change only when
    /// this does.
    pub fn display_step(&self, meter: Meter, points_per_quarter: f64) -> Step {
        let step = self.step(meter, points_per_quarter);
        if points(step, meter, points_per_quarter) < MIN_LINE_POINTS {
            adaptive(
                meter,
                points_per_quarter,
                FALLBACK_LINE_POINTS,
                Feel::Straight,
            )
        } else {
            step
        }
    }

    /// One step finer (Ctrl+1): a narrower width, or a smaller size.
    pub fn narrower(&mut self) {
        if self.fixed {
            self.size = neighbour(&Size::LADDER, self.size, 1);
        } else {
            self.width = neighbour(&Width::ALL, self.width, -1);
        }
    }

    /// One step coarser (Ctrl+2).
    pub fn wider(&mut self) {
        if self.fixed {
            self.size = neighbour(&Size::LADDER, self.size, -1);
        } else {
            self.width = neighbour(&Width::ALL, self.width, 1);
        }
    }

    /// Triplet on or off (Ctrl+3).
    pub fn toggle_triplet(&mut self) {
        self.feel = if self.feel == Feel::Triplet {
            Feel::Straight
        } else {
            Feel::Triplet
        };
    }

    /// Fixed or adaptive (Ctrl+5). Turning fixed takes the size closest to
    /// the step showing now (`shown`), as Ableton does.
    pub fn toggle_fixed(&mut self, shown: Step) {
        if !self.fixed {
            self.size = Size::nearest(shown, self.feel);
        }
        self.fixed = !self.fixed;
    }

    /// What the grid is, for the screen: "1/16", "1/8T", "1/8.", "4 bars",
    /// or "Off".
    pub fn label(&self, step: Step) -> String {
        if !self.snap {
            return "Off".into();
        }
        step_label(step, self.feel)
    }
}

impl Size {
    /// The fixed size nearest `step` (a step made with `feel`).
    fn nearest(step: Step, feel: Feel) -> Size {
        match step {
            Step::Bars(n) => Size::LADDER
                .iter()
                .copied()
                .find(|s| matches!(s, Size::Bars(b) if *b <= n))
                .unwrap_or(Size::Bars(1)),
            Step::Ticks(t) => Size::LADDER
                .iter()
                .copied()
                .filter_map(|s| match s {
                    Size::Note(n) => note(n, feel).map(|len| (s, (len.0 - t.0).abs())),
                    Size::Bars(_) => None,
                })
                .min_by_key(|&(_, distance)| distance)
                .map_or(Size::Note(16), |(s, _)| s),
        }
    }
}

/// The name of `step`: "1/16", "1/8T", "1/8.", "1 bar", "4 bars".
pub fn step_label(step: Step, feel: Feel) -> String {
    match step {
        Step::Bars(1) => "1 bar".into(),
        Step::Bars(n) => format!("{n} bars"),
        Step::Ticks(t) => {
            // The note value this step was made from.
            let (straight, mark) = match feel {
                Feel::Straight => (t.0, ""),
                Feel::Triplet => (t.0 * 3 / 2, "T"),
                Feel::Dotted => (t.0 * 2 / 3, "."),
            };
            if straight > 0 && (4 * PPQ) % straight == 0 {
                format!("1/{}{mark}", 4 * PPQ / straight)
            } else {
                format!("{:.3} beats", t.as_quarters())
            }
        }
    }
}

/// How far apart `step` is on screen, in points (for bars, in this meter).
fn points(step: Step, meter: Meter, points_per_quarter: f64) -> f64 {
    match step {
        Step::Bars(n) => f64::from(n) * meter.bar().as_quarters() * points_per_quarter,
        Step::Ticks(t) => t.as_quarters() * points_per_quarter,
    }
}

/// The finest step at least `min` points wide: a note value from 1/64 up
/// to (not including) a bar, then 1, 2, 4... bars. Straight note values
/// must divide the bar, so lines meet every bar line.
fn adaptive(meter: Meter, points_per_quarter: f64, min: f64, feel: Feel) -> Step {
    let bar = meter.bar();
    for n in [64, 32, 16, 8, 4, 2, 1] {
        let Some(step) = note(n, feel) else { continue };
        if step >= bar || (feel == Feel::Straight && bar.0 % step.0 != 0) {
            continue;
        }
        if step.as_quarters() * points_per_quarter >= min {
            return Step::Ticks(step);
        }
    }
    let mut n = 1;
    while n < MAX_BARS && f64::from(n) * bar.as_quarters() * points_per_quarter < min {
        n *= 2;
    }
    Step::Bars(n)
}

/// The item `by` places from `current` in `all`, held at the ends.
fn neighbour<T: Copy + PartialEq>(all: &[T], current: T, by: isize) -> T {
    let at = all.iter().position(|x| *x == current).unwrap_or(0) as isize;
    all[(at + by).clamp(0, all.len() as isize - 1) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meter::MeterPoint;

    const FOUR: Meter = Meter::FOUR_FOUR;

    fn sixteenth() -> Ticks {
        Ticks(PPQ / 4)
    }

    fn map() -> MeterMap {
        MeterMap::constant(FOUR).unwrap()
    }

    #[test]
    fn the_adaptive_grid_follows_the_zoom_and_the_width() {
        let grid = Grid::default(); // Narrow: 12 points.
        // 24 points a quarter: eighths are 12.
        assert_eq!(grid.step(FOUR, 24.0), Step::Ticks(Ticks(PPQ / 2)));
        // Zoomed in: down to 1/64 and no finer.
        assert_eq!(grid.step(FOUR, 10_000.0), Step::Ticks(Ticks(PPQ / 16)));
        // Zoomed out: a bar, then 2, 4... bars.
        assert_eq!(grid.step(FOUR, 3.0), Step::Bars(1));
        assert_eq!(grid.step(FOUR, 1.0), Step::Bars(4));
        // Wider widths take coarser steps at the same zoom.
        let wide = Grid {
            width: Width::Widest,
            ..grid
        };
        assert_eq!(wide.step(FOUR, 24.0), Step::Bars(1));
    }

    #[test]
    fn triplets_and_dotted_steps() {
        let fixed = |feel| Grid {
            fixed: true,
            size: Size::Note(8),
            feel,
            ..Grid::default()
        };
        assert_eq!(
            fixed(Feel::Straight).step(FOUR, 1.0),
            Step::Ticks(Ticks(PPQ / 2))
        );
        assert_eq!(
            fixed(Feel::Triplet).step(FOUR, 1.0),
            Step::Ticks(Ticks(PPQ / 3))
        );
        assert_eq!(
            fixed(Feel::Dotted).step(FOUR, 1.0),
            Step::Ticks(Ticks(PPQ * 3 / 4))
        );
        assert_eq!(
            fixed(Feel::Triplet).label(fixed(Feel::Triplet).step(FOUR, 1.0)),
            "1/8T"
        );
        assert_eq!(
            fixed(Feel::Dotted).label(fixed(Feel::Dotted).step(FOUR, 1.0)),
            "1/8."
        );
        // An adaptive triplet grid: eighth triplets are 8 points at 24.
        let triplet = Grid {
            feel: Feel::Triplet,
            width: Width::Narrowest,
            ..Grid::default()
        };
        assert_eq!(triplet.step(FOUR, 24.0), Step::Ticks(Ticks(PPQ / 3)));
    }

    #[test]
    fn snapping_goes_to_the_nearest_line_and_can_be_off() {
        let grid = Grid {
            fixed: true,
            size: Size::Note(16),
            ..Grid::default()
        };
        let s = sixteenth().0;
        assert_eq!(grid.snap(&map(), Ticks(s / 2 - 1), 1.0), Ticks::ZERO);
        assert_eq!(grid.snap(&map(), Ticks(s / 2 + 1), 1.0), sixteenth());
        assert_eq!(grid.snap(&map(), Ticks(-5), 1.0), Ticks::ZERO);
        let off = Grid {
            snap: false,
            ..grid
        };
        assert_eq!(off.snap(&map(), Ticks(123), 1.0), Ticks(123));
        // Every 4 bars: bar 3 (of 1-based 1..) goes back to bar 1, bar 4 on to 5.
        let bars = Grid {
            size: Size::Bars(4),
            ..grid
        };
        assert_eq!(bars.snap(&map(), Ticks::quarters(7), 1.0), Ticks::ZERO);
        assert_eq!(
            bars.snap(&map(), Ticks::quarters(9), 1.0),
            Ticks::quarters(16)
        );
    }

    #[test]
    fn dotted_steps_restart_at_each_bar_and_snap_to_the_bar_line() {
        let grid = Grid {
            fixed: true,
            size: Size::Note(4),
            feel: Feel::Dotted,
            ..Grid::default()
        };
        // Dotted quarters in 4/4: 0, 1.5, 3, then the next bar at 4.
        let lines: Vec<Ticks> = grid
            .lines(&map(), Ticks::ZERO, Ticks::quarters(6), 100.0)
            .iter()
            .map(|l| l.at)
            .collect();
        assert_eq!(
            lines,
            [
                Ticks::ZERO,
                Ticks(PPQ * 3 / 2),
                Ticks::quarters(3),
                Ticks::quarters(4),
                Ticks(PPQ * 11 / 2)
            ]
        );
        // Just before the bar line, the bar line is nearer than 3 + 1.5.
        assert_eq!(
            grid.snap(&map(), Ticks(PPQ * 7 / 2 + 1), 100.0),
            Ticks::quarters(4)
        );
    }

    #[test]
    fn steps_count_from_each_bar_across_time_signature_changes() {
        // Two bars of 4/4, then 7/8 (3.5 quarters a bar).
        let map = MeterMap::new(vec![
            MeterPoint {
                at: Ticks::ZERO,
                meter: FOUR,
            },
            MeterPoint {
                at: Ticks::quarters(8),
                meter: Meter {
                    numerator: 7,
                    denominator: 8,
                },
            },
        ])
        .unwrap();
        let grid = Grid {
            fixed: true,
            size: Size::Note(4),
            ..Grid::default()
        };
        // In 7/8, quarters from the bar at 8: 8, 9, 10, 11, then 11.5.
        let lines: Vec<(Ticks, Level)> = grid
            .lines(&map, Ticks::quarters(8), Ticks::quarters(12), 100.0)
            .iter()
            .map(|l| (l.at, l.level))
            .collect();
        assert_eq!(lines[0], (Ticks::quarters(8), Level::Bar));
        assert_eq!(lines[1], (Ticks::quarters(9), Level::Beat));
        assert_eq!(lines[4], (Ticks(PPQ * 23 / 2), Level::Bar));
        assert_eq!(
            grid.snap(&map, Ticks(PPQ * 23 / 2 - 10), 100.0),
            Ticks(PPQ * 23 / 2)
        );
    }

    #[test]
    fn lines_thin_out_when_the_grid_is_finer_than_the_screen() {
        // A fixed 1/64 grid at 1 point a quarter would be 0.06 points apart:
        // the lines shown are every 4 bars (16 points) instead; snapping
        // still uses 1/64.
        let grid = Grid {
            fixed: true,
            size: Size::Note(64),
            ..Grid::default()
        };
        let lines = grid.lines(&map(), Ticks::ZERO, Ticks::quarters(64), 1.0);
        assert!(lines.iter().all(|l| l.level == Level::Bar));
        assert_eq!(
            lines[1].at - lines[0].at,
            Ticks::quarters(16),
            "every 4 bars"
        );
        assert_eq!(grid.snap(&map(), Ticks(PPQ / 16 + 1), 1.0), Ticks(PPQ / 16));
    }

    #[test]
    fn keys_step_the_grid_and_switch_modes() {
        let mut grid = Grid::default();
        grid.narrower();
        assert_eq!(grid.width, Width::Narrowest);
        grid.narrower();
        assert_eq!(grid.width, Width::Narrowest, "held at the end");
        grid.wider();
        grid.wider();
        assert_eq!(grid.width, Width::Medium);
        // Fixed takes the size showing now.
        grid.toggle_fixed(Step::Ticks(Ticks(PPQ / 2)));
        assert!(grid.fixed);
        assert_eq!(grid.size, Size::Note(8));
        grid.wider();
        assert_eq!(grid.size, Size::Note(4));
        grid.toggle_fixed(Step::Bars(1));
        assert!(!grid.fixed);
        assert_eq!(grid.width, Width::Medium, "adaptive returns to its width");
        grid.toggle_triplet();
        assert_eq!(grid.feel, Feel::Triplet);
        assert_eq!(
            Grid {
                snap: false,
                ..grid
            }
            .label(Step::Bars(1)),
            "Off"
        );
    }

    #[test]
    fn the_grid_saves_as_readable_json() {
        let grid = Grid::default();
        let json = serde_json::to_string(&grid).unwrap();
        assert_eq!(
            json,
            r#"{"fixed":false,"width":"narrow","size":{"note":16},"feel":"straight","snap":true}"#
        );
        assert_eq!(serde_json::from_str::<Grid>(&json).unwrap(), grid);
    }
}
