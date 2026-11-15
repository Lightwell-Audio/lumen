use crate::model::{DrumMap, Note};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Converted {
    pub moved: usize,
    pub kept: usize,
    pub dropped: usize,
}

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

    fn kit_two() -> DrumMap {
        map(vec![
            Sound::new("Snare", 50, 51, DrumIcon::Snare),
            Sound::new("Kick", 60, 61, DrumIcon::Kick),
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
