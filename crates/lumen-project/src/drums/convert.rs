//! Converting a clip's notes from one drum map to another (M8 D0): the one
//! destructive drum edit (§6.4). Each note that has a sound in the map it is
//! stored in becomes the input note of the same sound in the target map,
//! found by name first and then by the note the sound plays; a sound the
//! target lacks can be kept as it is, or left out.

use crate::model::{DrumMap, Note};

/// What a conversion did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Converted {
    /// Notes rewritten to the target's input note.
    pub moved: usize,
    /// Notes the target lacks, kept as they were because they were asked for.
    pub kept: usize,
    /// Notes the target lacks, left out.
    pub dropped: usize,
}

/// `notes` rewritten from `from` to `to`. A note with no sound in `from`
/// plays as written through either map, so it is kept as it is;
/// `keep_missing` keeps the notes whose sound `to` lacks, otherwise they are
/// dropped. The notes' times, lengths, velocities and flags do not change.
pub fn convert_notes(
    from: &DrumMap,
    to: &DrumMap,
    notes: &[Note],
    keep_missing: bool,
) -> (Vec<Note>, Converted) {
    let mut out = Vec::with_capacity(notes.len());
    let mut done = Converted::default();
    for note in notes {
        let Some(sound) = from.sound(note.pitch()) else {
            out.push(*note);
            continue;
        };
        let found = to
            .sounds
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(&sound.name))
            .or_else(|| to.sound_playing(sound.output));
        match found {
            Some(target) if target.input != note.pitch() => {
                let mut moved = *note;
                moved.2 = target.input;
                out.push(moved);
                done.moved += 1;
            }
            Some(_) => {
                out.push(*note);
            }
            None if keep_missing => {
                out.push(*note);
                done.kept += 1;
            }
            None => done.dropped += 1,
        }
    }
    (out, done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DrumIcon, Id, Sound};
    use lumen_core::time::Ticks;

    fn map(sounds: Vec<Sound>) -> DrumMap {
        DrumMap::new(Id(1), "Kit", sounds)
    }

    fn note(pitch: u8) -> Note {
        Note::new(Ticks(48), Ticks(96), pitch, 100)
    }

    fn kit_one() -> DrumMap {
        map(vec![
            Sound::new("Kick", 36, 40, DrumIcon::Kick),
            Sound::new("Snare", 38, 41, DrumIcon::Snare),
            Sound::new("Clave", 75, 76, DrumIcon::Percussion),
        ])
    }

    /// The target numbers its sounds elsewhere, as another kit would.
    fn kit_two() -> DrumMap {
        map(vec![
            // The snare comes first here: the name still finds it.
            Sound::new("Snare", 50, 51, DrumIcon::Snare),
            Sound::new("Kick", 60, 61, DrumIcon::Kick),
            // No sound called "Clave"; this one plays the same note.
            Sound::new("Stick", 80, 76, DrumIcon::Percussion),
        ])
    }

    #[test]
    fn a_note_becomes_the_same_sounds_input_note_by_name_then_by_output() {
        let (notes, done) = convert_notes(
            &kit_one(),
            &kit_two(),
            &[note(36), note(38), note(75)],
            false,
        );
        assert_eq!(
            notes.iter().map(|n| n.pitch()).collect::<Vec<_>>(),
            vec![60, 50, 80],
            "kick and snare by name, the clave by the note it plays"
        );
        assert_eq!((done.moved, done.kept, done.dropped), (3, 0, 0));
        // Nothing else about a note changes.
        assert_eq!(notes[0].start(), Ticks(48));
        assert_eq!(notes[0].length(), Ticks(96));
        assert_eq!(notes[0].velocity(), 100);
    }

    #[test]
    fn a_sound_the_target_lacks_is_kept_or_left_out() {
        let target = map(vec![Sound::new("Kick", 60, 61, DrumIcon::Kick)]);
        let (kept, done) = convert_notes(&kit_one(), &target, &[note(38)], true);
        assert_eq!(kept, vec![note(38)], "kept as it is");
        assert_eq!((done.moved, done.kept, done.dropped), (0, 1, 0));
        let (dropped, done) = convert_notes(&kit_one(), &target, &[note(36), note(38)], false);
        assert_eq!(dropped, vec![note(60)], "the kick moves, the snare goes");
        assert_eq!((done.moved, done.kept, done.dropped), (1, 0, 1));
    }

    #[test]
    fn a_note_with_no_sound_plays_as_written_through_either_map() {
        let (notes, done) = convert_notes(&kit_one(), &kit_two(), &[note(60), note(99)], false);
        assert_eq!(notes, vec![note(60), note(99)]);
        assert_eq!((done.moved, done.kept, done.dropped), (0, 0, 0));
    }

    #[test]
    fn a_sound_already_on_its_targets_note_is_not_counted_as_moved() {
        let (notes, done) = convert_notes(&kit_one(), &kit_one(), &[note(36)], false);
        assert_eq!(notes, vec![note(36)]);
        assert_eq!(done.moved, 0);
    }
}
