//! Recording a lane (M7 slice A4b): what a track's mode (Read, Touch, Latch,
//! Write) does with the controls that move while the song plays. A pure state
//! machine, so the rules are tested without the interface: the app tells it
//! each frame where the playhead is and what was touched, and it says which
//! lanes to take over and which passes to write.
//!
//! Touch writes while a control is held and hands the lane back when it is let
//! go (the owner's choice: a short glide, so there is no click). Latch starts at
//! the first touch and goes on until Stop. Write goes from where playing begins
//! until Stop. A looped song ends a pass at the wrap and Latch and Write begin
//! the next at once, so each loop is one undo step.

use std::collections::{BTreeMap, BTreeSet};

use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::model::Id;

/// What a track does with the controls that move while it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// The lane plays; a moved control takes it over (A4a).
    #[default]
    Read,
    /// Writes while a control is held.
    Touch,
    /// Writes from the first touch until Stop.
    Latch,
    /// Writes from where playing begins until Stop.
    Write,
}

impl Mode {
    /// The words for the chip and the menu.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Read => "Read",
            Mode::Touch => "Touch",
            Mode::Latch => "Latch",
            Mode::Write => "Write",
        }
    }

    /// What the mode does, for a tip.
    pub fn about(self) -> &'static str {
        match self {
            Mode::Read => "The lane plays; moving a control takes it over",
            Mode::Touch => "Writes while you hold a control, then returns to the lane",
            Mode::Latch => "Writes from your first touch until Stop",
            Mode::Write => "Overwrites the lanes from where playing starts until Stop",
        }
    }

    /// Whether the mode can write a lane.
    pub fn records(self) -> bool {
        self != Mode::Read
    }

    /// The modes in the order a menu lists them.
    pub const ALL: [Mode; 4] = [Mode::Read, Mode::Touch, Mode::Latch, Mode::Write];
}

/// How a pass ends: a control was let go of (glide back to the lane), or the
/// song stopped or wrapped (the lane keeps what was written).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Released,
    Stopped,
}

/// What the recorder asks of the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// The lane stops playing: its control's own value is heard and written.
    TakeOver(Id, Target),
    /// A finished pass, to be written into the lane.
    Write {
        track: Id,
        target: Target,
        raw: Vec<(Ticks, f32)>,
        end: End,
    },
}

/// What the recorder needs to know of the project, a frame at a time.
pub trait World {
    /// The mode of `track`.
    fn mode(&self, track: Id) -> Mode;
    /// Whether a move of `target` on `track` can be written: it has a lane or
    /// it is the lane the track shows.
    fn armed(&self, track: Id, target: Target) -> bool;
    /// Every lane a track in Write mode writes.
    fn write_targets(&self) -> Vec<(Id, Target)>;
    /// The value the control has now.
    fn value(&self, track: Id, target: Target) -> Option<f32>;
}

/// One frame of input.
pub struct Frame<'a> {
    pub playing: bool,
    /// Where the playhead is.
    pub at: Ticks,
    /// A mouse button is down: a control held with it is still held.
    pub pointer_down: bool,
    /// The controls moved since the last frame.
    pub touched: &'a [(Id, Target)],
    /// The plugin gestures that are open (begun, not ended).
    pub open: &'a BTreeSet<(Id, Target)>,
}

/// The passes being recorded.
#[derive(Debug, Default)]
pub struct Recorder {
    passes: BTreeMap<(Id, Target), Vec<(Ticks, f32)>>,
    last: Option<Ticks>,
}

impl Recorder {
    /// The lanes being written now.
    pub fn active(&self) -> impl Iterator<Item = (Id, Target)> + '_ {
        self.passes.keys().copied()
    }

    /// Whether a pass is under way.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    /// Takes one frame in and says what to do.
    pub fn frame(&mut self, f: &Frame<'_>, world: &dyn World) -> Vec<Action> {
        let mut out = Vec::new();
        if !f.playing {
            self.finish_all(End::Stopped, &mut out);
            self.last = None;
            return out;
        }
        // A jump back while playing is a loop wrap: the pass ends there, and
        // those that go on until Stop begin again.
        let wrapped = self.last.is_some_and(|last| f.at < last);
        self.last = Some(f.at);
        let mut again: Vec<(Id, Target)> = Vec::new();
        if wrapped {
            again = self
                .passes
                .keys()
                .copied()
                .filter(|&(t, _)| matches!(world.mode(t), Mode::Latch | Mode::Write))
                .collect();
            self.finish_all(End::Stopped, &mut out);
        }
        for key in again {
            self.begin(key, &mut out);
        }
        for &(track, target) in f.touched {
            if matches!(world.mode(track), Mode::Touch | Mode::Latch)
                && world.armed(track, target)
                && !self.passes.contains_key(&(track, target))
            {
                self.begin((track, target), &mut out);
            }
        }
        for key in world.write_targets() {
            if !self.passes.contains_key(&key) {
                self.begin(key, &mut out);
            }
        }
        let keys: Vec<_> = self.passes.keys().copied().collect();
        for key in keys {
            let (track, target) = key;
            let mode = world.mode(track);
            if mode == Mode::Read || (mode == Mode::Write && !world.write_targets().contains(&key))
            {
                self.finish(key, End::Stopped, &mut out);
                continue;
            }
            let held = f.touched.contains(&key) || f.pointer_down || f.open.contains(&key);
            if mode == Mode::Touch && !held {
                self.finish(key, End::Released, &mut out);
                continue;
            }
            if let Some(value) = world.value(track, target)
                && let Some(raw) = self.passes.get_mut(&key)
            {
                raw.push((f.at, value));
            }
        }
        out
    }

    fn begin(&mut self, key: (Id, Target), out: &mut Vec<Action>) {
        self.passes.insert(key, Vec::new());
        out.push(Action::TakeOver(key.0, key.1));
    }

    fn finish(&mut self, key: (Id, Target), end: End, out: &mut Vec<Action>) {
        if let Some(raw) = self.passes.remove(&key)
            && !raw.is_empty()
        {
            out.push(Action::Write {
                track: key.0,
                target: key.1,
                raw,
                end,
            });
        }
    }

    fn finish_all(&mut self, end: End, out: &mut Vec<Action>) {
        let keys: Vec<_> = self.passes.keys().copied().collect();
        for key in keys {
            self.finish(key, end, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const T: Id = Id(1);
    const VOL: Target = Target::Volume;
    const PAN: Target = Target::Pan;

    #[derive(Default)]
    struct Fake {
        modes: BTreeMap<Id, Mode>,
        values: BTreeMap<(Id, Target), f32>,
        armed: Vec<(Id, Target)>,
    }

    impl World for Fake {
        fn mode(&self, track: Id) -> Mode {
            self.modes.get(&track).copied().unwrap_or_default()
        }
        fn armed(&self, track: Id, target: Target) -> bool {
            self.armed.contains(&(track, target))
        }
        fn write_targets(&self) -> Vec<(Id, Target)> {
            self.armed
                .iter()
                .copied()
                .filter(|(t, _)| self.mode(*t) == Mode::Write)
                .collect()
        }
        fn value(&self, track: Id, target: Target) -> Option<f32> {
            self.values.get(&(track, target)).copied()
        }
    }

    fn world(mode: Mode) -> Fake {
        let mut w = Fake::default();
        w.modes.insert(T, mode);
        w.armed = vec![(T, VOL), (T, PAN)];
        w.values.insert((T, VOL), -6.0);
        w.values.insert((T, PAN), 0.25);
        w
    }

    fn step(
        r: &mut Recorder,
        w: &Fake,
        at: i64,
        down: bool,
        touched: &[(Id, Target)],
    ) -> Vec<Action> {
        r.frame(
            &Frame {
                playing: true,
                at: Ticks(at),
                pointer_down: down,
                touched,
                open: &BTreeSet::new(),
            },
            w,
        )
    }

    fn stop(r: &mut Recorder, w: &Fake) -> Vec<Action> {
        r.frame(
            &Frame {
                playing: false,
                at: Ticks(0),
                pointer_down: false,
                touched: &[],
                open: &BTreeSet::new(),
            },
            w,
        )
    }

    fn written(actions: &[Action]) -> Vec<(Target, Vec<i64>, End)> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Write {
                    target, raw, end, ..
                } => Some((*target, raw.iter().map(|p| p.0.0).collect(), *end)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn read_records_nothing() {
        let w = world(Mode::Read);
        let mut r = Recorder::default();
        assert!(step(&mut r, &w, 0, true, &[(T, VOL)]).is_empty());
        assert!(r.is_empty());
        assert!(stop(&mut r, &w).is_empty());
    }

    #[test]
    fn touch_writes_while_held_and_ends_when_let_go() {
        let w = world(Mode::Touch);
        let mut r = Recorder::default();
        // Nothing is written until a control is touched.
        assert!(step(&mut r, &w, 0, false, &[]).is_empty());
        let a = step(&mut r, &w, 10, true, &[(T, VOL)]);
        assert_eq!(a, [Action::TakeOver(T, VOL)]);
        assert!(step(&mut r, &w, 20, true, &[]).is_empty(), "held, no move");
        assert!(step(&mut r, &w, 30, true, &[(T, VOL)]).is_empty());
        let a = step(&mut r, &w, 40, false, &[]);
        assert_eq!(written(&a), [(VOL, vec![10, 20, 30], End::Released)]);
        assert!(r.is_empty());
        // The other control was never touched.
        assert!(step(&mut r, &w, 50, false, &[]).is_empty());
    }

    #[test]
    fn a_wheel_move_is_a_touch_of_one_frame() {
        let w = world(Mode::Touch);
        let mut r = Recorder::default();
        step(&mut r, &w, 10, false, &[(T, PAN)]);
        let a = step(&mut r, &w, 20, false, &[]);
        assert_eq!(written(&a), [(PAN, vec![10], End::Released)]);
    }

    #[test]
    fn a_control_with_no_lane_and_not_shown_is_not_recorded() {
        let mut w = world(Mode::Touch);
        w.armed.clear();
        let mut r = Recorder::default();
        assert!(step(&mut r, &w, 10, true, &[(T, VOL)]).is_empty());
        assert!(r.is_empty());
    }

    #[test]
    fn a_plugin_gesture_is_held_until_it_ends() {
        let w = world(Mode::Touch);
        let mut r = Recorder::default();
        let open: BTreeSet<_> = [(T, VOL)].into();
        let frame = |r: &mut Recorder, at, open: &BTreeSet<(Id, Target)>, touched: &[_]| {
            r.frame(
                &Frame {
                    playing: true,
                    at: Ticks(at),
                    pointer_down: false,
                    touched,
                    open,
                },
                &w,
            )
        };
        frame(&mut r, 10, &open, &[(T, VOL)]);
        assert!(frame(&mut r, 20, &open, &[]).is_empty(), "open, no pointer");
        let a = frame(&mut r, 30, &BTreeSet::new(), &[]);
        assert_eq!(written(&a), [(VOL, vec![10, 20], End::Released)]);
    }

    #[test]
    fn latch_starts_at_the_first_touch_and_goes_on_until_stop() {
        let w = world(Mode::Latch);
        let mut r = Recorder::default();
        assert!(step(&mut r, &w, 0, false, &[]).is_empty(), "not yet");
        step(&mut r, &w, 10, true, &[(T, VOL)]);
        assert!(
            step(&mut r, &w, 20, false, &[]).is_empty(),
            "let go: still on"
        );
        assert!(step(&mut r, &w, 30, false, &[]).is_empty());
        let a = stop(&mut r, &w);
        assert_eq!(written(&a), [(VOL, vec![10, 20, 30], End::Stopped)]);
    }

    #[test]
    fn write_takes_the_lanes_over_at_once_and_goes_on_until_stop() {
        let w = world(Mode::Write);
        let mut r = Recorder::default();
        let a = step(&mut r, &w, 0, false, &[]);
        assert_eq!(a, [Action::TakeOver(T, VOL), Action::TakeOver(T, PAN)]);
        step(&mut r, &w, 10, false, &[]);
        let a = stop(&mut r, &w);
        assert_eq!(
            written(&a),
            [
                (VOL, vec![0, 10], End::Stopped),
                (PAN, vec![0, 10], End::Stopped)
            ]
        );
    }

    #[test]
    fn a_loop_wrap_ends_a_pass_and_write_and_latch_begin_the_next() {
        for mode in [Mode::Write, Mode::Latch] {
            let w = world(mode);
            let mut r = Recorder::default();
            step(&mut r, &w, 0, true, &[(T, VOL)]);
            step(&mut r, &w, 100, true, &[]);
            let a = step(&mut r, &w, 5, true, &[]);
            assert!(
                written(&a)
                    .iter()
                    .any(|(t, raw, _)| *t == VOL && raw == &[0, 100]),
                "{mode:?}: {a:?}"
            );
            assert!(a.contains(&Action::TakeOver(T, VOL)), "{mode:?} goes on");
            assert!(!r.is_empty());
        }
        // Touch held across the wrap: the pass ends and the next waits for a
        // touch, which the held control does not make until it moves.
        let w = world(Mode::Touch);
        let mut r = Recorder::default();
        step(&mut r, &w, 0, true, &[(T, VOL)]);
        step(&mut r, &w, 100, true, &[]);
        let a = step(&mut r, &w, 5, true, &[]);
        assert_eq!(written(&a), [(VOL, vec![0, 100], End::Stopped)]);
        assert!(r.is_empty());
    }

    #[test]
    fn switching_to_read_ends_the_pass() {
        let mut w = world(Mode::Latch);
        let mut r = Recorder::default();
        step(&mut r, &w, 10, true, &[(T, VOL)]);
        w.modes.insert(T, Mode::Read);
        let a = step(&mut r, &w, 20, false, &[]);
        assert_eq!(written(&a), [(VOL, vec![10], End::Stopped)]);
    }

    #[test]
    fn only_a_playing_song_is_recorded() {
        let w = world(Mode::Write);
        let mut r = Recorder::default();
        assert!(stop(&mut r, &w).is_empty());
        assert!(r.is_empty(), "a stopped song starts nothing");
    }
}
