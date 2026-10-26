//! Clip commands of M7b slice E1 that work on the selection: join, crop to
//! the time range, unloop, nudge, and the selection variants.

use eframe::egui;
use lumen_core::grid::Step;
use lumen_core::time::{PPQ, Ticks};
use lumen_project::model::Id;

use crate::App;

/// The finest nudge: a 128th note, as the clip editor's fine nudge.
const FINE: Ticks = Ticks(PPQ / 32);

impl App {
    /// The selected clips, in no order.
    pub(super) fn selected_clip_ids(&self) -> Vec<Id> {
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

    /// Makes a short crossfade into each selected audio clip from the clip
    /// before it, or removes theirs.
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
        if enabled {
            let project = self.document.project();
            let made = project
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .any(|c| ids.contains(&c.id) && c.crossfade);
            if !made {
                self.say("No selected clip has an audio clip against its start".to_owned());
            }
        }
    }

    /// Joins the selected clips (Ctrl+J).
    pub(crate) fn join_selected_clips(&mut self, ctx: &egui::Context) {
        let ids = self.selected_clip_ids();
        if ids.len() < 2 {
            self.say("Select two or more clips to join".to_owned());
            return;
        }
        let held = self.other_takes_held();
        let joined = self.document.join_clips(&ids);
        self.say_dropped_takes(held);
        if joined.merged > 0 {
            self.arrangement.clips = joined.clips.iter().copied().collect();
            self.arrangement.clip_cursor = None;
        }
        if joined.apart {
            // Audio that does not continue in its source cannot be one clip
            // of the same file: it is rendered into a new file instead.
            self.arrangement.clips = joined.clips.into_iter().collect();
            self.consolidate_selected_clips(ctx);
        } else if joined.merged == 0 {
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
        let held = self.other_takes_held();
        let (now, too_long) = self.document.unloop_clips(&ids);
        self.say_dropped_takes(held);
        if too_long {
            self.say("A clip repeats too many times to unloop".to_owned());
        }
        if !now.is_empty() {
            self.arrangement.clips = now.into_iter().collect();
            self.arrangement.clip_cursor = None;
        }
    }

    /// How many passes the project's clips hold besides the ones they play.
    fn other_takes_held(&self) -> u32 {
        self.document
            .project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter_map(|c| c.takes.as_ref())
            .map(|t| t.count - 1)
            .sum()
    }

    /// Says so when an edit that rewrites clips left them with fewer takes
    /// than `before` (see `other_takes_held`): the clip keeps the pass it
    /// plays, and undo brings the rest back.
    fn say_dropped_takes(&mut self, before: u32) {
        let dropped = before.saturating_sub(self.other_takes_held());
        if dropped > 0 {
            self.say(format!(
                "{} other {} removed with the clip's change. Undo brings {} back.",
                dropped,
                if dropped == 1 {
                    "take was"
                } else {
                    "takes were"
                },
                if dropped == 1 { "it" } else { "them" },
            ));
        }
    }

    /// Plays the next or previous take of the selected clips that have them.
    pub(crate) fn step_selected_takes(&mut self, by: i32) {
        let ids = self.selected_clip_ids();
        let Some(now) = self.document.step_takes(&ids, by) else {
            self.say("Select a clip that has takes".to_owned());
            return;
        };
        let count = self
            .document
            .project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .find_map(|c| c.takes.as_ref().filter(|_| ids.contains(&c.id)))
            .map_or(0, |t| t.count);
        self.say(format!("Take {} of {}", now + 1, count));
    }

    /// Plays take `take` (from 0) of the clip, from its Takes menu.
    pub(crate) fn play_clip_take(&mut self, clip: Id, take: u32) {
        if self.document.play_take(clip, take) {
            self.say(format!("Playing take {}", take + 1));
        }
    }

    /// Throws away the other takes of the selected clips.
    pub(crate) fn keep_only_selected_take(&mut self) {
        let ids = self.selected_clip_ids();
        let dropped = self.document.keep_only_take(&ids);
        if dropped > 0 {
            self.say(format!(
                "Kept the playing take; {} other {} removed. Undo brings {} back.",
                dropped,
                if dropped == 1 {
                    "take was"
                } else {
                    "takes were"
                },
                if dropped == 1 { "it" } else { "them" },
            ));
        }
    }

    /// Puts take `take` (from 0) of the audio clip on a new track after its
    /// own.
    pub(crate) fn extract_clip_take(&mut self, clip: Id, take: u32) {
        if let Some(made) = self.document.extract_take(clip, take) {
            self.arrangement.clips = std::iter::once(made.clip).collect();
            self.arrangement.clip_cursor = None;
            self.say(format!("Take {} is on a new track", take + 1));
        }
    }

    /// Plays the selected audio clips backwards, or forwards again.
    pub(crate) fn reverse_selected_clips(&mut self) {
        let ids = self.selected_clip_ids();
        let held = self.other_takes_held();
        let done = self.document.reverse_clips(&ids);
        self.say_dropped_takes(held);
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
