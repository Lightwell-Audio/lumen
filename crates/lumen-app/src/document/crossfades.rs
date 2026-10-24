//! Undoable crossfade requests and their incoming fade duration.

use std::sync::Arc;

use lumen_core::curve::Curve;
use lumen_core::fade::Fade;
use lumen_core::time::{TempoMap, Ticks};
use lumen_project::model::{Clip, ClipContent, Id};

use super::Document;

impl Document {
    /// Enables or disables automatic crossfades for future eligible audio
    /// joins. The setting is part of the project history.
    pub fn set_auto_crossfade(&mut self, enabled: bool) {
        self.history.apply("Automatic crossfades", None, |project| {
            project.auto_crossfade = enabled;
        });
    }

    /// Marks unlocked audio clips with a custom crossfade request. When enabling a clip
    /// with no fade-in duration, it receives a linear fade of about 5 ms at
    /// the tempo at its join. Playback eligibility is checked separately.
    pub fn set_crossfade(&mut self, ids: &[Id], enabled: bool) {
        self.history.apply("Crossfade", None, |project| {
            let tempo = TempoMap::new(project.tempo.clone()).ok();
            for track in &mut project.tracks {
                if !track.clips.iter().any(|clip| {
                    ids.contains(&clip.id)
                        && !clip.locked
                        && matches!(clip.content, ClipContent::Audio { .. })
                }) {
                    continue;
                }
                let track = Arc::make_mut(track);
                for existing in &mut track.clips {
                    if ids.contains(&existing.id)
                        && !existing.locked
                        && matches!(existing.content, ClipContent::Audio { .. })
                    {
                        let clip = Arc::make_mut(existing);
                        clip.crossfade = enabled;
                        if enabled
                            && clip.fade_in.length <= Ticks::ZERO
                            && let Some(tempo) = &tempo
                        {
                            clip.fade_in = initial_crossfade_fade(tempo, clip);
                        }
                    }
                }
            }
        });
    }

    /// Sets an incoming crossfade's duration as one history edit. Passing a
    /// merge key lets the join handle's drag remain one undo gesture.
    pub fn set_crossfade_length(&mut self, id: Id, length: Ticks, merge: Option<&str>) {
        self.history.apply("Crossfade length", merge, |project| {
            for track in &mut project.tracks {
                if !track.clips.iter().any(|clip| {
                    clip.id == id
                        && !clip.locked
                        && matches!(clip.content, ClipContent::Audio { .. })
                }) {
                    continue;
                }
                let track = Arc::make_mut(track);
                if let Some(existing) = track
                    .clips
                    .iter_mut()
                    .find(|clip| clip.id == id && !clip.locked)
                    && let ClipContent::Audio { .. } = existing.content
                {
                    let clip = Arc::make_mut(existing);
                    clip.crossfade = true;
                    let room = (clip.length - clip.fade_out.length).max(Ticks::ZERO);
                    clip.fade_in.length = length.clamp(Ticks::ZERO, room);
                    clip.fade_in.offset = Ticks::ZERO;
                    clip.fade_in.span = Ticks::ZERO;
                }
            }
        });
    }
}

fn initial_crossfade_fade(tempo: &TempoMap, clip: &Clip) -> Fade {
    let start_seconds = tempo.seconds_at(clip.start);
    let end = tempo.ticks_at_seconds(start_seconds + 0.005);
    let room = (clip.length - clip.fade_out.length).max(Ticks::ZERO);
    Fade {
        length: (end - clip.start).max(Ticks(1)).min(room),
        curve: Curve::LINEAR,
        offset: Ticks::ZERO,
        span: Ticks::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::document::tests::file;
    use lumen_core::curve::Curve;
    use lumen_project::edit::Edge;

    #[test]
    fn setting_and_handle_are_undoable_and_reject_locked_or_midi() {
        let mut doc = Document::new(crate::empty_project());
        let media = doc.fresh_id();
        doc.import(vec![file(media, "Audio", Ticks::ZERO)]);
        let id = doc.project().tracks[0].clips[0].id;
        doc.set_fade_curve(&[id], Edge::Start, Curve::S_CURVE, None);
        let original = doc.project().tracks[0].clips[0].clone();
        assert!(doc.project().auto_crossfade);

        doc.set_auto_crossfade(false);
        assert!(!doc.project().auto_crossfade);
        doc.undo();
        assert!(doc.project().auto_crossfade);

        for length in [Ticks(100), Ticks(200), Ticks(300), Ticks::ZERO] {
            doc.set_crossfade_length(id, length, Some("crossfade drag"));
        }
        let changed = &doc.project().tracks[0].clips[0];
        assert!(changed.crossfade);
        assert_eq!(
            changed.fade_in.length,
            Ticks::ZERO,
            "zero disables the join"
        );
        assert_eq!(changed.fade_in.curve, Curve::S_CURVE);
        doc.undo();
        assert_eq!(*doc.project().tracks[0].clips[0], *original);

        doc.apply("lock", None, |project| {
            Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]).locked = true;
        });
        let locked = doc.project().tracks[0].clips[0].clone();
        doc.set_crossfade(&[id], true);
        doc.set_crossfade_length(id, Ticks(500), None);
        doc.set_fade_length(id, Edge::Start, Ticks(500), None);
        assert_eq!(*doc.project().tracks[0].clips[0], *locked);

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
        let mut midi_doc = Document::new(project);
        let midi_id = midi_doc.project().tracks[0].clips[0].id;
        let midi_before = midi_doc.project().tracks[0].clips[0].clone();
        midi_doc.set_crossfade(&[midi_id], true);
        midi_doc.set_crossfade_length(midi_id, Ticks(100), None);
        assert_eq!(*midi_doc.project().tracks[0].clips[0], *midi_before);
    }

    #[test]
    fn enabling_sets_a_five_millisecond_linear_fade_and_undo_restores_exactly() {
        let mut doc = Document::new(crate::empty_project());
        let media = doc.fresh_id();
        doc.import(vec![file(media, "Audio", Ticks::ZERO)]);
        let id = doc.project().tracks[0].clips[0].id;
        let original = doc.project().tracks[0].clips[0].clone();
        doc.set_crossfade(&[id], true);
        let clip = &doc.project().tracks[0].clips[0];
        assert!(clip.crossfade);
        assert_eq!(clip.fade_in.curve, Curve::LINEAR);
        let seconds = clip.fade_in.length.0 as f64 / lumen_core::time::PPQ as f64 * 0.5;
        assert!((seconds - 0.005).abs() < 0.00001, "{seconds}s");
        doc.undo();
        assert_eq!(*doc.project().tracks[0].clips[0], *original);
    }
}
