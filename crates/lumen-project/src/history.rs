//! Undo and redo by shared snapshots (ADR-0024).
//!
//! Every step keeps the whole project as it was, but a snapshot is cheap:
//! tracks and clips are `Arc`s, so it shares everything the edit did not
//! touch. Undo therefore restores the exact previous state by construction.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::model::{Project, Track};

/// Steps kept by default; the oldest are dropped beyond this.
pub const DEFAULT_LIMIT: usize = 10_000;

#[derive(Debug, Clone)]
struct Step {
    /// What the edit that left this state did ("Change tempo").
    label: String,
    project: Project,
    state: u64,
}

/// The project with its undo and redo history.
#[derive(Debug, Clone)]
pub struct History {
    current: Project,
    state: u64,
    next_state: u64,
    /// Oldest first; the last entry is the state before the latest edit.
    undo: VecDeque<Step>,
    /// The next redo is the last entry.
    redo: Vec<Step>,
    /// The merge key of the gesture in progress, if any.
    open_gesture: Option<String>,
    limit: usize,
}

impl History {
    pub fn new(project: Project) -> Self {
        Self {
            current: project,
            state: 0,
            next_state: 1,
            undo: VecDeque::new(),
            redo: Vec::new(),
            open_gesture: None,
            limit: DEFAULT_LIMIT,
        }
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    pub fn project(&self) -> &Project {
        &self.current
    }

    /// Identifies the current state: equal values mean the same project (for
    /// example, after undoing back to what was saved).
    pub fn state(&self) -> u64 {
        self.state
    }

    /// Applies `edit` as one undo step named `label`. Edits with the same
    /// `merge` key while a gesture is open (until [`seal`](Self::seal))
    /// become one step, so a drag undoes in one go. An edit that changes
    /// nothing records nothing. Returns whether the project changed.
    pub fn apply(
        &mut self,
        label: &str,
        merge: Option<&str>,
        edit: impl FnOnce(&mut Project),
    ) -> bool {
        let mut next = self.current.clone();
        edit(&mut next);
        if unchanged(&self.current, &next) {
            return false;
        }
        let continuing =
            merge.is_some() && merge == self.open_gesture.as_deref() && !self.undo.is_empty();
        if !continuing {
            self.undo.push_back(Step {
                label: label.to_owned(),
                project: std::mem::replace(&mut self.current, next),
                state: self.state,
            });
            while self.undo.len() > self.limit {
                self.undo.pop_front();
            }
        } else {
            self.current = next;
        }
        self.redo.clear();
        self.open_gesture = merge.map(str::to_owned);
        self.state = self.fresh_state();
        true
    }

    /// Ends the gesture in progress: the next edit is a new step.
    pub fn seal(&mut self) {
        self.open_gesture = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The label of the step [`undo`](Self::undo) would revert.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.back().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    /// Reverts the latest step. Returns its label.
    pub fn undo(&mut self) -> Option<String> {
        let step = self.undo.pop_back()?;
        self.open_gesture = None;
        let label = step.label.clone();
        self.redo.push(Step {
            label: step.label,
            project: std::mem::replace(&mut self.current, step.project),
            state: std::mem::replace(&mut self.state, step.state),
        });
        Some(label)
    }

    /// Re-applies the latest undone step. Returns its label.
    pub fn redo(&mut self) -> Option<String> {
        let step = self.redo.pop()?;
        self.open_gesture = None;
        let label = step.label.clone();
        self.undo.push_back(Step {
            label: step.label,
            project: std::mem::replace(&mut self.current, step.project),
            state: std::mem::replace(&mut self.state, step.state),
        });
        Some(label)
    }

    /// The step labels: done (oldest first), then undone (next redo first).
    pub fn steps(&self) -> (Vec<&str>, Vec<&str>) {
        (
            self.undo.iter().map(|s| s.label.as_str()).collect(),
            self.redo.iter().rev().map(|s| s.label.as_str()).collect(),
        )
    }

    /// Moves to the state after `done` steps (0 = before the oldest kept
    /// step), undoing or redoing as needed.
    pub fn jump_to(&mut self, done: usize) {
        while self.undo.len() > done && self.undo().is_some() {}
        while self.undo.len() < done && self.redo().is_some() {}
    }

    fn fresh_state(&mut self) -> u64 {
        let state = self.next_state;
        self.next_state += 1;
        state
    }
}

/// Fast equality for the edit check: tracks and clips that are the same
/// `Arc` are not compared field by field.
fn unchanged(a: &Project, b: &Project) -> bool {
    // Destructured, so a new field cannot be forgotten here (one was: the
    // routing fields of format v7 made edits look like no change).
    let Project {
        name,
        sample_rate,
        tempo,
        time_signature,
        tracks,
        master,
        pan_law,
        media,
        markers,
        transport,
        view,
    } = a;
    *name == b.name
        && *sample_rate == b.sample_rate
        && *tempo == b.tempo
        && *time_signature == b.time_signature
        && *pan_law == b.pan_law
        && *media == b.media
        && *markers == b.markers
        && *transport == b.transport
        && *view == b.view
        && same_track(master, &b.master)
        && tracks.len() == b.tracks.len()
        && tracks.iter().zip(&b.tracks).all(|(x, y)| same_track(x, y))
}

/// Whether two tracks are the same to the bit (floats by their bits, so a
/// change from 0.0 to −0.0 still counts and NaN equals itself).
fn same_track(x: &Arc<Track>, y: &Arc<Track>) -> bool {
    if Arc::ptr_eq(x, y) {
        return true;
    }
    let Track {
        id,
        name,
        kind,
        color,
        parent,
        gain_db,
        pan,
        output,
        sends,
        invert,
        trim_db,
        mute,
        solo,
        armed,
        devices,
        midi_to,
        clips,
    } = &**x;
    let bits = |a: f32, b: f32| a.to_bits() == b.to_bits();
    *id == y.id
        && *name == y.name
        && *kind == y.kind
        && *color == y.color
        && *parent == y.parent
        && bits(*gain_db, y.gain_db)
        && bits(*pan, y.pan)
        && *output == y.output
        && sends.len() == y.sends.len()
        && sends
            .iter()
            .zip(&y.sends)
            .all(|(s, t)| s.to == t.to && s.pre_fader == t.pre_fader && bits(s.level.0, t.level.0))
        && *invert == y.invert
        && bits(*trim_db, y.trim_db)
        && *mute == y.mute
        && *solo == y.solo
        && *armed == y.armed
        && *devices == y.devices
        && *midi_to == y.midi_to
        && clips.len() == y.clips.len()
        && clips
            .iter()
            .zip(&y.clips)
            .all(|(c, d)| Arc::ptr_eq(c, d) || c == d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Clip, ClipContent, Color, Id, Track, TrackKind};
    use crate::testing::{Rng, small_project};
    use lumen_core::time::Ticks;

    fn edit_at_random(rng: &mut Rng, project: &mut Project, next_id: &mut u64) {
        let mut id = || {
            *next_id += 1;
            Id(*next_id)
        };
        match rng.below(6) {
            0 => {
                project.tracks.push(Arc::new(Track::new(
                    id(),
                    format!("Track {}", rng.below(100)),
                    TrackKind::Midi,
                    Color(0x4C7A9E),
                )));
            }
            1 if project.tracks.len() > 1 => {
                let i = rng.below(project.tracks.len() as u64) as usize;
                // Keep group parents valid: only remove tracks nobody points at.
                let removed = project.tracks[i].id;
                if project.tracks.iter().all(|t| t.parent != Some(removed)) {
                    project.tracks.remove(i);
                }
            }
            2 if !project.tracks.is_empty() => {
                let i = rng.below(project.tracks.len() as u64) as usize;
                Arc::make_mut(&mut project.tracks[i]).name = format!("Renamed {}", rng.below(100));
            }
            3 if !project.tracks.is_empty() => {
                let i = rng.below(project.tracks.len() as u64) as usize;
                if project.tracks[i].kind == TrackKind::Midi {
                    let clip = Arc::new(Clip {
                        id: id(),
                        name: "Clip".into(),
                        start: Ticks::quarters(rng.below(64) as i64),
                        length: Ticks::quarters(1 + rng.below(8) as i64),
                        muted: false,
                        color: None,
                        locked: false,
                        fade_in: Default::default(),
                        fade_out: Default::default(),
                        loop_length: None,
                        content: ClipContent::midi(),
                    });
                    Arc::make_mut(&mut project.tracks[i]).clips.push(clip);
                }
            }
            4 => {
                for track in &mut project.tracks {
                    if let Some(clip) = track.clips.first() {
                        let mut moved = (**clip).clone();
                        moved.start = moved.start + Ticks::quarters(1);
                        Arc::make_mut(track).clips[0] = Arc::new(moved);
                        break;
                    }
                }
            }
            _ => {
                if let Some(first) = project.tempo.first_mut() {
                    first.bpm = 60.0 + rng.below(120) as f64;
                }
            }
        }
    }

    /// Random edits, undos and redos, checked against a reference that keeps
    /// deep copies: the history must always show exactly the expected state.
    #[test]
    fn random_edits_undo_and_redo_exactly() {
        for seed in 1..=40 {
            let mut rng = Rng::new(seed);
            let start = small_project(seed);
            let mut history = History::new(start.clone());
            let mut done: Vec<Project> = vec![start.clone()];
            let mut undone: Vec<Project> = vec![];
            let mut next_id = 1_000_000;
            for _ in 0..300 {
                match rng.below(10) {
                    0..=5 => {
                        let mut expected = done.last().unwrap().clone();
                        let mut edited = expected.clone();
                        edit_at_random(&mut rng, &mut edited, &mut next_id);
                        let applied = history.apply("Edit", None, |p| *p = edited.clone());
                        if applied {
                            expected = edited;
                            done.push(expected);
                            undone.clear();
                        }
                    }
                    6..=7 => {
                        if history.undo().is_some() {
                            undone.push(done.pop().unwrap());
                        }
                    }
                    _ => {
                        if history.redo().is_some() {
                            done.push(undone.pop().unwrap());
                        }
                    }
                }
                assert_eq!(history.project(), done.last().unwrap(), "seed {seed}");
            }
            // Redo to the newest state, then undo everything (the starting
            // project) and redo everything (back to the newest).
            while history.redo().is_some() {}
            let end = history.project().clone();
            assert_eq!(&end, undone.first().or(done.last()).unwrap(), "seed {seed}");
            while history.undo().is_some() {}
            assert_eq!(history.project(), &start, "seed {seed}");
            while history.redo().is_some() {}
            assert_eq!(history.project(), &end, "seed {seed}");
        }
    }

    #[test]
    fn routing_changes_are_edits() {
        // Format v7's fields once looked unchanged to the history, so the
        // edits recorded nothing.
        use crate::model::{Level, Output, Send};
        type Change = fn(&mut Project);
        let changes: [Change; 7] = [
            |p| p.pan_law = lumen_core::pan::PanLaw::Linear,
            |p| {
                Arc::make_mut(&mut p.tracks[2]).midi_to = Some(crate::model::MidiTo {
                    port: "Port".into(),
                    channel: 3,
                });
            },
            |p| Arc::make_mut(&mut p.master).gain_db = -1.0,
            |p| Arc::make_mut(&mut p.tracks[2]).output = Output::Master,
            |p| Arc::make_mut(&mut p.tracks[2]).invert = true,
            |p| Arc::make_mut(&mut p.tracks[2]).trim_db = 2.0,
            |p| {
                let to = p.tracks[1].id;
                Arc::make_mut(&mut p.tracks[2]).sends.push(Send {
                    to,
                    level: Level::SILENT,
                    pre_fader: false,
                })
            },
        ];
        for change in changes {
            let mut history = History::new(small_project(2));
            assert!(history.apply("change", None, change));
            assert!(history.can_undo());
        }
    }

    #[test]
    fn a_gesture_is_one_step_until_sealed() {
        let start = small_project(3);
        let mut history = History::new(start.clone());
        for bpm in 100..160 {
            history.apply("Change tempo", Some("tempo"), |p| {
                p.tempo[0].bpm = f64::from(bpm);
            });
        }
        assert_eq!(history.steps().0, ["Change tempo"]);
        history.seal();
        history.apply("Change tempo", Some("tempo"), |p| p.tempo[0].bpm = 90.0);
        assert_eq!(history.steps().0.len(), 2);
        history.undo();
        assert_eq!(history.project().tempo[0].bpm, 159.0);
        history.undo();
        assert_eq!(history.project(), &start);
    }

    #[test]
    fn edits_that_change_nothing_record_nothing() {
        let mut history = History::new(small_project(4));
        assert!(!history.apply("Nothing", None, |_| {}));
        let bpm = history.project().tempo[0].bpm;
        assert!(!history.apply("Same tempo", None, |p| p.tempo[0].bpm = bpm));
        assert!(!history.can_undo());
    }

    #[test]
    fn the_state_returns_when_undoing_to_a_saved_point() {
        let mut history = History::new(small_project(5));
        let saved = history.state();
        history.apply("Rename", None, |p| p.name = "Other".into());
        assert_ne!(history.state(), saved);
        history.undo();
        assert_eq!(history.state(), saved);
        history.redo();
        assert_ne!(history.state(), saved);
    }

    #[test]
    fn a_new_edit_clears_redo_and_the_limit_drops_the_oldest() {
        let mut history = History::new(small_project(6)).with_limit(3);
        for i in 0..5 {
            history.apply(&format!("Step {i}"), None, |p| p.name = format!("{i}"));
        }
        assert_eq!(history.steps().0, ["Step 2", "Step 3", "Step 4"]);
        history.undo();
        assert!(history.can_redo());
        history.apply("New", None, |p| p.name = "new".into());
        assert!(!history.can_redo());
        history.jump_to(0);
        assert_eq!(history.project().name, "1");
    }

    #[test]
    fn snapshots_share_what_an_edit_did_not_touch() {
        let mut history = History::new(crate::testing::synthetic(9, 50, 20, 4, 0));
        let before = history.project().clone();
        history.apply("Rename track", None, |p| {
            Arc::make_mut(&mut p.tracks[7]).name = "Lead".into();
        });
        let after = history.project();
        let shared = before
            .tracks
            .iter()
            .zip(&after.tracks)
            .filter(|(a, b)| Arc::ptr_eq(a, b))
            .count();
        assert_eq!(shared, 49, "only the renamed track is copied");
        assert!(
            before.tracks[7]
                .clips
                .iter()
                .zip(&after.tracks[7].clips)
                .all(|(a, b)| Arc::ptr_eq(a, b)),
            "its clips are still shared"
        );
    }
}
