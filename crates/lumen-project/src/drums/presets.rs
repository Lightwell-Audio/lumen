//! The built-in drum maps, as code tables (M8 D0): General MIDI, GM2 and a
//! common 808-style layout. A preset is copied into the project when it is
//! attached, so a project never depends on a file elsewhere (the owner's
//! DM A).
//!
//! Every sound carries the icon its name reads as ([`super::icon_for_name`]),
//! so a kit's names and drawings cannot drift apart; a test pins each
//! preset's icons and its notes.

use crate::model::{Color, DrumIcon, Id, Sound};

use super::icon_for_name;

/// A built-in map the user can attach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// The stable id, for saved choices and tests.
    pub id: &'static str,
    pub name: &'static str,
    /// What each line is: the note, its name, and its choke group.
    table: &'static [(u8, &'static str, u8)],
}

impl Preset {
    /// Every preset, in the chooser's order.
    pub const ALL: [Preset; 3] = [
        Preset::GENERAL_MIDI,
        Preset::GENERAL_MIDI_2,
        Preset::EIGHT_OH_EIGHT,
    ];

    pub const GENERAL_MIDI: Preset = Preset {
        id: "gm",
        name: "General MIDI",
        table: GM,
    };

    pub const GENERAL_MIDI_2: Preset = Preset {
        id: "gm2",
        name: "General MIDI 2",
        table: GM2,
    };

    /// The 808-style kit: a short layout on General MIDI's notes, so a
    /// pattern written for one plays the right piece on the other. Its three
    /// hats choke each other.
    pub const EIGHT_OH_EIGHT: Preset = Preset {
        id: "808",
        name: "808 style",
        table: EIGHT_OH_EIGHT,
    };

    /// The id of the preset `id`, if there is one.
    pub fn from_id(id: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.id == id)
    }

    /// The preset called `name`.
    pub fn from_name(name: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.name == name)
    }

    /// The preset's sounds, in the map's order.
    pub fn sounds(&self) -> Vec<Sound> {
        self.table
            .iter()
            .map(|&(note, name, choke)| {
                let icon = icon_for_name(name);
                let mut sound = Sound::new(name, note, note, icon);
                if choke != 0 {
                    sound = sound.choking(choke);
                }
                sound
            })
            .collect()
    }

    /// The preset as a map of the project, under `id`.
    pub fn map(&self, id: Id) -> crate::model::DrumMap {
        crate::model::DrumMap::new(id, self.name, self.sounds())
    }
}

/// The colour of a sound's lane, by its piece: the mixed track palette, so a
/// kit reads in blocks (kick, snare, hats, toms, cymbals, percussion). The
/// rule lives on the icon (`DrumIcon::color`); this is the name the presets
/// and the editors use.
pub fn color_of(icon: DrumIcon) -> Color {
    icon.color()
}

/// General MIDI percussion (notes 35 to 81), on channel 10's notes. The
/// hats choke each other, as they do on a real kit.
const GM: &[(u8, &str, u8)] = &[
    (35, "Acoustic Bass Drum", 0),
    (36, "Bass Drum 1", 0),
    (37, "Side Stick", 0),
    (38, "Acoustic Snare", 0),
    (39, "Hand Clap", 0),
    (40, "Electric Snare", 0),
    (41, "Low Floor Tom", 0),
    (42, "Closed Hi-Hat", 1),
    (43, "High Floor Tom", 0),
    (44, "Pedal Hi-Hat", 1),
    (45, "Low Tom", 0),
    (46, "Open Hi-Hat", 1),
    (47, "Low-Mid Tom", 0),
    (48, "Hi-Mid Tom", 0),
    (49, "Crash Cymbal 1", 0),
    (50, "High Tom", 0),
    (51, "Ride Cymbal 1", 0),
    (52, "Chinese Cymbal", 0),
    (53, "Ride Bell", 0),
    (54, "Tambourine", 0),
    (55, "Splash Cymbal", 0),
    (56, "Cowbell", 0),
    (57, "Crash Cymbal 2", 0),
    (58, "Vibraslap", 0),
    (59, "Ride Cymbal 2", 0),
    (60, "Hi Bongo", 0),
    (61, "Low Bongo", 0),
    (62, "Mute Hi Conga", 0),
    (63, "Open Hi Conga", 0),
    (64, "Low Conga", 0),
    (65, "High Timbale", 0),
    (66, "Low Timbale", 0),
    (67, "High Agogo", 0),
    (68, "Low Agogo", 0),
    (69, "Cabasa", 0),
    (70, "Maracas", 0),
    (71, "Short Whistle", 0),
    (72, "Long Whistle", 0),
    (73, "Short Guiro", 0),
    (74, "Long Guiro", 0),
    (75, "Claves", 0),
    (76, "Hi Wood Block", 0),
    (77, "Low Wood Block", 0),
    (78, "Mute Cuica", 0),
    (79, "Open Cuica", 0),
    (80, "Mute Triangle", 0),
    (81, "Open Triangle", 0),
];

/// General MIDI 2's percussion: GM, plus the eight effects below note 35 and
/// the six that follow note 81.
const GM2: &[(u8, &str, u8)] = &[
    (27, "High Q", 0),
    (28, "Slap", 0),
    (29, "Scratch Push", 0),
    (30, "Scratch Pull", 0),
    (31, "Sticks", 0),
    (32, "Square Click", 0),
    (33, "Metronome Click", 0),
    (34, "Metronome Bell", 0),
    (35, "Acoustic Bass Drum", 0),
    (36, "Bass Drum 1", 0),
    (37, "Side Stick", 0),
    (38, "Acoustic Snare", 0),
    (39, "Hand Clap", 0),
    (40, "Electric Snare", 0),
    (41, "Low Floor Tom", 0),
    (42, "Closed Hi-Hat", 1),
    (43, "High Floor Tom", 0),
    (44, "Pedal Hi-Hat", 1),
    (45, "Low Tom", 0),
    (46, "Open Hi-Hat", 1),
    (47, "Low-Mid Tom", 0),
    (48, "Hi-Mid Tom", 0),
    (49, "Crash Cymbal 1", 0),
    (50, "High Tom", 0),
    (51, "Ride Cymbal 1", 0),
    (52, "Chinese Cymbal", 0),
    (53, "Ride Bell", 0),
    (54, "Tambourine", 0),
    (55, "Splash Cymbal", 0),
    (56, "Cowbell", 0),
    (57, "Crash Cymbal 2", 0),
    (58, "Vibraslap", 0),
    (59, "Ride Cymbal 2", 0),
    (60, "Hi Bongo", 0),
    (61, "Low Bongo", 0),
    (62, "Mute Hi Conga", 0),
    (63, "Open Hi Conga", 0),
    (64, "Low Conga", 0),
    (65, "High Timbale", 0),
    (66, "Low Timbale", 0),
    (67, "High Agogo", 0),
    (68, "Low Agogo", 0),
    (69, "Cabasa", 0),
    (70, "Maracas", 0),
    (71, "Short Whistle", 0),
    (72, "Long Whistle", 0),
    (73, "Short Guiro", 0),
    (74, "Long Guiro", 0),
    (75, "Claves", 0),
    (76, "Hi Wood Block", 0),
    (77, "Low Wood Block", 0),
    (78, "Mute Cuica", 0),
    (79, "Open Cuica", 0),
    (80, "Mute Triangle", 0),
    (81, "Open Triangle", 0),
    (82, "Shaker", 0),
    (83, "Jingle Bell", 0),
    (84, "Belltree", 0),
    (85, "Castanets", 0),
    (86, "Mute Surdo", 0),
    (87, "Open Surdo", 0),
];

/// The 808-style kit: one of each piece a one-shot kit needs, on the General
/// MIDI notes a pattern for that kit uses.
const EIGHT_OH_EIGHT: &[(u8, &str, u8)] = &[
    (36, "Kick", 0),
    (37, "Rim", 0),
    (38, "Snare", 0),
    (39, "Clap", 0),
    (41, "Floor Tom", 0),
    (42, "Closed Hat", 1),
    (43, "Low Tom", 0),
    (44, "Pedal Hat", 1),
    (45, "Mid Tom", 0),
    (46, "Open Hat", 1),
    (47, "High Tom", 0),
    (49, "Crash", 0),
    (51, "Ride", 0),
    (56, "Cowbell", 0),
    (70, "Shaker", 0),
    (75, "Claves", 0),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_is_a_sound_kit_with_unique_notes_in_order() {
        for preset in Preset::ALL {
            let sounds = preset.sounds();
            assert!(!sounds.is_empty(), "{}", preset.id);
            assert_eq!(sounds.len(), preset.table.len());
            let notes: Vec<u8> = sounds.iter().map(|s| s.input).collect();
            let mut sorted = notes.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(
                sorted.len(),
                notes.len(),
                "{}: two sounds share a note",
                preset.id
            );
            assert_eq!(notes, sorted, "{}: sounds are in note order", preset.id);
            assert!(notes.iter().all(|n| *n < 128));
            for sound in &sounds {
                assert_eq!(
                    sound.icon,
                    icon_for_name(&sound.name),
                    "{}: {} carries its name's icon",
                    preset.id,
                    sound.name
                );
                assert_eq!(sound.output, sound.input, "a preset plays as written");
                assert_eq!(sound.route, crate::model::Route::Main);
                assert_eq!(sound.channel, None);
                assert!(!sound.mute && !sound.solo);
                assert!(sound.color != Color(0));
            }
        }
    }

    #[test]
    fn every_preset_has_its_pieces_and_its_hats_choke() {
        let pieces = |preset: Preset| -> Vec<DrumIcon> {
            preset
                .sounds()
                .iter()
                .map(|s| s.icon)
                .filter(|i| {
                    matches!(
                        i,
                        DrumIcon::Kick
                            | DrumIcon::Snare
                            | DrumIcon::ClosedHat
                            | DrumIcon::PedalHat
                            | DrumIcon::OpenHat
                    )
                })
                .collect()
        };
        for preset in Preset::ALL {
            let found = pieces(preset);
            for piece in [
                DrumIcon::Kick,
                DrumIcon::Snare,
                DrumIcon::ClosedHat,
                DrumIcon::OpenHat,
            ] {
                assert!(found.contains(&piece), "{} has a {piece:?}", preset.id);
            }
        }
        // General MIDI's and GM2's three hats are one group; so are the 808's.
        for preset in Preset::ALL {
            let groups: Vec<u8> = preset
                .sounds()
                .iter()
                .filter(|s| {
                    matches!(
                        s.icon,
                        DrumIcon::ClosedHat | DrumIcon::PedalHat | DrumIcon::OpenHat
                    )
                })
                .filter_map(|s| s.choke)
                .collect();
            assert!(
                groups.len() >= 2,
                "{}: the hats choke each other",
                preset.id
            );
            assert!(groups.windows(2).all(|w| w[0] == w[1]), "{}", preset.id);
        }
    }

    #[test]
    fn a_preset_makes_a_map_of_its_own() {
        let map = Preset::GENERAL_MIDI.map(Id(7));
        assert_eq!((map.id, map.name.as_str()), (Id(7), "General MIDI"));
        assert_eq!(map.sounds.len(), GM.len());
        assert_eq!(
            map.sound(42).map(|s| s.name.as_str()),
            Some("Closed Hi-Hat")
        );
        assert_eq!(Preset::from_id("gm2"), Some(Preset::GENERAL_MIDI_2));
        assert_eq!(Preset::from_id("nope"), None);
        assert_eq!(Preset::from_name("808 style"), Some(Preset::EIGHT_OH_EIGHT));
        assert_eq!(Preset::EIGHT_OH_EIGHT.id, "808");
    }
}
