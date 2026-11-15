#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KitKey {
    pub key: u8,
    pub channel: Option<u8>,
    pub silent: bool,
}

impl KitKey {
    pub fn as_written(key: u8) -> KitKey {
        KitKey {
            key,
            channel: None,
            silent: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrumKit {
    keys: [KitKey; 128],
    groups: [u8; 128],
}

impl Default for DrumKit {
    fn default() -> Self {
        Self::new()
    }
}

impl DrumKit {
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

    pub fn set(&mut self, input: u8, key: KitKey) {
        self.keys[usize::from(input)] = key;
    }

    pub fn set_choke(&mut self, output: u8, group: u8) {
        self.groups[usize::from(output)] = group;
    }

    pub fn key(&self, input: u8) -> KitKey {
        self.keys[usize::from(input)]
    }

    pub fn choke_group(&self, output: u8) -> u8 {
        self.groups[usize::from(output)]
    }

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
        let mut other = kit.clone();
        other.set_choke(41, 1);
        assert_ne!(kit, other);
    }
}
