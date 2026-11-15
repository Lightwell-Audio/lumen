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
use crate::arrangement::auto_state::AutoView;
use crate::document::Document;
use crate::recording::auto_record::{Action, End, Frame, Mode, World};

const GLIDE_MS: i64 = 100;

const COMMIT_EVERY: Duration = Duration::from_secs(5);

const TOLERANCE: f32 = 0.005;

struct Committing {
    base: Lane,
    key: String,
    since: Instant,
    given_up: bool,
    written: bool,
}

pub(crate) struct Commits {
    open: BTreeMap<(Id, Target), Committing>,
    begun: u64,
    every: Duration,
    autosave: Duration,
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
    pub(crate) fn writing(&self) -> bool {
        self.open.values().any(|c| c.written)
    }

    pub(crate) fn set_pacing(&mut self, every: Duration, autosave: Duration) {
        self.every = every;
        self.autosave = autosave;
    }
}

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
        self.track(track)
            .map(|t| lumen_project::automation::own_value(t, target))
    }
}

impl App {
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

    pub(crate) fn forget_passes(&mut self) {
        self.recorder = Default::default();
        self.gestures_open.clear();
        self.lane_commits.open.clear();
        self.controllers.forget_project();
    }

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
        if result.is_ok() {
            self.arrangement.auto.shown.entry(track).or_insert(target);
        }
        self.arrangement.note_lane(result);
    }

    pub(crate) fn auto_set_mode(&mut self, mode: Mode) {
        for track in self.auto_tracks() {
            self.arrangement.auto.set_mode(track, mode);
        }
    }

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
        base = Lane::new(vec![first]).ok()?;
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

struct Pass<'a> {
    track: Id,
    target: Target,
    raw: &'a [(Ticks, f32)],
    end: End,
    rest: Option<f32>,
}

fn glide(project: &lumen_project::model::Project, at: Ticks, rate: u32) -> Ticks {
    let Ok(tempo) = TempoMap::new(project.tempo.clone()) else {
        return Ticks(0);
    };
    let from = tempo.sample_at(at, rate);
    let over = i64::from(rate) * GLIDE_MS / 1000;
    tempo.ticks_at_sample(from + over, rate) - at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pass_whose_rest_is_not_a_number_is_dropped() {
        let lane = pass_lane(
            &Lane::default(),
            &[],
            Some(f32::NAN),
            Target::Volume,
            Release::Hold,
        );
        assert!(lane.is_none());
    }
}
