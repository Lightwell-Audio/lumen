//! Making MIDI clips (M5 slice 3a): the clip a double-click on an empty MIDI
//! lane, or the Create MIDI clip command, puts on a track.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::edit::{self, MIN_LENGTH};
use lumen_project::model::{Clip, ClipContent, Id, TrackKind};

use super::Document;
use super::clips::fresh_ids;

/// Where a new MIDI clip goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewClip {
    /// Over a time range (a time selection). The ends may come in either
    /// order; a start before zero is moved to zero, and a range shorter than
    /// the shortest clip is lengthened to it.
    Range(Ticks, Ticks),
    /// One bar long, starting at the bar line at or before this time.
    Bar(Ticks),
}

impl Document {
    /// Puts an empty MIDI clip on the MIDI track `track` and returns its
    /// id. It cuts away the unlocked clips it covers, as every clip put
    /// down does; if a locked clip is in the way, or `track` is not a MIDI
    /// track, nothing happens and there is no id. One undo step.
    pub fn create_midi_clip(&mut self, track: Id, place: NewClip) -> Option<Id> {
        let project = self.project();
        let target = project
            .tracks
            .iter()
            .find(|t| t.id == track && t.kind == TrackKind::Midi)?;
        let (start, length) = match place {
            NewClip::Range(a, b) => {
                let (a, b) = (a.min(b).max(Ticks::ZERO), a.max(b));
                (a, (b - a).max(MIN_LENGTH))
            }
            NewClip::Bar(at) => {
                // The bar the time falls in, whatever the meter is there.
                let map = project.meter_map();
                let bar = map.bbt_at(at.max(Ticks::ZERO)).bar;
                let start = map.bar_start(bar);
                (start, map.bar_start(bar + 1) - start)
            }
        };
        let end = start + length;
        if target
            .clips
            .iter()
            .any(|c| c.locked && c.start < end && c.start + c.length > start)
        {
            return None;
        }
        let id = self.fresh_id();
        let next = &mut self.next_id;
        self.history.apply("Create MIDI clip", None, |project| {
            if let Some(t) = project.tracks.iter_mut().find(|t| t.id == track) {
                let t = Arc::make_mut(t);
                t.clips.push(Arc::new(Clip {
                    id,
                    name: "MIDI clip".into(),
                    start,
                    length,
                    muted: false,
                    color: None,
                    locked: false,
                    fade_in: Default::default(),
                    fade_out: Default::default(),
                    loop_length: None,
                    loop_phase: Default::default(),
                    envelopes: Default::default(),
                    audio_phase_seconds: 0.0,
                    reversed: false,
                    inverted: false,
                    content: ClipContent::midi(),
                }));
                t.clips.sort_by_key(|c| c.start);
            }
            edit::overwrite(project, &[id], &mut fresh_ids(next));
        });
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_midi_track() -> (Document, Id) {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Midi, None);
        (doc, track)
    }

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn clips(doc: &Document) -> Vec<(Ticks, Ticks)> {
        doc.project().tracks[0]
            .clips
            .iter()
            .map(|c| (c.start, c.length))
            .collect()
    }

    #[test]
    fn a_bar_clip_starts_on_the_bar_line_and_is_one_bar() {
        let (mut doc, track) = doc_with_midi_track();
        let id = doc
            .create_midi_clip(track, NewClip::Bar(q(5) + Ticks(7)))
            .unwrap();
        assert_eq!(clips(&doc), [(q(4), q(4))]);
        assert_eq!(doc.project().tracks[0].clips[0].id, id);
        assert!(matches!(
            doc.project().tracks[0].clips[0].content,
            ClipContent::Midi { .. }
        ));
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
        doc.undo();
        assert!(clips(&doc).is_empty(), "one undo step");
        doc.redo();
        assert_eq!(clips(&doc), [(q(4), q(4))]);
    }

    #[test]
    fn a_bar_follows_the_time_signature() {
        let (mut doc, track) = doc_with_midi_track();
        doc.set_time_signature_at(Ticks::ZERO, 6, 8, None);
        doc.create_midi_clip(track, NewClip::Bar(q(4)));
        // A 6/8 bar is three quarters.
        assert_eq!(clips(&doc), [(q(3), q(3))]);
        doc.create_midi_clip(track, NewClip::Bar(Ticks(-5)));
        assert_eq!(clips(&doc)[0], (Ticks::ZERO, q(3)));
    }

    #[test]
    fn a_range_clip_covers_the_range_in_either_order() {
        let (mut doc, track) = doc_with_midi_track();
        doc.create_midi_clip(track, NewClip::Range(q(10), q(6)));
        assert_eq!(clips(&doc), [(q(6), q(4))]);
        doc.create_midi_clip(track, NewClip::Range(q(-3), q(2)));
        assert_eq!(clips(&doc)[0], (Ticks::ZERO, q(2)));
        doc.create_midi_clip(track, NewClip::Range(q(20), q(20)));
        assert_eq!(clips(&doc)[2], (q(20), MIN_LENGTH));
    }

    #[test]
    fn a_new_clip_overwrites_unlocked_clips_but_never_locked_ones() {
        let (mut doc, track) = doc_with_midi_track();
        let old = doc
            .create_midi_clip(track, NewClip::Range(q(0), q(8)))
            .unwrap();
        doc.create_midi_clip(track, NewClip::Range(q(2), q(4)));
        // The old clip is cut around the new one.
        assert_eq!(clips(&doc), [(q(0), q(2)), (q(2), q(2)), (q(4), q(4))]);
        assert_eq!(doc.project().tracks[0].clips[0].id, old);
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));

        doc.set_clips_locked(&[old], true);
        let before = doc.project().clone();
        let state = doc.state();
        assert_eq!(
            doc.create_midi_clip(track, NewClip::Range(q(1), q(3))),
            None
        );
        assert_eq!(doc.state(), state, "nothing changed");
        assert_eq!(*doc.project(), before);
        // Beside the locked clip is fine.
        assert!(
            doc.create_midi_clip(track, NewClip::Range(q(8), q(12)))
                .is_some()
        );
    }

    #[test]
    fn only_midi_tracks_take_midi_clips() {
        let mut doc = Document::new(crate::empty_project());
        let audio = doc.add_track(TrackKind::Audio, None);
        let state = doc.state();
        assert_eq!(doc.create_midi_clip(audio, NewClip::Bar(q(0))), None);
        assert_eq!(doc.create_midi_clip(Id(9999), NewClip::Bar(q(0))), None);
        assert_eq!(doc.state(), state);
    }
}
