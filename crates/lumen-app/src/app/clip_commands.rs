//! Clip commands of M7b slice E1 that work on the selection: join, crop to
//! the time range, unloop, nudge, and the selection variants.

use lumen_core::grid::Step;
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::Id;

use crate::App;

/// The finest nudge: a 128th note, as the clip editor's fine nudge.
const FINE: Ticks = Ticks(PPQ / 32);

impl App {
    /// The selected clips, in no order.
    fn selected_clip_ids(&self) -> Vec<Id> {
        self.arrangement.clips.iter().copied().collect()
    }

    /// Whether any selected clip is an audio clip (locked or not).
    pub(crate) fn selection_has_audio(&self) -> bool {
        self.document
            .project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .any(|c| {
                self.arrangement.clips.contains(&c.id)
                    && matches!(c.content, lumen_project::model::ClipContent::Audio { .. })
            })
    }

    /// Toggles an explicit crossfade request for the selected audio clips.
    /// The engine ignores requests without an eligible touching join/tail.
    pub(crate) fn toggle_selected_crossfade(&mut self) {
        let (ids, enabled) = {
            let project = self.document.project();
            let clips: Vec<_> = project
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .filter(|c| self.arrangement.clips.contains(&c.id) && !c.locked)
                .filter(|c| matches!(c.content, lumen_project::model::ClipContent::Audio { .. }))
                .collect();
            if clips.is_empty() {
                return;
            }
            (
                clips.iter().map(|c| c.id).collect::<Vec<_>>(),
                !clips.iter().all(|c| c.crossfade),
            )
        };
        self.document.set_crossfade(&ids, enabled);
        if !enabled && self.document.project().auto_crossfade {
            self.say("Custom crossfade disabled; automatic crossfades still apply".to_owned());
        } else if enabled
            && !ids.iter().any(|id| {
                self.document.project().tracks.iter().any(|track| {
                    track
                        .clips
                        .iter()
                        .find(|c| c.id == *id)
                        .is_some_and(|clip| {
                            crate::crossfades::fade(self.document.project(), track.id, clip)
                                .is_some()
                        })
                })
            })
        {
            self.say("No selected clip has an eligible join and source tail".to_owned());
        }
    }

    /// Joins the selected clips (Ctrl+J).
    pub(crate) fn join_selected_clips(&mut self) {
        let ids = self.selected_clip_ids();
        if ids.len() < 2 {
            self.say("Select two or more clips to join".to_owned());
            return;
        }
        let joined = self.document.join_clips(&ids);
        if joined.merged > 0 {
            self.arrangement.clips = joined.clips.into_iter().collect();
            self.arrangement.clip_cursor = None;
        } else if joined.apart {
            self.say("These clips do not continue one another: use Consolidate".to_owned());
        } else {
            self.say("Nothing to join: the clips are locked or apart".to_owned());
        }
    }

    /// Keeps the selected time range and removes the rest of its tracks'
    /// clips (Crop to selection).
    pub(crate) fn crop_to_range(&mut self) {
        let Some(range) = self.arrangement.range.clone() else {
            return;
        };
        self.document
            .crop_clips(&range.tracks, range.start, range.end);
        self.arrangement.clips.clear();
    }

    /// Turns the selected looping clips into real clips.
    pub(crate) fn unloop_selected_clips(&mut self) {
        let ids = self.selected_clip_ids();
        let (now, too_long) = self.document.unloop_clips(&ids);
        if too_long {
            self.say("A clip repeats too many times to unloop".to_owned());
        }
        if !now.is_empty() {
            self.arrangement.clips = now.into_iter().collect();
            self.arrangement.clip_cursor = None;
        }
    }

    /// Plays the selected audio clips backwards, or forwards again.
    pub(crate) fn reverse_selected_clips(&mut self) {
        let ids = self.selected_clip_ids();
        let done = self.document.reverse_clips(&ids);
        if done.refused > 0 {
            self.say(if done.changed > 0 {
                "Some clips were left as they were: they reach past the end of their file"
                    .to_owned()
            } else {
                "This clip reaches past the end of its file, so it cannot be reversed".to_owned()
            });
        }
    }

    /// Inverts the polarity of the selected audio clips, or puts it back.
    pub(crate) fn invert_selected_clips(&mut self) {
        let ids = self.selected_clip_ids();
        self.document.invert_clips(&ids);
    }

    /// One grid step in ticks, at the playhead (a step in bars is as long as
    /// the bar there).
    fn clip_nudge_step(&self) -> Ticks {
        match self.grid_step() {
            Step::Ticks(step) => step,
            Step::Bars(n) => {
                let meter = self
                    .transport
                    .timeline
                    .meter
                    .meter_at(self.transport.position());
                Ticks(meter.bar().0.saturating_mul(i64::from(n)))
            }
        }
    }

    /// Moves the selected clips one grid step, or finely, earlier
    /// (`direction` below zero) or later.
    pub(crate) fn nudge_selected_clips(&mut self, direction: i64, fine: bool) {
        let step = if fine { FINE } else { self.clip_nudge_step() };
        let ids = self.selected_clip_ids();
        self.document
            .nudge_clips(&ids, Ticks(step.0.saturating_mul(direction)));
    }

    /// Selects the clips that are not selected, and no others.
    pub(crate) fn invert_clip_selection(&mut self) {
        let project = self.document.project();
        let chosen = &self.arrangement.clips;
        let inverted = project
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .map(|c| c.id)
            .filter(|id| !chosen.contains(id))
            .collect();
        self.arrangement.clips = inverted;
        self.arrangement.clip_cursor = None;
        self.arrangement.range = None;
        self.arrangement.selected.clear();
    }

    /// Selects every clip on the tracks that hold a selected clip, or the
    /// selected tracks.
    pub(crate) fn select_clips_in_track(&mut self) {
        let project = self.document.project();
        let chosen = &self.arrangement.clips;
        let tracks = project.tracks.iter().filter(|t| {
            self.arrangement.selected.contains(&t.id)
                || t.clips.iter().any(|c| chosen.contains(&c.id))
        });
        let all = tracks.flat_map(|t| &t.clips).map(|c| c.id).collect();
        self.arrangement.clips = all;
        self.arrangement.clip_cursor = None;
        self.arrangement.range = None;
        self.arrangement.selected.clear();
    }

    /// Selects, on the tracks that hold a selected clip, every clip from the
    /// start of the song up to the latest selected clip (`to_end` false), or
    /// from the earliest selected clip to the end of the song (`to_end`).
    pub(crate) fn select_clips_toward(&mut self, to_end: bool) {
        let project = self.document.project();
        let chosen = &self.arrangement.clips;
        let held = |t: &&std::sync::Arc<lumen_project::model::Track>| {
            t.clips.iter().any(|c| chosen.contains(&c.id))
        };
        let starts = || {
            project
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .filter(|c| chosen.contains(&c.id))
                .map(|c| c.start)
        };
        let Some(edge) = (if to_end {
            starts().min()
        } else {
            starts().max()
        }) else {
            return;
        };
        let all = project
            .tracks
            .iter()
            .filter(held)
            .flat_map(|t| &t.clips)
            .filter(|c| {
                if to_end {
                    c.start >= edge
                } else {
                    c.start <= edge
                }
            })
            .map(|c| c.id)
            .collect();
        self.arrangement.clips = all;
        self.arrangement.clip_cursor = None;
    }
}
