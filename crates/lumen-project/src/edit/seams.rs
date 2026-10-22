//! Notes that carry on across a seam (ADR-0058).
//!
//! A clip is a window onto its notes (ADR-0056), so a note running past a
//! clip's end is cut there. When the next clip on the track begins exactly
//! where that one ends (a **seam**: an Overdub begun inside a clip, or a
//! clip cut in two) and holds the same note, the note **carries on**: it sounds, and
//! is drawn, through the seam until it ends or the next clip does, and on
//! through further seams that hold it too. "The same note" is a note of the
//! same pitch and velocity starting at the same arrangement time, which the
//! next clip keeps hidden before its start, and which sounds no later than
//! the note it continues: the copy a split leaves in the right piece, or the
//! one an Overdub carries into its new clip (`record::carry`). A clip that
//! does not hold it (a clip moved up against another, a note cut short
//! where a new one was played) cuts it at the seam, as before. A recorded
//! note kept hidden before a take's window (`record::capture_parts`) is not
//! a copy: it may share an old note's start and pitch, but runs longer or
//! was struck at another velocity, so it stays hidden and the old note
//! stops at the seam.
//!
//! Only clips that do not loop meet at seams: a loop's repeats begin again
//! at every repeat, so nothing crosses them. Muted clips and muted notes
//! carry nothing.

use lumen_core::time::Ticks;

use crate::model::{Clip, ClipContent, Note, Track};

fn notes(clip: &Clip) -> &[Note] {
    match &clip.content {
        ClipContent::Midi { notes, .. } => notes,
        ClipContent::Audio { .. } => &[],
    }
}

/// Whether notes can carry on from `prev`, which plays `prev_length` of
/// itself, into `next`: both are unmuted MIDI clips that do not loop, and
/// `next` begins where `prev` stops.
pub fn is_seam(prev: &Clip, prev_length: Ticks, next: &Clip) -> bool {
    let plain = |c: &Clip| {
        !c.muted
            && matches!(c.content, ClipContent::Midi { .. })
            && c.loop_length.is_none_or(|l| l.0 <= 0)
    };
    plain(prev) && plain(next) && prev.start + prev_length == next.start
}

/// The notes of `next` (by index) that carry on notes sounding up to the
/// seam with `prev`. `prev` plays `prev_length` of itself, and `carried`
/// are the notes it carries on from the clip before it. Call only where
/// [`is_seam`] holds.
pub fn carried_on(prev: &Clip, prev_length: Ticks, carried: &[usize], next: &Clip) -> Vec<usize> {
    // Every note sounding up to the seam: its arrangement start, pitch,
    // velocity and arrangement end.
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
            // A copy of the note never sounds past it: a split leaves the
            // same note, an Overdub carries it as long as it was heard. A
            // hidden recorded note that only shares its start and pitch
            // (one played before a punch-in) is longer, or struck at
            // another velocity, so it stays hidden. Explicit provenance
            // also excludes an identical or shorter hidden capture.
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

/// For each clip of `track` (by index, in the track's order), the notes it
/// carries on from the clip before it: what plays and is drawn from its
/// start, beside the notes in its window.
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
            fade_out: Default::default(),
            loop_length: None,
            envelopes: Default::default(),
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
        // A note from 2 to 11 split at 4 and 8: each piece keeps it.
        let t = track(vec![
            clip(1, 0, 4, vec![note(2, 9, 60), note(0, 1, 62)]),
            clip(2, 4, 4, vec![note(-2, 9, 60), note(-4, 1, 62)]),
            clip(3, 8, 4, vec![note(-6, 9, 60), note(-8, 1, 62)]),
        ]);
        assert_eq!(carried(&t), of(&[&[], &[0], &[0]]));
    }

    #[test]
    fn a_note_carries_on_only_into_a_clip_that_holds_it_where_they_meet() {
        // The next clip holds the note at another time, or another pitch.
        let other = track(vec![
            clip(1, 0, 4, vec![note(2, 4, 60)]),
            clip(2, 4, 4, vec![note(-1, 4, 60), note(-2, 4, 61)]),
        ]);
        assert_eq!(carried(&other), of(&[&[], &[]]));
        // A gap between them.
        let apart = track(vec![
            clip(1, 0, 4, vec![note(2, 4, 60)]),
            clip(2, 5, 4, vec![note(-3, 4, 60)]),
        ]);
        assert_eq!(carried(&apart), of(&[&[], &[]]));
        // The note ends before the seam.
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
        // The old note sounds 1 to 3; the next clip begins at 2 and keeps a
        // recorded note hidden from 1 with the same pitch (a take begun
        // before a punch-in). It runs to 5, past the old note: not a copy.
        let longer = track(vec![
            clip(1, 0, 2, vec![note(1, 2, 60)]),
            clip(2, 2, 2, vec![note(-1, 4, 60)]),
        ]);
        assert_eq!(carried(&longer), of(&[&[], &[]]));
        // Struck at another velocity, it is another note too.
        let softer = track(vec![
            clip(1, 0, 2, vec![note(1, 2, 60)]),
            clip(2, 2, 2, vec![Note::new(q(-1), q(2), 60, 30)]),
        ]);
        assert_eq!(carried(&softer), of(&[&[], &[]]));
        // A copy that ends sooner, as an Overdub carries what was heard,
        // or that runs on only past the clip's end, still carries.
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
