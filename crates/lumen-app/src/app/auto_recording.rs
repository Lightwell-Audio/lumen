//! Recording a lane, the app's side (M7 slice A4b): each frame, after the
//! controls were drawn, the recorder is told where the playhead is and which
//! controls were moved, and what it finishes is written into the lanes, one
//! undo step for each pass. A plugin's own gestures (begin, perform, end) are
//! how a hold in a plugin's window is known.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use eframe::egui;
use lumen_core::automation::{Lane, Point, Shape};
use lumen_core::time::{TempoMap, Ticks};
use lumen_hosting::{Edit, EditKind};
use lumen_project::automation::Target;
use lumen_project::edit::automation::{Release, recorded};
use lumen_project::model::Id;

use crate::App;
use crate::arrangement::auto_scale as scale;
use crate::arrangement::auto_state::AutoView;
use crate::auto_record::{Action, End, Frame, Mode, World};
use crate::document::Document;

/// How long a Touch takes to return to the lane when a control is let go of.
const GLIDE_MS: i64 = 100;

/// How often a pass under way is written into its lane, so a crash or a quit
/// costs the last few seconds of it and not the whole pass (M7 review M1).
const COMMIT_EVERY: Duration = Duration::from_secs(5);

/// How far from what was played a recorded lane may stray, as a share of the
/// target's range: half a percent.
const TOLERANCE: f32 = 0.005;

/// A pass the project holds part of: the lane as it was when the pass began,
/// which every part is written over, and the key that joins the parts into
/// one undo step.
struct Committing {
    base: Lane,
    key: String,
    /// When the last part was written, or the pass began.
    since: Instant,
    /// A part was refused or came out too long: no more are tried.
    given_up: bool,
    /// At least one part is in the project: the undo step is open.
    written: bool,
}

/// The passes being written into the project in parts.
pub(crate) struct Commits {
    open: BTreeMap<(Id, Target), Committing>,
    /// Counts the passes, to give each its own key.
    begun: u64,
    every: Duration,
    /// How soon the autosave runs while passes are under way.
    autosave: Duration,
    /// The autosave is on its short delay for the passes.
    fast: bool,
}

impl Default for Commits {
    fn default() -> Self {
        Self {
            open: BTreeMap::new(),
            begun: 0,
            every: COMMIT_EVERY,
            autosave: crate::app::record::AUTOSAVE_DELAY,
            fast: false,
        }
    }
}

impl Commits {
    /// Whether a pass has parts in the project, so its undo step is open and
    /// the history must not be sealed until the pass ends.
    pub(crate) fn writing(&self) -> bool {
        self.open.values().any(|c| c.written)
    }

    /// Sets how often a pass is written and how soon the autosave follows
    /// (tests).
    pub(crate) fn set_pacing(&mut self, every: Duration, autosave: Duration) {
        self.every = every;
        self.autosave = autosave;
    }
}

/// What the recorder sees of the project and the lane view.
struct Looking<'a> {
    doc: &'a Document,
    auto: &'a AutoView,
}

impl Looking<'_> {
    fn track(&self, id: Id) -> Option<&lumen_project::model::Track> {
        lumen_project::routing::all_tracks(self.doc.project()).find(|t| t.id == id)
    }
}

impl World for Looking<'_> {
    fn mode(&self, track: Id) -> Mode {
        self.auto.mode_of(track)
    }

    fn armed(&self, track: Id, target: Target) -> bool {
        // A control a controller is mapped to is meant to be recorded, with
        // or without a lane yet.
        self.doc.mapping_of(track, target).is_some()
            || self.auto.shown.get(&track) == Some(&target)
            || self
                .track(track)
                .is_some_and(|t| t.lane(target).is_some_and(|l| l.enabled))
    }

    fn write_targets(&self) -> Vec<(Id, Target)> {
        let mut out = Vec::new();
        for (&id, &mode) in &self.auto.modes {
            if mode != Mode::Write {
                continue;
            }
            let Some(track) = self.track(id) else {
                continue;
            };
            // Only the lane the chip shows: a track's other lanes keep what
            // was written to them.
            if let Some(&shown) = self.auto.shown.get(&id)
                && shown.clip().is_none()
                && track.has_target(shown)
            {
                out.push((id, shown));
            }
        }
        out
    }

    fn value(&self, track: Id, target: Target) -> Option<f32> {
        self.track(track).map(|t| scale::own_value(t, target))
    }
}

impl App {
    /// Records what moved this frame. Call it after the interface was drawn,
    /// so the moves made in it are this frame's.
    pub(crate) fn record_automation(&mut self, ctx: &egui::Context, rate: u32) {
        let touched = self.document.take_touched();
        let rests = self.document.take_rests();
        let at = self.transport.position();
        let frame = Frame {
            playing: self.transport.state().playing,
            at,
            pointer_down: ctx.input(|i| i.pointer.any_down()),
            touched: &touched,
            open: &self.gestures_open,
            rests: &rests,
        };
        let world = Looking {
            doc: &self.document,
            auto: &self.arrangement.auto,
        };
        let actions = self.recorder.frame(&frame, &world);
        for action in actions {
            match action {
                Action::TakeOver(track, target) => {
                    self.document.override_lane(track, target);
                    self.begin_commits(track, target);
                }
                Action::Write {
                    track,
                    target,
                    raw,
                    end,
                    rest,
                } => self.write_pass(
                    Pass {
                        track,
                        target,
                        raw: &raw,
                        end,
                        rest,
                    },
                    at,
                    rate,
                ),
            }
        }
        self.commit_passes();
        let active: BTreeSet<(Id, Target)> = self.recorder.active().collect();
        if self.arrangement.auto.recording != active {
            self.arrangement.auto.recording = active;
        }
        self.pace_autosave();
    }

    /// Forgets the passes, the plugin gestures and the controller state of
    /// the project that was open, for another one (M7 review H1): what a
    /// pass under way would write belongs to the track ids of the old
    /// project, and the new one has tracks of its own with the same ids.
    pub(crate) fn forget_passes(&mut self) {
        self.recorder = Default::default();
        self.gestures_open.clear();
        self.lane_commits.open.clear();
        self.controllers.forget_project();
    }

    /// Notes the lane a pass begins on as it is now.
    fn begin_commits(&mut self, track: Id, target: Target) {
        let base = lumen_project::routing::all_tracks(self.document.project())
            .find(|t| t.id == track)
            .and_then(|t| t.lane(target))
            .map_or_else(
                || Lane::new(Vec::new()).expect("an empty lane"),
                |l| l.lane.clone(),
            );
        let commits = &mut self.lane_commits;
        commits.begun += 1;
        let key = format!(
            "record automation {:x} {target:?} {}",
            track.0, commits.begun
        );
        commits.open.insert(
            (track, target),
            Committing {
                base,
                key,
                since: Instant::now(),
                given_up: false,
                written: false,
            },
        );
    }

    /// Writes each pass that is due into its lane, so far as it has gone.
    fn commit_passes(&mut self) {
        let due: Vec<(Id, Target)> = self
            .lane_commits
            .open
            .iter()
            .filter(|(_, c)| !c.given_up && c.since.elapsed() >= self.lane_commits.every)
            .map(|(&key, _)| key)
            .collect();
        for (track, target) in due {
            let Some((raw, rest)) = self
                .recorder
                .heard((track, target))
                .map(|heard| (heard.raw.to_vec(), heard.rest))
            else {
                continue;
            };
            let Some(commit) = self.lane_commits.open.get_mut(&(track, target)) else {
                continue;
            };
            commit.since = Instant::now();
            if raw.is_empty() {
                continue;
            }
            let (key, base) = (commit.key.clone(), commit.base.clone());
            let lane = pass_lane(&base, &raw, rest, target, Release::Hold);
            let done = match lane {
                Some(lane) => self.document.record_lane_so_far(track, target, lane, &key),
                None => Err(crate::document::LaneRefusal::TooMany),
            };
            if done.is_ok() {
                if let Some(commit) = self.lane_commits.open.get_mut(&(track, target)) {
                    commit.written = true;
                }
            } else {
                if let Some(commit) = self.lane_commits.open.get_mut(&(track, target)) {
                    commit.given_up = true;
                }
                self.say(
                    "A pass in progress could not be kept in the project; it will be written when it ends"
                        .to_owned(),
                );
            }
        }
    }

    /// Keeps the autosave on its short delay while passes are written, as it
    /// is while a MIDI take is.
    fn pace_autosave(&mut self) {
        let active = !self.lane_commits.open.is_empty();
        if active == self.lane_commits.fast {
            return;
        }
        self.lane_commits.fast = active;
        let midi_taking = self.record.active();
        if let Some(autosave) = self.autosave.as_mut() {
            if active {
                autosave.hurry(self.lane_commits.autosave);
            } else if !midi_taking {
                autosave.unhurry();
            }
        }
    }

    /// Writes a finished pass into its lane as one undo step, over the lane as
    /// it was when the pass began and joined to the parts already written.
    fn write_pass(&mut self, pass: Pass<'_>, at: Ticks, rate: u32) {
        let Pass {
            track,
            target,
            raw,
            end,
            rest,
        } = pass;
        let project = self.document.project();
        let (base, key) = match self.lane_commits.open.remove(&(track, target)) {
            Some(commit) => (commit.base, commit.key),
            None => {
                self.lane_commits.begun += 1;
                let base = lumen_project::routing::all_tracks(project)
                    .find(|t| t.id == track)
                    .and_then(|t| t.lane(target))
                    .map_or_else(
                        || Lane::new(Vec::new()).expect("an empty lane"),
                        |l| l.lane.clone(),
                    );
                (
                    base,
                    format!("record automation {}", self.lane_commits.begun),
                )
            }
        };
        let release = match end {
            End::Stopped => Release::Hold,
            End::Released => Release::Glide(glide(project, at, rate)),
        };
        let Some(lane) = pass_lane(&base, raw, rest, target, release) else {
            self.say("That pass would make the lane too long to keep".to_owned());
            return;
        };
        let result = self.document.record_lane(track, target, lane, &key);
        // A lane the pass made is shown, if the track shows none, so it can be
        // seen and edited.
        if result.is_ok() {
            self.arrangement.auto.shown.entry(track).or_insert(target);
        }
        self.arrangement.note_lane(result);
    }

    /// Sets what the tracks the lane commands act on do while the song plays.
    pub(crate) fn auto_set_mode(&mut self, mode: Mode) {
        for track in self.auto_tracks() {
            self.arrangement.auto.set_mode(track, mode);
        }
    }

    /// Notes the plugin gestures that open and close in `edits`, which a
    /// plugin made in `device`: a control held in its window is held until its
    /// gesture ends.
    pub(crate) fn note_gestures(&mut self, device: Id, edits: &[(Edit, bool)]) {
        let Some((track, _)) = crate::document::find_device(self.document.project(), device) else {
            return;
        };
        for &(edit, inside) in edits {
            let key = (
                track,
                Target::Device {
                    device,
                    param: edit.id,
                },
            );
            match edit.kind {
                EditKind::Begin if inside => {
                    self.gestures_open.insert(key);
                }
                EditKind::End if inside => {
                    self.gestures_open.remove(&key);
                }
                _ => {}
            }
        }
    }
}

/// The lane a pass makes of `raw` over `base`, the lane as it was when the pass
/// began. A lane the pass makes says what the control held until it was
/// touched, and a Touch returns to that. `None` when the result would hold too
/// many points.
fn pass_lane(
    base: &Lane,
    raw: &[(Ticks, f32)],
    rest: Option<f32>,
    target: Target,
    release: Release,
) -> Option<Lane> {
    let mut base = base.clone();
    if base.points().is_empty()
        && let Some(value) = rest
    {
        let first = Point::new(Ticks::ZERO, value);
        let first = if target == Target::Mute {
            first.with_shape(Shape::Step)
        } else {
            first
        };
        base = Lane::new(vec![first]).expect("one point");
    }
    let (low, high) = target.range();
    recorded(
        &base,
        raw,
        release,
        target == Target::Mute,
        (high - low) * TOLERANCE,
    )
}

/// A finished pass, to be written into a lane.
struct Pass<'a> {
    track: Id,
    target: Target,
    raw: &'a [(Ticks, f32)],
    end: End,
    rest: Option<f32>,
}

/// A Touch's return to the lane, in ticks at the tempo where it ends.
fn glide(project: &lumen_project::model::Project, at: Ticks, rate: u32) -> Ticks {
    let Ok(tempo) = TempoMap::new(project.tempo.clone()) else {
        return Ticks(0);
    };
    let from = tempo.sample_at(at, rate);
    let over = i64::from(rate) * GLIDE_MS / 1000;
    tempo.ticks_at_sample(from + over, rate) - at
}
