use lumen_project::edit::takes;
use lumen_project::model::Id;

use super::Document;
use super::clips::fresh_ids;

impl Document {
    pub fn play_take(&mut self, id: Id, take: u32) -> bool {
        let mut done = false;
        self.history.apply("Play take", None, |project| {
            done = takes::play(project, id, take);
        });
        done
    }

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

    pub fn keep_only_take(&mut self, ids: &[Id]) -> u32 {
        let mut dropped = 0;
        self.history.apply("Keep only this take", None, |project| {
            dropped = takes::keep_only(project, ids);
        });
        dropped
    }

    pub fn extract_take(&mut self, id: Id, take: u32) -> Option<takes::Extracted> {
        let mut out = None;
        let next = &mut self.next_id;
        self.history.apply("Extract take", None, |project| {
            out = takes::extract(project, id, take, &mut fresh_ids(next));
        });
        out
    }
}
