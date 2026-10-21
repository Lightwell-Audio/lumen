//! Recording a lane, the app's side (M7 slice A4b): each frame, after the
//! controls were drawn, the recorder is told where the playhead is and which
//! controls were moved, and what it finishes is written into the lanes, one
//! undo step for each pass. A plugin's own gestures (begin, perform, end) are
//! how a hold in a plugin's window is known.

use std::collections::BTreeSet;

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

/// How far from what was played a recorded lane may stray, as a share of the
/// target's range: half a percent.
const TOLERANCE: f32 = 0.005;

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
        let active: BTreeSet<(Id, Target)> = self.recorder.active().collect();
        if self.arrangement.auto.recording != active {
            self.arrangement.auto.recording = active;
        }
    }

    /// Writes a finished pass into its lane as one undo step.
    fn write_pass(&mut self, pass: Pass<'_>, at: Ticks, rate: u32) {
        let Pass {
            track,
            target,
            raw,
            end,
            rest,
        } = pass;
        let project = self.document.project();
        let mut base = lumen_project::routing::all_tracks(project)
            .find(|t| t.id == track)
            .and_then(|t| t.lane(target))
            .map_or_else(
                || Lane::new(Vec::new()).expect("an empty lane"),
                |l| l.lane.clone(),
            );
        // A lane the pass makes says what the control held until it was
        // touched, and a Touch returns to that.
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
        let release = match end {
            End::Stopped => Release::Hold,
            End::Released => Release::Glide(glide(project, at, rate)),
        };
        let (low, high) = target.range();
        let Some(lane) = recorded(
            &base,
            raw,
            release,
            target == Target::Mute,
            (high - low) * TOLERANCE,
        ) else {
            self.say("That pass would make the lane too long to keep".to_owned());
            return;
        };
        let result = self.document.record_lane(track, target, lane);
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
