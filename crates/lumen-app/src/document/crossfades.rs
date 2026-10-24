//! Undoable crossfades (M7b E2, ADR-0094). The rules and the shape of a
//! crossfade are in `crate::crossfades`; these are the edits that make,
//! resize and remove one.

use std::sync::Arc;

use lumen_core::fade::Fade;
use lumen_core::time::Ticks;
use lumen_project::edit::{self, Edge};
use lumen_project::model::{Clip, ClipContent, Id, Project};

use super::Document;

impl Document {
    /// Enables or disables automatic crossfades for eligible audio joins.
    /// The setting is part of the project history.
    pub fn set_auto_crossfade(&mut self, enabled: bool) {
        let label = if enabled {
            "Enable automatic crossfades"
        } else {
            "Disable automatic crossfades"
        };
        self.history.apply(label, None, |project| {
            project.auto_crossfade = enabled;
        });
    }

    /// Makes (or resizes) the crossfade into `right` by dragging the end of
    /// the clip before it, which met `right` at `from`, to `until`: that clip
    /// is extended to `until` and `right` is trimmed to start there, so the
    /// crossfade lasts from `from` to `until` and ends where `right` starts.
    /// Dragged back to `from` the two are as they were. The gesture passes
    /// a `merge` key and is one undo step.
    pub fn crossfade_across(&mut self, right: Id, from: Ticks, until: Ticks, merge: Option<&str>) {
        self.history.apply("Crossfade", merge, |project| {
            cross(project, right, from, until);
        });
    }

    /// Sets the length of the crossfade into `right`, the clips staying
    /// where they are (the outgoing clip's fade-out handle): as long as the
    /// two have audio for, and zero removes it.
    pub fn set_crossfade_length(&mut self, right: Id, length: Ticks, merge: Option<&str>) {
        self.history.apply("Crossfade length", merge, |project| {
            let Some((track, clip)) = locate(project, right) else {
                return;
            };
            let room = crate::crossfades::room(project, track, &clip);
            let length = length.clamp(Ticks::ZERO, room);
            replace(project, right, |c| {
                c.crossfade = length > Ticks::ZERO;
                c.fade_in.length = length;
                c.fade_in.offset = Ticks::ZERO;
                c.fade_in.span = Ticks::ZERO;
            });
        });
    }

    /// Makes a short crossfade into each of `ids` (as dragging the clip
    /// before it 5 ms over would), or removes theirs.
    pub fn set_crossfade(&mut self, ids: &[Id], enabled: bool) {
        self.history.apply("Crossfade", None, |project| {
            for &id in ids {
                let Some((_, clip)) = locate(project, id) else {
                    continue;
                };
                if enabled {
                    if !clip.crossfade {
                        let tempo = edit::tempo_of(project);
                        let begins = tempo.seconds_at(clip.start);
                        let five = tempo.ticks_at_seconds(begins + 0.005) - clip.start;
                        cross(project, id, clip.start, clip.start + five);
                    }
                } else {
                    replace(project, id, |c| {
                        c.crossfade = false;
                        c.fade_in.length = Ticks::ZERO;
                    });
                }
            }
        });
    }
}

/// The unlocked audio clip `id` and its track.
fn locate(project: &Project, id: Id) -> Option<(Id, Clip)> {
    project.tracks.iter().find_map(|t| {
        t.clips
            .iter()
            .find(|c| c.id == id && !c.locked && matches!(c.content, ClipContent::Audio { .. }))
            .map(|c| (t.id, (**c).clone()))
    })
}

fn replace(project: &mut Project, id: Id, change: impl FnOnce(&mut Clip)) {
    for track in &mut project.tracks {
        if let Some(i) = track.clips.iter().position(|c| c.id == id) {
            change(Arc::make_mut(&mut Arc::make_mut(track).clips[i]));
            return;
        }
    }
}

/// The crossfade into `right`, made over the end of the audio clip that ends
/// where `right` starts: that clip is trimmed to end at `until` (no earlier
/// than `from`) and `right` to start there, which lengthens the first and
/// shortens the second, each as far as its audio and length allow.
fn cross(project: &mut Project, right: Id, from: Ticks, until: Ticks) {
    let Some(t) = project
        .tracks
        .iter()
        .position(|t| t.clips.iter().any(|c| c.id == right))
    else {
        return;
    };
    let clips = &project.tracks[t].clips;
    let Some(next) = clips.iter().find(|c| c.id == right) else {
        return;
    };
    let Some(first) = clips
        .iter()
        .find(|c| c.id != right && c.start + c.length == next.start && c.length > Ticks::ZERO)
    else {
        return;
    };
    let audio = |c: &Clip| matches!(c.content, ClipContent::Audio { .. }) && !c.locked;
    if !audio(first) || !audio(next) || next.loop_length.is_some() {
        return;
    }
    let tempo = edit::tempo_of(project);
    let mut first_new = edit::trim(project, &tempo, first, Edge::End, until.max(from), false);
    let mut next_new = edit::trim(
        project,
        &tempo,
        next,
        Edge::Start,
        first_new.start + first_new.length,
        false,
    );
    // A clip too short to give up that much: they meet where it can.
    let meeting = next_new.start;
    if meeting < first_new.start + first_new.length {
        first_new = edit::trim(project, &tempo, &first_new, Edge::End, meeting, false);
    }
    let length = (meeting - from).max(Ticks::ZERO);
    next_new.crossfade = length > Ticks::ZERO;
    next_new.fade_in = Fade {
        length,
        curve: next.fade_in.curve,
        ..Fade::default()
    };
    let (first_id, next_id) = (first.id, next.id);
    let track = Arc::make_mut(&mut project.tracks[t]);
    for clip in &mut track.clips {
        if clip.id == first_id {
            *clip = Arc::new(first_new.clone());
        } else if clip.id == next_id {
            *clip = Arc::new(next_new.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::document::tests::file;

    /// A two-second file split at beat 2 (120 BPM): two clips side by side
    /// that continue each other.
    fn split_pair() -> (Document, Id, Id) {
        let mut doc = Document::new(crate::empty_project());
        let media = doc.fresh_id();
        doc.import(vec![file(media, "Audio", Ticks::ZERO)]);
        let id = doc.project().tracks[0].clips[0].id;
        doc.split_clips(&[id], &[Ticks::quarters(2)]);
        let clips = &doc.project().tracks[0].clips;
        assert_eq!(clips.len(), 2);
        let ids = (clips[0].id, clips[1].id);
        (doc, ids.0, ids.1)
    }

    fn clip(doc: &Document, id: Id) -> Clip {
        doc.project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .find(|c| c.id == id)
            .map(|c| (**c).clone())
            .expect("clip exists")
    }

    #[test]
    fn dragging_the_left_clip_over_the_right_one_extends_it_and_starts_the_right_one_at_the_end() {
        let (mut doc, left, right) = split_pair();
        let (a, b) = (clip(&doc, left), clip(&doc, right));
        let before = doc.project().clone();
        let meet = Ticks::quarters(2);
        let half = Ticks(Ticks::quarters(1).0 / 2);
        for until in [meet + Ticks(half.0 / 2), meet + half] {
            doc.crossfade_across(right, meet, until, Some("clip fade"));
        }
        let (a2, b2) = (clip(&doc, left), clip(&doc, right));
        assert_eq!(a2.start, a.start);
        assert_eq!(
            a2.start + a2.length,
            meet + half,
            "the left clip is extended"
        );
        assert_eq!(b2.start, meet + half, "the right clip starts where it ends");
        assert_eq!(b2.start + b2.length, b.start + b.length, "its end stays");
        assert!(b2.crossfade);
        assert_eq!(b2.fade_in.length, half);
        // Its sound is unmoved: a quarter of a second further into the file.
        assert!((b2.audio_phase_seconds - b.audio_phase_seconds - 0.25).abs() < 1e-9);
        // The whole drag is one step.
        doc.undo();
        assert_eq!(*doc.project(), before);
    }

    #[test]
    fn dragging_back_to_where_they_met_leaves_them_as_they_were() {
        let (mut doc, left, right) = split_pair();
        let (a, b) = (clip(&doc, left), clip(&doc, right));
        let meet = Ticks::quarters(2);
        for until in [meet + Ticks::quarters(1), meet] {
            doc.crossfade_across(right, meet, until, Some("clip fade"));
        }
        let (a2, b2) = (clip(&doc, left), clip(&doc, right));
        assert_eq!((a2.start, a2.length), (a.start, a.length));
        assert_eq!((b2.start, b2.length), (b.start, b.length));
        assert!(!b2.crossfade);
        assert!((b2.audio_phase_seconds - b.audio_phase_seconds).abs() < 1e-9);
    }

    #[test]
    fn the_clips_meet_where_they_can_when_dragged_too_far() {
        let (mut doc, left, right) = split_pair();
        doc.crossfade_across(right, Ticks::quarters(2), Ticks::quarters(40), None);
        let (a, b) = (clip(&doc, left), clip(&doc, right));
        assert_eq!(a.start + a.length, b.start, "they still touch");
        assert!(b.length >= edit::MIN_LENGTH && b.length < Ticks::quarters(2));
        assert!(b.crossfade);
        assert_eq!(b.fade_in.length, b.start - Ticks::quarters(2));
    }

    #[test]
    fn the_handle_resizes_a_crossfade_and_zero_removes_it() {
        let (mut doc, left, right) = split_pair();
        let meet = Ticks::quarters(2);
        let half = Ticks(Ticks::quarters(1).0 / 2);
        doc.crossfade_across(right, meet, meet + half, None);
        let at = clip(&doc, right).start;
        doc.set_crossfade_length(right, Ticks(half.0 / 2), Some("clip fade"));
        let b = clip(&doc, right);
        assert_eq!(b.fade_in.length, Ticks(half.0 / 2));
        assert_eq!(b.start, at, "the clips stay where they are");
        // More than the two have audio for is held to what they have.
        doc.set_crossfade_length(right, Ticks::quarters(40), None);
        let b = clip(&doc, right);
        assert!(b.fade_in.length <= clip(&doc, left).length);
        assert!(b.fade_in.length > half, "but more is allowed");
        doc.set_crossfade_length(right, Ticks::ZERO, None);
        let b = clip(&doc, right);
        assert!(!b.crossfade);
        assert_eq!(b.fade_in.length, Ticks::ZERO);
        assert_eq!(b.start, at);
    }

    #[test]
    fn moving_either_clip_away_ends_the_crossfade() {
        let (mut doc, left, right) = split_pair();
        let meet = Ticks::quarters(2);
        doc.crossfade_across(right, meet, meet + Ticks::quarters(1), None);
        assert!(clip(&doc, right).crossfade);
        doc.apply("move", None, |project| {
            Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[1]).start =
                Ticks::quarters(9);
        });
        let moved = clip(&doc, right);
        assert!(!moved.crossfade, "an ordinary clip again");
        assert_eq!(moved.fade_in.length, Ticks::ZERO);
        let _ = left;
    }

    #[test]
    fn the_command_crossfades_a_little_and_toggles_off() {
        let (mut doc, _, right) = split_pair();
        let start = clip(&doc, right).start;
        doc.set_crossfade(&[right], true);
        let b = clip(&doc, right);
        assert!(b.crossfade);
        let tempo = edit::tempo_of(doc.project());
        let seconds = tempo.seconds_at(b.start) - tempo.seconds_at(start);
        assert!((seconds - 0.005).abs() < 1e-4, "{seconds}");
        assert_eq!(b.fade_in.length, b.start - start);
        doc.set_crossfade(&[right], false);
        assert!(!clip(&doc, right).crossfade);
        doc.undo();
        assert!(clip(&doc, right).crossfade);
        doc.undo();
        assert!(!clip(&doc, right).crossfade);
        assert_eq!(clip(&doc, right).start, start);
    }

    #[test]
    fn locked_midi_and_lone_clips_are_left_alone() {
        let (mut doc, left, right) = split_pair();
        let lone = clip(&doc, left);
        doc.set_crossfade(&[left], true);
        assert_eq!(clip(&doc, left), lone, "nothing before it to cross from");
        doc.set_clips_locked(&[right], true);
        let locked = clip(&doc, right);
        doc.crossfade_across(right, Ticks::quarters(2), Ticks::quarters(3), None);
        doc.set_crossfade(&[right], true);
        doc.set_crossfade_length(right, Ticks(100), None);
        assert_eq!(clip(&doc, right), locked);

        let project = lumen_project::testing::project_of(
            "MIDI",
            vec![lumen_project::testing::midi_track(
                40,
                "Keys",
                "poly",
                (0, 4),
                None,
                Vec::new(),
                Vec::new(),
            )],
        );
        let mut midi = Document::new(project);
        let id = midi.project().tracks[0].clips[0].id;
        let before = midi.project().tracks[0].clips[0].clone();
        midi.set_crossfade(&[id], true);
        midi.set_crossfade_length(id, Ticks(100), None);
        assert_eq!(*midi.project().tracks[0].clips[0], *before);
    }

    #[test]
    fn extending_the_left_clip_moves_the_crossfade_along_at_the_same_length() {
        let (mut doc, left, right) = split_pair();
        let meet = Ticks::quarters(2);
        let half = Ticks(Ticks::quarters(1).0 / 2);
        doc.crossfade_across(right, meet, meet + half, None);
        let (a, b) = (clip(&doc, left), clip(&doc, right));
        let base = doc.project().clone();
        let to = a.start + a.length + half;
        doc.trim_clip(&base, left, Edge::End, to, false, None);
        let (a2, b2) = (clip(&doc, left), clip(&doc, right));
        assert_eq!(a2.start + a2.length, to);
        assert_eq!(b2.start, to, "the right clip starts where it ends");
        assert!(b2.crossfade);
        assert_eq!(b2.fade_in.length, b.fade_in.length, "as long as it was");
        assert_eq!(b2.start + b2.length, b.start + b.length);
        let _ = a;
    }

    #[test]
    fn the_automatic_setting_is_part_of_the_history() {
        let mut doc = Document::new(crate::empty_project());
        assert!(doc.project().auto_crossfade);
        doc.set_auto_crossfade(false);
        assert!(!doc.project().auto_crossfade);
        doc.undo();
        assert!(doc.project().auto_crossfade);
    }
}
