use std::sync::Arc;

use lumen_core::fade::Fade;
use lumen_core::time::Ticks;
use lumen_project::edit::{self, Edge};
use lumen_project::model::{Clip, ClipContent, Id, Project};

use super::Document;

impl Document {
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

    pub fn crossfade_across(&mut self, right: Id, from: Ticks, until: Ticks, merge: Option<&str>) {
        self.history.apply("Crossfade", merge, |project| {
            cross(project, right, from, until);
        });
    }

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
        assert!((b2.audio_phase_seconds - b.audio_phase_seconds - 0.25).abs() < 1e-9);
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
