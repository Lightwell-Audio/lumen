//! A MIDI channel message of up to three bytes, shared by the engine, the
//! devices and the app (M5 slice 1). Plain data: no allocation, so it can
//! travel on the audio thread.

/// Controller numbers Lumen's built-in devices answer to.
pub mod cc {
    /// Modulation wheel.
    pub const MODULATION: u8 = 1;
    /// Bank select, most and least significant byte.
    pub const BANK_MSB: u8 = 0;
    pub const BANK_LSB: u8 = 32;
    /// Sustain pedal: 64 and up is down.
    pub const SUSTAIN: u8 = 64;
    /// Silence at once (no release tail).
    pub const ALL_SOUND_OFF: u8 = 120;
    pub const ALL_NOTES_OFF: u8 = 123;
}

/// A channel message: status byte (kind and channel), then up to two data
/// bytes. Program change and channel pressure use one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Msg(pub [u8; 3]);

/// What a [`Msg`] says, decoded. Note-on with velocity 0 reads as a
/// note-off, as the MIDI standard has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    NoteOff {
        key: u8,
        velocity: u8,
    },
    NoteOn {
        key: u8,
        velocity: u8,
    },
    /// Polyphonic key pressure.
    KeyPressure {
        key: u8,
        value: u8,
    },
    Controller {
        number: u8,
        value: u8,
    },
    Program {
        program: u8,
    },
    /// Channel pressure (aftertouch).
    Pressure {
        value: u8,
    },
    /// −8192 to 8191, 0 is centre.
    PitchBend {
        value: i16,
    },
    /// Anything else (system messages).
    Other,
}

impl Msg {
    pub fn note_on(channel: u8, key: u8, velocity: u8) -> Self {
        Self([0x90 | (channel & 15), key & 127, velocity.clamp(1, 127)])
    }

    pub fn note_off(channel: u8, key: u8) -> Self {
        Self([0x80 | (channel & 15), key & 127, 64])
    }

    pub fn controller(channel: u8, number: u8, value: u8) -> Self {
        Self([0xB0 | (channel & 15), number & 127, value & 127])
    }

    pub fn program(channel: u8, program: u8) -> Self {
        Self([0xC0 | (channel & 15), program & 127, 0])
    }

    pub fn pressure(channel: u8, value: u8) -> Self {
        Self([0xD0 | (channel & 15), value & 127, 0])
    }

    /// `value` is −8192 to 8191, 0 centre.
    pub fn pitch_bend(channel: u8, value: i16) -> Self {
        let raw = (i32::from(value.clamp(-8192, 8191)) + 8192) as u16;
        Self([0xE0 | (channel & 15), (raw & 127) as u8, (raw >> 7) as u8])
    }

    /// The channel, 0 to 15.
    pub fn channel(self) -> u8 {
        self.0[0] & 15
    }

    /// How many of the three bytes mean something.
    pub fn len(self) -> usize {
        match self.0[0] >> 4 {
            0xC | 0xD => 2,
            0x8..=0xB | 0xE => 3,
            _ => 1,
        }
    }

    pub fn is_empty(self) -> bool {
        false
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0[..self.len()]
    }

    pub fn kind(self) -> Kind {
        let [status, a, b] = self.0;
        match status >> 4 {
            0x8 => Kind::NoteOff {
                key: a,
                velocity: b,
            },
            0x9 if b == 0 => Kind::NoteOff {
                key: a,
                velocity: 0,
            },
            0x9 => Kind::NoteOn {
                key: a,
                velocity: b,
            },
            0xA => Kind::KeyPressure { key: a, value: b },
            0xB => Kind::Controller {
                number: a,
                value: b,
            },
            0xC => Kind::Program { program: a },
            0xD => Kind::Pressure { value: a },
            0xE => Kind::PitchBend {
                value: ((i32::from(b) << 7 | i32::from(a)) - 8192) as i16,
            },
            _ => Kind::Other,
        }
    }
}

/// A message at a frame within a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// Frames from the start of the block.
    pub offset: u32,
    pub msg: Msg,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_through_their_bytes() {
        assert_eq!(
            Msg::note_on(2, 60, 100).kind(),
            Kind::NoteOn {
                key: 60,
                velocity: 100
            }
        );
        assert_eq!(
            Msg::note_off(0, 60).kind(),
            Kind::NoteOff {
                key: 60,
                velocity: 64
            }
        );
        assert_eq!(
            Msg::controller(0, cc::SUSTAIN, 127).kind(),
            Kind::Controller {
                number: 64,
                value: 127
            }
        );
        for value in [-8192, -1, 0, 1, 8191] {
            assert_eq!(Msg::pitch_bend(3, value).kind(), Kind::PitchBend { value });
        }
        assert_eq!(Msg::program(1, 4).kind(), Kind::Program { program: 4 });
        assert_eq!(Msg::pressure(1, 9).kind(), Kind::Pressure { value: 9 });
    }

    #[test]
    fn a_zero_velocity_note_on_is_a_note_off() {
        let msg = Msg([0x90, 61, 0]);
        assert_eq!(
            msg.kind(),
            Kind::NoteOff {
                key: 61,
                velocity: 0
            }
        );
    }

    #[test]
    fn lengths_follow_the_status() {
        assert_eq!(Msg::note_on(0, 1, 1).len(), 3);
        assert_eq!(Msg::program(0, 1).len(), 2);
        assert_eq!(Msg::pressure(0, 1).bytes().len(), 2);
        assert_eq!(Msg([0xF8, 0, 0]).len(), 1);
    }
}
