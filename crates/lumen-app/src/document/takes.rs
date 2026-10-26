//! Takes of M9 slice R3: playing another pass of a loop recording, keeping
//! only one, and extracting one to a new track. Each is one undo step; one
//! that changes nothing adds none.

use lumen_project::edit::takes;
use lumen_project::model::Id;

use super::Document;
use super::clips::fresh_ids;

impl Document {
    /// Plays pass `take` (from 0) of the clip. Returns whether it changed.
    pub fn play_take(&mut self, id: Id, take: u32) -> bool {
        let mut done = false;
        self.history.apply("Play take", None, |project| {
            done = takes::play(project, id, take);
        });
        done
    }

    /// Plays the take `by` places after the one playing on each unlocked
    /// clip among `ids`, round the set (see `lumen_project::edit::takes::step`).
    /// Returns the number now playing on the first that changed.
    pub fn step_takes(&mut self, ids: &[Id], by: i32) -> Option<u32> {
        let mut out = None;
        let label = if by < 0 { "Previous take" } else { "Next take" };
        self.history.apply(label, None, |project| {
            for &id in ids {
                let now = takes::step(project, id, by);
                out = out.or(now);
            }
        });
        out
    }

    /// Drops the other takes of the unlocked clips among `ids`. Returns how
    /// many were dropped.
    pub fn keep_only_take(&mut self, ids: &[Id]) -> u32 {
        let mut dropped = 0;
        self.history.apply("Keep only this take", None, |project| {
            dropped = takes::keep_only(project, ids);
        });
        dropped
    }

    /// Puts pass `take` of the audio clip on a new track after its own.
    pub fn extract_take(&mut self, id: Id, take: u32) -> Option<takes::Extracted> {
        let mut out = None;
        let next = &mut self.next_id;
        self.history.apply("Extract take", None, |project| {
            out = takes::extract(project, id, take, &mut fresh_ids(next));
        });
        out
    }
}
