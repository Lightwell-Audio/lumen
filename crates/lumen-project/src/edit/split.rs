//! Pure clip splitting at arrangement ticks.

use std::collections::HashSet;
use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::model::{Id, Project};

/// Splits unlocked clips at interior ticks, keeping each source id on its
/// first part. The returned ids cover the resulting parts in track order.
pub fn clips(
    project: &mut Project,
    ids: &[Id],
    at: &[Ticks],
    fresh: &mut impl FnMut() -> Id,
) -> Vec<Id> {
    if ids.is_empty() || at.is_empty() {
        return Vec::new();
    }
    let selected: HashSet<Id> = ids.iter().copied().collect();
    let mut cuts = at.to_vec();
    cuts.sort_unstable();
    cuts.dedup();
    let tempo = super::tempo_of(project);
    let source_project = project.clone();
    let mut result = Vec::new();

    for track_index in 0..project.tracks.len() {
        let track = &project.tracks[track_index];
        let affected = track.clips.iter().any(|clip| {
            if !selected.contains(&clip.id) || clip.locked {
                return false;
            }
            let end = clip.start + clip.length;
            cuts.iter().any(|cut| clip.start < *cut && *cut < end)
        });
        if !affected {
            result.extend(
                track
                    .clips
                    .iter()
                    .filter(|clip| selected.contains(&clip.id))
                    .map(|clip| clip.id),
            );
            continue;
        }

        let original = track.clips.clone();
        let mut replacement = Vec::with_capacity(original.len());
        for clip in &original {
            if !selected.contains(&clip.id) {
                replacement.push(Arc::clone(clip));
                continue;
            }
            if clip.locked {
                result.push(clip.id);
                replacement.push(Arc::clone(clip));
                continue;
            }
            let start = clip.start;
            let end = clip.start + clip.length;
            let boundaries: Vec<_> = cuts
                .iter()
                .copied()
                .filter(|cut| start < *cut && *cut < end)
                .collect();
            if boundaries.is_empty() {
                result.push(clip.id);
                replacement.push(Arc::clone(clip));
                continue;
            }

            let mut left = start;
            for (index, right) in boundaries
                .iter()
                .copied()
                .chain(std::iter::once(end))
                .enumerate()
            {
                let mut part = super::slice(&source_project, &tempo, clip, left, right)
                    .expect("interior split boundaries produce non-empty slices");
                if index > 0 {
                    part.id = fresh();
                }
                result.push(part.id);
                replacement.push(Arc::new(part));
                left = right;
            }
        }
        Arc::make_mut(&mut project.tracks[track_index]).clips = replacement;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::tests::{clips as clip_summary, ids, q, song};
    use crate::model::ClipContent;

    #[test]
    fn splits_only_selected_unlocked_clips_and_keeps_exact_audio_source_phase() {
        let mut project = song();
        let original = project.tracks[0].clips[0].clone();
        let untouched = project.tracks[1].clone();
        let selected = [original.id];
        let parts = clips(&mut project, &selected, &[q(8), q(4), q(8)], &mut ids());
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], original.id);
        assert_eq!(
            clip_summary(&project, 0),
            [(10, 4, 4, 1_000), (1_001, 8, 4, 1_000)]
        );
        assert_eq!(project.tracks[0].clips[1].audio_phase_seconds, 2.0);
        assert!(
            Arc::ptr_eq(&untouched, &project.tracks[1]),
            "unaffected tracks keep sharing"
        );
    }

    #[test]
    fn cross_rate_split_keeps_exact_source_time_at_123_bpm_and_accumulates() {
        let mut project = song();
        project.tempo[0].bpm = 123.0;
        project.media[0].sample_rate = 44_100;
        project.media[0].frames = 441_000;
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
        clip.start = q(2);
        let source_id = clip.id;
        let tempo = super::super::tempo_of(&project);
        let first_cut = q(3);
        let second_cut = q(4);
        let expected_phase = tempo.seconds_at(second_cut) - tempo.seconds_at(q(2));

        // Splitting twice from the same original preserves the exact source
        // clock. A frame-rounded native offset would land one device frame
        // off at 44.1 kHz -> 48 kHz for this tempo.
        let parts = clips(
            &mut project,
            &[source_id],
            &[first_cut, second_cut],
            &mut ids(),
        );
        assert_eq!(parts.len(), 3);
        let first = &project.tracks[0].clips[0];
        let second = &project.tracks[0].clips[1];
        let third = &project.tracks[0].clips[2];
        assert_eq!(first.audio_phase_seconds, 0.0);
        assert_eq!(
            second.audio_phase_seconds,
            tempo.seconds_at(first_cut) - tempo.seconds_at(q(2))
        );
        assert_eq!(third.audio_phase_seconds, expected_phase);
        assert!(matches!(
            third.content,
            ClipContent::Audio { offset: 1_000, .. }
        ));

        // The same cut at a nonzero source origin leaves the native anchor
        // unchanged and adds only elapsed project seconds.
        assert_eq!(
            third.audio_phase_seconds,
            tempo.seconds_at(third.start) - tempo.seconds_at(first.start)
        );
        let bytes = crate::to_bytes(&project).unwrap();
        assert_eq!(crate::from_bytes(&bytes).unwrap(), project);

        let mut zero_origin = song();
        zero_origin.tempo[0].bpm = 123.0;
        zero_origin.media[0].sample_rate = 44_100;
        zero_origin.media[0].frames = 441_000;
        let zero_clip = Arc::make_mut(&mut Arc::make_mut(&mut zero_origin.tracks[0]).clips[0]);
        zero_clip.start = Ticks::ZERO;
        zero_clip.length = q(4);
        let source_id = zero_clip.id;
        let cut = q(1);
        let expected = super::super::tempo_of(&zero_origin).seconds_at(cut);
        clips(&mut zero_origin, &[source_id], &[cut], &mut ids());
        assert_eq!(zero_origin.tracks[0].clips[1].audio_phase_seconds, expected);
        assert_eq!(crate::validate(&zero_origin), Ok(()));
    }

    #[test]
    fn boundaries_and_locked_clips_do_not_split() {
        let mut project = song();
        let original = project.tracks[0].clips[0].clone();
        Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]).locked = true;
        let before = project.tracks[0].clone();
        let mut fresh = ids();
        let parts = clips(&mut project, &[original.id], &[q(4), q(12)], &mut fresh);
        assert_eq!(parts, [original.id]);
        assert!(Arc::ptr_eq(&before, &project.tracks[0]));
        assert_eq!(fresh(), Id(1_001), "no id was consumed");
    }

    #[test]
    fn loop_splits_advance_source_phase_without_shifting_loop_content() {
        use crate::envelope::Kind;
        use lumen_core::automation::{Lane, Point};
        let mut project = song();
        let track = Arc::make_mut(&mut project.tracks[0]);
        let clip = Arc::make_mut(&mut track.clips[0]);
        clip.loop_length = Some(q(4));
        clip.loop_phase = q(1);
        clip.envelopes.set(
            Kind::Gain,
            Lane::new(vec![Point::new(q(0), -6.0), Point::new(q(3), 0.0)]).unwrap(),
        );
        let source = clip.clone();
        let parts = clips(&mut project, &[source.id], &[q(8)], &mut ids());
        assert_eq!(parts, [source.id, Id(1_001)]);
        let first = &project.tracks[0].clips[0];
        let second = &project.tracks[0].clips[1];
        assert_eq!(first.loop_phase, q(1));
        assert_eq!(
            second.loop_phase,
            q(5),
            "elapsed source phase is accumulated"
        );
        assert_eq!(
            clip_summary(&project, 0),
            [(10, 4, 4, 1_000), (1_001, 8, 4, 1_000)]
        );
        assert_eq!(
            first.envelopes, second.envelopes,
            "loop envelopes remain source-local"
        );

        let mut midi = song();
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut midi.tracks[0]).clips[0]);
        clip.loop_length = Some(q(4));
        clip.content = crate::model::ClipContent::Midi {
            notes: vec![crate::model::Note::new(q(1), q(2), 60, 90)],
            events: vec![],
        };
        clips(&mut midi, &[Id(10)], &[q(6)], &mut ids());
        let ClipContent::Midi { notes, .. } = &midi.tracks[0].clips[1].content else {
            panic!("MIDI source content retained");
        };
        assert_eq!(notes[0].start(), q(1), "loop-local note time is unchanged");
        assert_eq!(midi.tracks[0].clips[1].loop_phase, q(2));
    }

    #[test]
    fn trimming_a_loop_start_stops_at_its_known_source_origin() {
        let project = song();
        let mut clip = (*project.tracks[0].clips[0]).clone();
        clip.loop_length = Some(q(4));
        clip.loop_phase = q(2);
        let trimmed = super::super::trim(
            &project,
            &super::super::tempo_of(&project),
            &clip,
            crate::edit::Edge::Start,
            q(1),
            false,
        );
        assert_eq!(
            trimmed.start,
            q(2),
            "cannot reveal before known loop origin"
        );
        assert_eq!(trimmed.loop_phase, Ticks::ZERO);
    }
}
