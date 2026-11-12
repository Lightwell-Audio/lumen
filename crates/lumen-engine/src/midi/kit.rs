//! A drum map as the engine plays it (M8 slice D0, ADR-XXXX): the note each
//! input note sounds, its channel, and the choke groups.
//!
//! The app builds this on its side, from the track's map, and hands it to the
//! engine with the track's MIDI (the owner's E3: translation happens where the
//! tracks are built, except live input's, which the engine reads from here).
//! Nothing here allocates after [`DrumKit::new`], and nothing locks: the audio
//! thread reads two small arrays.
//!
//! A choke group ends the other sounds of the group when one is struck (the
//! owner's CO A): the engine sends their note-offs at the same sample as the
//! new note-on, so nothing rings under the new hit.

/// What one input note plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KitKey {
    /// The note the instrument gets.
    pub key: u8,
    /// The sound's channel, or the track's when none.
    pub channel: Option<u8>,
    /// A muted sound, or one no solo lets through: it sounds nothing.
    pub silent: bool,
}

impl KitKey {
    /// A note that plays as it came, on the track's channel.
    pub fn as_written(key: u8) -> KitKey {
        KitKey {
            key,
            channel: None,
            silent: false,
        }
    }
}

/// A drum map's notes and choke groups: what a track plays live, and what
/// chokes what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrumKit {
    /// By input note: what each one plays.
    keys: [KitKey; 128],
    /// By the note a sound plays: its choke group, 0 for none.
    groups: [u8; 128],
}

impl Default for DrumKit {
    fn default() -> Self {
        Self::new()
    }
}

impl DrumKit {
    /// A kit that plays every note as written. Makes one array; call off the
    /// audio thread.
    pub fn new() -> Self {
        let mut keys = [KitKey::as_written(0); 128];
        for (input, key) in keys.iter_mut().enumerate() {
            *key = KitKey::as_written(input as u8);
        }
        Self {
            keys,
            groups: [0; 128],
        }
    }

    /// What input note `input` plays.
    pub fn set(&mut self, input: u8, key: KitKey) {
        self.keys[usize::from(input)] = key;
    }

    /// Puts the sounds playing `output` in choke group `group` (1 to 255;
    /// 0 takes them out).
    pub fn set_choke(&mut self, output: u8, group: u8) {
        self.groups[usize::from(output)] = group;
    }

    /// What `input` plays.
    pub fn key(&self, input: u8) -> KitKey {
        self.keys[usize::from(input)]
    }

    /// The choke group of the sound playing `output`: 0 for none.
    pub fn choke_group(&self, output: u8) -> u8 {
        self.groups[usize::from(output)]
    }

    /// Whether any group is set (a track with none may skip the check).
    pub fn chokes(&self) -> bool {
        self.groups.iter().any(|g| *g != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_kit_plays_every_note_as_written() {
        let kit = DrumKit::new();
        assert_eq!(kit.key(60), KitKey::as_written(60));
        assert_eq!(kit.key(36), KitKey::as_written(36));
        assert_eq!(kit.choke_group(36), 0);
        assert!(!kit.chokes());
    }

    #[test]
    fn a_kit_reads_back_its_notes_and_its_groups() {
        let mut kit = DrumKit::new();
        kit.set(
            36,
            KitKey {
                key: 40,
                channel: Some(9),
                silent: false,
            },
        );
        kit.set(
            38,
            KitKey {
                key: 38,
                channel: None,
                silent: true,
            },
        );
        kit.set_choke(40, 1);
        assert_eq!(kit.key(36).key, 40);
        assert_eq!(kit.key(36).channel, Some(9));
        assert!(kit.key(38).silent);
        assert_eq!(kit.choke_group(40), 1);
        assert_eq!(kit.choke_group(41), 0);
        assert!(kit.chokes());
        // It compares by its contents, so a changed map is another kit.
        let mut other = kit.clone();
        other.set_choke(41, 1);
        assert_ne!(kit, other);
    }
}
