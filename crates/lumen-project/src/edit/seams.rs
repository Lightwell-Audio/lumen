use lumen_core::time::Ticks;

use crate::model::{Clip, ClipContent, Note, Track};

fn notes(clip: &Clip) -> &[Note] {
    match &clip.content {
        ClipContent::Midi { notes, .. } => notes,
        ClipContent::Audio { .. } => &[],
    }
}

pub fn is_seam(prev: &Clip, prev_length: Ticks, next: &Clip) -> bool {
    let plain = |c: &Clip| {
        !c.muted
            && matches!(c.content, ClipContent::Midi { .. })
            && c.loop_length.is_none_or(|l| l.0 <= 0)
    };
    plain(prev) && plain(next) && prev.start + prev_length == next.start
}

pub fn carried_on(prev: &Clip, prev_length: Ticks, carried: &[usize], next: &Clip) -> Vec<usize> {
    let reaching: Vec<(Ticks, u8, u8, Ticks)> = notes(prev)
        .iter()
        .enumerate()
        .filter(|&(i, n)| {
            let shown = n.start() >= Ticks::ZERO || carried.contains(&i);
            !n.muted() && shown && n.start() < prev_length && n.end() >= prev_length
        })
        .map(|(_, n)| {
            (
                prev.start + n.start(),
                n.pitch(),
                n.velocity(),
                prev.start + n.end(),
            )
        })
        .collect();
    if reaching.is_empty() {
        return Vec::new();
    }
    notes(next)
        .iter()
        .enumerate()
        .filter(|&(_, n)| {
            let end = next.start + n.end().min(next.length);
            !n.muted()
                && n.carries_on()
                && n.start() < Ticks::ZERO
                && n.end() > Ticks::ZERO
                && reaching.iter().any(|&(start, pitch, velocity, until)| {
                    (start, pitch, velocity) == (next.start + n.start(), n.pitch(), n.velocity())
                        && end <= until
                })
        })
        .map(|(i, _)| i)
        .collect()
}

pub fn carried(track: &Track) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); track.clips.len()];
    for i in 1..track.clips.len() {
        let (prev, next) = (&track.clips[i - 1], &track.clips[i]);
        if is_seam(prev, prev.length, next) {
            out[i] = carried_on(prev, prev.length, &out[i - 1], next);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::model::{Color, Id, TrackKind};

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn clip(id: u64, start: i64, length: i64, notes: Vec<Note>) -> Arc<Clip> {
        Arc::new(Clip {
            id: Id(id),
            name: String::new(),
            start: q(start),
            length: q(length),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            crossfade: false,
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            envelopes: Default::default(),
            takes: None,
            warp: None,
            content: ClipContent::Midi {
                notes,
                events: Vec::new(),
            },
        })
    }

    fn track(clips: Vec<Arc<Clip>>) -> Track {
        Track {
            clips,
            ..Track::new(Id(1), "t", TrackKind::Midi, Color(0))
        }
    }

    fn of(lists: &[&[usize]]) -> Vec<Vec<usize>> {
        lists.iter().map(|l| l.to_vec()).collect()
    }

    fn note(start: i64, length: i64, pitch: u8) -> Note {
        Note::new(q(start), q(length), pitch, 100)
    }

    #[test]
    fn a_split_note_carries_on_into_the_right_piece_and_beyond() {
        let t = track(vec![
            clip(1, 0, 4, vec![note(2, 9, 60), note(0, 1, 62)]),
            clip(2, 4, 4, vec![note(-2, 9, 60), note(-4, 1, 62)]),
            clip(3, 8, 4, vec![note(-6, 9, 60), note(-8, 1, 62)]),
        ]);
        assert_eq!(carried(&t), of(&[&[], &[0], &[0]]));
    }

    #[test]
    fn a_note_carries_on_only_into_a_clip_that_holds_it_where_they_meet() {
        let other = track(vec![
            clip(1, 0, 4, vec![note(2, 4, 60)]),
            clip(2, 4, 4, vec![note(-1, 4, 60), note(-2, 4, 61)]),
        ]);
        assert_eq!(carried(&other), of(&[&[], &[]]));
        let apart = track(vec![
            clip(1, 0, 4, vec![note(2, 4, 60)]),
            clip(2, 5, 4, vec![note(-3, 4, 60)]),
        ]);
        assert_eq!(carried(&apart), of(&[&[], &[]]));
        let short = track(vec![
            clip(1, 0, 4, vec![note(2, 1, 60)]),
            clip(2, 4, 4, vec![note(-2, 4, 60)]),
        ]);
        assert_eq!(carried(&short), of(&[&[], &[]]));
    }

    #[test]
    fn muted_clips_and_notes_and_loops_carry_nothing() {
        let base = || {
            vec![
                clip(1, 0, 4, vec![note(2, 4, 60)]),
                clip(2, 4, 4, vec![note(-2, 4, 60)]),
            ]
        };
        let mut muted = base();
        Arc::make_mut(&mut muted[0]).muted = true;
        assert_eq!(carried(&track(muted)), of(&[&[], &[]]));
        let mut looping = base();
        Arc::make_mut(&mut looping[1]).loop_length = Some(q(2));
        assert_eq!(carried(&track(looping)), of(&[&[], &[]]));
        let mut quiet = base();
        if let ClipContent::Midi { notes, .. } = &mut Arc::make_mut(&mut quiet[1]).content {
            notes[0].4 = true;
        }
        assert_eq!(carried(&track(quiet)), of(&[&[], &[]]));
        assert_eq!(carried(&track(base())), of(&[&[], &[0]]));
    }

    #[test]
    fn a_hidden_note_that_outlasts_or_outplays_the_one_reaching_the_seam_is_not_carried() {
        let longer = track(vec![
            clip(1, 0, 2, vec![note(1, 2, 60)]),
            clip(2, 2, 2, vec![note(-1, 4, 60)]),
        ]);
        assert_eq!(carried(&longer), of(&[&[], &[]]));
        let softer = track(vec![
            clip(1, 0, 2, vec![note(1, 2, 60)]),
            clip(2, 2, 2, vec![Note::new(q(-1), q(2), 60, 30)]),
        ]);
        assert_eq!(carried(&softer), of(&[&[], &[]]));
        let heard = track(vec![
            clip(1, 0, 2, vec![note(1, 4, 60)]),
            clip(2, 2, 2, vec![note(-1, 3, 60)]),
        ]);
        assert_eq!(carried(&heard), of(&[&[], &[0]]));
        let past = track(vec![
            clip(1, 0, 2, vec![note(1, 3, 60)]),
            clip(2, 2, 2, vec![note(-1, 9, 60)]),
        ]);
        assert_eq!(carried(&past), of(&[&[], &[0]]));
    }

    #[test]
    fn a_clip_cut_short_where_it_plays_meets_no_seam() {
        let t = track(vec![
            clip(1, 0, 4, vec![note(2, 4, 60)]),
            clip(2, 4, 4, vec![note(-2, 4, 60)]),
        ]);
        assert!(is_seam(&t.clips[0], q(4), &t.clips[1]));
        assert!(!is_seam(&t.clips[0], q(3), &t.clips[1]));
        assert_eq!(carried_on(&t.clips[0], q(4), &[], &t.clips[1]), vec![0]);
    }
}
