//! Clips glide to where an edit puts them (the interface audit's M1;
//! DESIGN §6 "clips settling"): undo, redo, commands and the snapped steps
//! of a drag (moves, not trims: a trimmed edge follows the hand). What is heard changes at once; only the drawing travels, and
//! clicks go by where the clip is, not where it is drawn. After undo or
//! redo, what changed is also washed in the accent for a moment (M4).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use lumen_core::time::Ticks;
use lumen_project::model::{Clip, Id, Project};

use crate::motion;

/// How long the accent wash on what undo or redo changed takes to fade.
pub(super) const WASH: f64 = motion::WASH as f64;
/// The wash's strength as it starts.
pub(super) const WASH_ALPHA: f32 = 0.42;

#[derive(Debug, Clone, Copy)]
struct Glide {
    /// How far from its place the clip is drawn as the glide starts.
    from: i64,
    since: f64,
    duration: f32,
}

/// The clips moving to new places, and what the last undo or redo changed.
#[derive(Debug, Default)]
pub(super) struct ClipGlides {
    /// The document session and state the starts below are from.
    seen: Option<(u64, u64)>,
    /// Each clip's start in the document, and what it is (to tell what
    /// changed).
    clips: HashMap<Id, Clip>,
    gliding: HashMap<Id, Glide>,
    /// Clips that the last undo or redo changed, and when.
    washed: Option<(HashSet<Id>, f64)>,
    /// The next change is an undo or a redo.
    undoing: bool,
    /// A recording was under way at the last update: the commit that ends
    /// it lands as it stops, and does not glide either.
    was_recording: bool,
}

impl ClipGlides {
    /// The next document change comes from undo or redo: what it changes is
    /// washed in the accent.
    pub(super) fn note_undo(&mut self) {
        self.undoing = true;
    }

    /// Notes clips that moved since the last frame, given the document's
    /// `session` and `state`. A drag's steps glide for `duration`; other
    /// edits for `GLIDE`. While `recording`, nothing glides: a take's
    /// commits cut the clips under it about once a second, and a clip that
    /// slid after each would never sit still.
    pub(super) fn update(
        &mut self,
        now: f64,
        (session, state): (u64, u64),
        project: &Project,
        (dragging, recording): (bool, bool),
    ) {
        self.gliding
            .retain(|_, g| now - g.since < f64::from(g.duration));
        if let Some((_, since)) = &self.washed
            && now - since > WASH
        {
            self.washed = None;
        }
        let was_recording = std::mem::replace(&mut self.was_recording, recording);
        let recording = recording || was_recording;
        if self.seen == Some((session, state)) {
            return;
        }
        let same_document = self.seen.is_some_and(|(s, _)| s == session);
        self.seen = Some((session, state));
        let undoing = std::mem::take(&mut self.undoing);
        if !same_document {
            self.gliding.clear();
            self.washed = None;
            self.clips = all_clips(project).map(|c| (c.id, c.clone())).collect();
            return;
        }
        let duration = if recording {
            0.0
        } else {
            motion::time(if dragging {
                motion::FAST
            } else {
                motion::GLIDE
            })
        };
        let mut changed = HashSet::new();
        let mut now_clips = HashMap::with_capacity(self.clips.len());
        for clip in all_clips(project) {
            if let Some(before) = self.clips.get(&clip.id) {
                // Only a moved clip glides. A trimmed start lands at once:
                // its length has already changed, so gliding the start
                // alone would shake the other edge and the notes inside
                // (MIDI notes are in clip time) on every step of the drag.
                let moved = before.start != clip.start && before.length == clip.length;
                if moved && duration > 0.0 {
                    let shown = before.start.0 + self.offset_at(clip.id, now);
                    self.gliding.insert(
                        clip.id,
                        Glide {
                            from: shown - clip.start.0,
                            since: now,
                            duration,
                        },
                    );
                }
                if before != clip {
                    changed.insert(clip.id);
                }
            } else {
                changed.insert(clip.id);
            }
            now_clips.insert(clip.id, clip.clone());
        }
        self.gliding.retain(|id, _| now_clips.contains_key(id));
        self.clips = now_clips;
        if undoing && !changed.is_empty() {
            self.washed = Some((changed, now));
        }
    }

    /// Ticks from its place a clip is drawn at `now`.
    fn offset_at(&self, id: Id, now: f64) -> i64 {
        self.gliding.get(&id).map_or(0, |g| {
            let t = ((now - g.since) / f64::from(g.duration).max(1e-6)) as f32;
            (g.from as f64 * f64::from(1.0 - motion::spring(t))).round() as i64
        })
    }

    /// Whether anything is moving or washed: draw the next frame too.
    pub(super) fn moving(&self) -> bool {
        !self.gliding.is_empty() || self.washed.is_some()
    }

    /// The project as drawn at `now`: the clips that glide where they are
    /// on their way. The project itself when nothing moves.
    pub(super) fn shown<'a>(&self, project: &'a Project, now: f64) -> Cow<'a, Project> {
        if self.gliding.is_empty() {
            return Cow::Borrowed(project);
        }
        let mut shown = project.clone();
        // Only the tracks and clips that move are copied: the rest stay
        // shared with the document.
        for track in &mut shown.tracks {
            if track.clips.iter().any(|c| self.gliding.contains_key(&c.id)) {
                let track = std::sync::Arc::make_mut(track);
                for clip in &mut track.clips {
                    let offset = self.offset_at(clip.id, now);
                    if offset != 0 {
                        std::sync::Arc::make_mut(clip).start = Ticks(clip.start.0 + offset);
                    }
                }
            }
        }
        Cow::Owned(shown)
    }

    /// How strongly clip `id` is washed at `now` (0 when it is not).
    pub(super) fn wash(&self, id: Id, now: f64) -> f32 {
        match &self.washed {
            Some((ids, since)) if ids.contains(&id) => {
                let left = (1.0 - (now - since) / WASH).clamp(0.0, 1.0) as f32;
                WASH_ALPHA * (1.0 - motion::ease_out(1.0 - left))
            }
            _ => 0.0,
        }
    }
}

fn all_clips(project: &Project) -> impl Iterator<Item = &Clip> {
    project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| &**c))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project(start: i64) -> Project {
        let mut project = lumen_project::testing::small_project(1);
        let track = project
            .tracks
            .iter_mut()
            .find(|t| !t.clips.is_empty())
            .expect("a track with clips");
        let track = std::sync::Arc::make_mut(track);
        track.clips.truncate(1);
        std::sync::Arc::make_mut(&mut track.clips[0]).start = Ticks(start);
        project
    }

    fn first(p: &Project) -> &Clip {
        all_clips(p).next().expect("a clip")
    }

    fn start_of(p: &Project) -> i64 {
        first(p).start.0
    }

    #[test]
    fn nothing_glides_while_recording() {
        let mut glides = ClipGlides::default();
        glides.update(0.0, (1, 1), &project(0), (false, false));
        let moved = project(1000);
        glides.update(1.0, (1, 2), &moved, (false, true));
        assert!(!glides.moving());
        assert_eq!(start_of(&glides.shown(&moved, 1.0)), 1000);
        // The last commit, as the recording stops, stays put too.
        glides.update(2.0, (1, 3), &project(2000), (false, false));
        assert!(!glides.moving());
        // The next edit glides again.
        glides.update(3.0, (1, 4), &project(0), (false, false));
        assert!(glides.moving());
    }

    #[test]
    fn a_moved_clip_glides_from_where_it_was_to_where_it_is() {
        let mut glides = ClipGlides::default();
        glides.update(0.0, (1, 1), &project(0), (false, false));
        let moved = project(1000);
        glides.update(1.0, (1, 2), &moved, (false, false));
        assert_eq!(
            start_of(&glides.shown(&moved, 1.0)),
            0,
            "starts where it was"
        );
        let mid = start_of(&glides.shown(&moved, 1.0 + 0.05));
        assert!((1..1000).contains(&mid), "on its way: {mid}");
        glides.update(2.0, (1, 2), &moved, (false, false));
        assert!(!glides.moving());
        assert_eq!(start_of(&glides.shown(&moved, 2.0)), 1000, "and lands");
    }

    #[test]
    fn a_trimmed_start_lands_at_once() {
        let mut glides = ClipGlides::default();
        let before = project(0);
        glides.update(0.0, (1, 1), &before, (false, false));
        // The start edge dragged later: the start moves and the clip is
        // shorter, the end staying put.
        let mut trimmed = before.clone();
        {
            let track = trimmed
                .tracks
                .iter_mut()
                .find(|t| !t.clips.is_empty())
                .expect("a track with clips");
            let clip = std::sync::Arc::make_mut(&mut std::sync::Arc::make_mut(track).clips[0]);
            clip.start = Ticks(480);
            clip.length = Ticks(clip.length.0 - 480);
        }
        glides.update(1.0, (1, 2), &trimmed, (true, false));
        assert!(!glides.moving(), "a trim does not glide");
        assert_eq!(start_of(&glides.shown(&trimmed, 1.0)), 480);
    }

    #[test]
    fn another_document_never_glides_and_undo_washes_what_changed() {
        let mut glides = ClipGlides::default();
        glides.update(0.0, (1, 1), &project(0), (false, false));
        // A different project with the same ids: no glide.
        glides.update(1.0, (2, 1), &project(5000), (false, false));
        assert!(!glides.moving());
        glides.note_undo();
        glides.update(2.0, (2, 2), &project(0), (false, false));
        let id = first(&project(0)).id;
        assert!(glides.wash(id, 2.0) > 0.4);
        assert!(glides.wash(id, 2.0 + WASH) < 0.01);
    }
}
