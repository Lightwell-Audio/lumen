//! Drum maps as the song plays them (M8 slice D0, ADR-XXXX).
//!
//! A clip stores the map's **input** notes; what is heard is worked out here:
//! [`played`] gives the note, channel and length a sound answers with, or
//! nothing for a muted sound (or one no solo lets through). [`presets`] holds
//! the built-in kits as code tables, [`icon`] guesses a sound's drawing from
//! its name, and [`convert`] rewrites a clip's notes from one map to another
//! when the user asks for it (the one destructive drum edit).

use lumen_core::time::Ticks;

use crate::model::{DrumMap, Note, Sound};

pub mod convert;
pub mod icon;
pub mod presets;

pub use convert::{Converted, convert_notes};
pub use icon::icon_for_name;

/// How a note sounds through a map: the note the instrument gets, the channel
/// (none for the track's) and a length override, when the sound has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Played {
    pub key: u8,
    pub channel: Option<u8>,
    pub length: Option<Ticks>,
}

impl Played {
    /// The length `note_length` plays with this sound's override.
    pub fn length_of(self, note_length: Ticks) -> Ticks {
        self.length.unwrap_or(note_length)
    }
}

/// How `input` sounds through `map`: nothing for a muted sound, or any
/// unsoloed sound while one is soloed; a note with no sound plays as written.
pub fn played(map: &DrumMap, input: u8) -> Option<Played> {
    match map.sound(input) {
        Some(sound) if !map.heard(sound) => None,
        Some(sound) => Some(Played {
            key: sound.output,
            channel: sound.channel,
            length: sound.length,
        }),
        None => Some(Played {
            key: input,
            channel: None,
            length: None,
        }),
    }
}

/// How `note` sounds through a map.
pub fn played_note(map: &DrumMap, note: Note) -> Option<Played> {
    played(map, note.pitch())
}

/// The key a recording keeps for a note that arrived as `key` (the owner's
/// E4): the map's input note when `key` is one; otherwise the input note of
/// the first sound that plays it, for a player whose pads send the notes the
/// kit plays; otherwise `key` itself.
pub fn store_key(map: &DrumMap, key: u8) -> u8 {
    if map.sound(key).is_some() {
        return key;
    }
    map.sound_playing(key).map_or(key, |sound| sound.input)
}

/// [`store_key`] for every note: what a recorder keeps of an incoming key.
pub fn store_table(map: &DrumMap) -> [u8; 128] {
    let mut table = [0u8; 128];
    for (key, kept) in table.iter_mut().enumerate() {
        *kept = store_key(map, key as u8);
    }
    table
}

/// A sound of `name` on `input`, playing `output`, carrying the icon
/// [`icon_for_name`] reads from the name.
pub fn sound(name: impl Into<String>, input: u8, output: u8) -> Sound {
    let name = name.into();
    let icon = icon_for_name(&name);
    Sound::new(name, input, output, icon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Color, DrumIcon, Id};

    fn map(sounds: Vec<Sound>) -> DrumMap {
        DrumMap::new(Id(1), "Kit", sounds)
    }

    #[test]
    fn a_note_with_no_sound_plays_as_written() {
        let kit = map(vec![Sound::new("Kick", 36, 40, DrumIcon::Kick)]);
        assert_eq!(
            played(&kit, 60),
            Some(Played {
                key: 60,
                channel: None,
                length: None
            })
        );
        assert_eq!(
            played(&kit, 36),
            Some(Played {
                key: 40,
                channel: None,
                length: None
            })
        );
    }

    #[test]
    fn a_muted_sound_is_silent_and_a_solo_silences_the_rest() {
        let mut kit = map(vec![
            Sound::new("Kick", 36, 36, DrumIcon::Kick),
            Sound::new("Snare", 38, 38, DrumIcon::Snare),
        ]);
        kit.sounds[1].solo = true;
        assert!(played(&kit, 36).is_none(), "unsoloed while one is soloed");
        assert!(played(&kit, 38).is_some());
        kit.sounds[1].solo = false;
        kit.sounds[0].mute = true;
        assert!(played(&kit, 36).is_none(), "muted");
        assert!(played(&kit, 38).is_some());
        // A muted sound that is also soloed stays silent.
        kit.sounds[0].solo = true;
        assert!(played(&kit, 36).is_none());
    }

    #[test]
    fn a_sound_plays_on_its_channel_and_for_its_length() {
        let mut kit = map(vec![Sound::new("Kick", 36, 36, DrumIcon::Kick)]);
        kit.sounds[0].channel = Some(9);
        kit.sounds[0].length = Some(Ticks(240));
        let heard = played(&kit, 36).expect("heard");
        assert_eq!(heard.channel, Some(9));
        assert_eq!(heard.length_of(Ticks(960)), Ticks(240));
        // No override keeps the note's length.
        kit.sounds[0].length = None;
        let heard = played(&kit, 36).expect("heard");
        assert_eq!(heard.length_of(Ticks(960)), Ticks(960));
    }

    #[test]
    fn a_recording_keeps_the_input_note_of_what_it_hears() {
        let kit = map(vec![
            Sound::new("Kick", 36, 40, DrumIcon::Kick),
            Sound::new("Snare", 38, 41, DrumIcon::Snare),
        ]);
        assert_eq!(store_key(&kit, 36), 36, "an input note stays");
        assert_eq!(store_key(&kit, 40), 36, "an output note maps back");
        assert_eq!(store_key(&kit, 60), 60, "anything else stays");
        let table = store_table(&kit);
        assert_eq!((table[40], table[60]), (36, 60));
        // Two sounds playing one output note: the first wins.
        let both = map(vec![
            Sound::new("Snare", 38, 41, DrumIcon::Snare),
            Sound::new("Clap", 39, 41, DrumIcon::Clap),
        ]);
        assert_eq!(store_key(&both, 41), 38);
        // A key that is an input note is kept, even when another sound plays
        // it.
        let crossed = map(vec![
            Sound::new("Kick", 36, 40, DrumIcon::Kick),
            Sound::new("Snare", 40, 41, DrumIcon::Snare),
        ]);
        assert_eq!(store_key(&crossed, 40), 40);
    }

    #[test]
    fn the_choke_group_of_a_played_note_is_its_sounds() {
        let kit = map(vec![
            Sound::new("Closed Hat", 42, 42, DrumIcon::ClosedHat).choking(1),
            Sound::new("Open Hat", 46, 46, DrumIcon::OpenHat).choking(1),
            Sound::new("Kick", 36, 36, DrumIcon::Kick),
        ]);
        assert_eq!(kit.choke_of_output(42), 1);
        assert_eq!(kit.choke_of_output(46), 1);
        assert_eq!(kit.choke_of_output(36), 0, "in no group");
        assert_eq!(kit.choke_of_output(99), 0);
    }

    #[test]
    fn a_sound_made_from_a_name_carries_its_icon() {
        let sound = sound("Open Hat", 46, 46);
        assert_eq!(sound.icon, DrumIcon::OpenHat);
        assert_eq!(sound.color, presets::color_of(DrumIcon::OpenHat));
        assert_ne!(sound.color, Color(0));
    }
}
