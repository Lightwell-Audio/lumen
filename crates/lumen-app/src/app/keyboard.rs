//! The computer keyboard as a MIDI keyboard (M5 slice 2, the owner's K A):
//! a mode, held by the app and saved nowhere, in which Ableton's layout of
//! letter keys plays notes. A to K are the white keys from the octave's C,
//! W E T Y U the black keys between them, O and L two more past K; Z and X
//! move the octave, C and V the velocity.
//!
//! [`ComputerKeyboard`] is the pure part: keys in, MIDI messages out, and
//! the rules that keep notes from hanging (each physical key remembers the
//! note it started, so a note sounding while the octave moves still ends on
//! its own key-up). [`App::computer_keyboard_keys`] feeds it the frame's
//! key events, taking the letters it uses before the commands see them.

use std::collections::BTreeMap;

use eframe::egui::{self, Key};
use lumen_core::midi::{Kind, Msg};

use crate::App;

/// The velocities C and V step through.
pub(crate) const VELOCITIES: [u8; 8] = [1, 20, 40, 60, 80, 100, 120, 127];
/// The velocity index the mode starts on (100).
const START_VELOCITY: usize = 5;
/// The octave A plays in at the start: C3, MIDI note 60.
const START_OCTAVE: i8 = 3;
/// The lowest and highest octave (C-2 and C8, as Ableton).
const LOWEST_OCTAVE: i8 = -2;
const HIGHEST_OCTAVE: i8 = 8;

/// The semitones above the octave's C that `key` plays, if it is a note
/// key. A S D F G H J K are the white keys; W E T Y U the black ones; O and
/// L the next octave's C sharp and D, as Ableton has.
pub(crate) fn semitone(key: Key) -> Option<u8> {
    Some(match key {
        Key::A => 0,
        Key::W => 1,
        Key::S => 2,
        Key::E => 3,
        Key::D => 4,
        Key::F => 5,
        Key::T => 6,
        Key::G => 7,
        Key::Y => 8,
        Key::H => 9,
        Key::U => 10,
        Key::J => 11,
        Key::K => 12,
        Key::O => 13,
        Key::L => 14,
        _ => return None,
    })
}

/// Whether the mode takes `key` when it goes down.
pub(crate) fn is_taken(key: Key) -> bool {
    semitone(key).is_some() || matches!(key, Key::Z | Key::X | Key::C | Key::V)
}

/// The mode's state.
#[derive(Debug, Clone)]
pub(crate) struct ComputerKeyboard {
    on: bool,
    octave: i8,
    velocity: usize,
    /// The note each physical key started, to end it on that key's release
    /// whatever the octave is by then.
    held: BTreeMap<Key, u8>,
    /// Note-offs that could not be sent yet (the queue was full); tried
    /// again every frame.
    owed: Vec<Msg>,
    /// What was sent, for tests (`testing`): the newest last.
    sent: Vec<Msg>,
}

impl Default for ComputerKeyboard {
    fn default() -> Self {
        Self {
            on: false,
            octave: START_OCTAVE,
            velocity: START_VELOCITY,
            held: BTreeMap::new(),
            owed: Vec::new(),
            sent: Vec::new(),
        }
    }
}

impl ComputerKeyboard {
    pub(crate) fn is_on(&self) -> bool {
        self.on
    }

    /// The note `key` plays in the current octave; none past MIDI's top.
    pub(crate) fn note_for(&self, key: Key) -> Option<u8> {
        let note = (i32::from(self.octave) + 2) * 12 + i32::from(semitone(key)?);
        u8::try_from(note).ok().filter(|n| *n <= 127)
    }

    pub(crate) fn velocity(&self) -> u8 {
        VELOCITIES[self.velocity]
    }

    #[cfg(test)]
    pub(crate) fn octave(&self) -> i8 {
        self.octave
    }

    /// The notes sounding from the keyboard now, lowest first.
    pub(crate) fn held_notes(&self) -> Vec<u8> {
        let mut notes: Vec<u8> = self.held.values().copied().collect();
        notes.sort_unstable();
        notes
    }

    /// The readout in the bar: the octave range under A to K, then the
    /// velocity ("C3–C4 · vel 100").
    pub(crate) fn readout(&self) -> String {
        format!(
            "C{}\u{2013}C{} \u{00B7} vel {}",
            self.octave,
            self.octave + 1,
            self.velocity()
        )
    }

    /// A key goes down. The message to send, if it plays a note; octave and
    /// velocity keys change the readout. A repeat, or a key already down,
    /// does nothing.
    pub(crate) fn key_down(&mut self, key: Key, repeat: bool) -> Option<Msg> {
        if !self.on || repeat {
            return None;
        }
        match key {
            Key::Z => self.octave = (self.octave - 1).max(LOWEST_OCTAVE),
            Key::X => self.octave = (self.octave + 1).min(HIGHEST_OCTAVE),
            Key::C => self.velocity = self.velocity.saturating_sub(1),
            Key::V => self.velocity = (self.velocity + 1).min(VELOCITIES.len() - 1),
            _ => {
                if self.held.contains_key(&key) {
                    return None;
                }
                let note = self.note_for(key)?;
                self.held.insert(key, note);
                return Some(self.log(Msg::note_on(0, note, self.velocity())));
            }
        }
        None
    }

    /// A key goes up: the note-off for the note it started, if any (even
    /// after the mode is off or the octave moved).
    pub(crate) fn key_up(&mut self, key: Key) -> Option<Msg> {
        let note = self.held.remove(&key)?;
        Some(self.log(Msg::note_off(0, note)))
    }

    /// Lets go of everything held: the note-offs to send.
    pub(crate) fn release_all(&mut self) -> Vec<Msg> {
        let notes: Vec<u8> = std::mem::take(&mut self.held).into_values().collect();
        notes
            .into_iter()
            .map(|note| self.log(Msg::note_off(0, note)))
            .collect()
    }

    /// Turns the mode on or off; the note-offs to send when it ends.
    pub(crate) fn set_on(&mut self, on: bool) -> Vec<Msg> {
        self.on = on;
        if on { Vec::new() } else { self.release_all() }
    }

    fn log(&mut self, msg: Msg) -> Msg {
        if self.sent.len() >= 256 {
            self.sent.remove(0);
        }
        self.sent.push(msg);
        msg
    }

    /// What was sent since the last call (for tests).
    pub(crate) fn take_sent(&mut self) -> Vec<Msg> {
        std::mem::take(&mut self.sent)
    }
}

impl App {
    /// Turns the computer keyboard on or off, letting go of what it holds.
    pub(crate) fn set_computer_keyboard(&mut self, on: bool) {
        let offs = self.keyboard.set_on(on);
        self.send_keyboard(offs);
    }

    /// Sends `msgs` from the keyboard; a note-off the queue refused is kept
    /// to try again.
    fn send_keyboard(&mut self, msgs: Vec<Msg>) {
        for msg in msgs {
            if !self.play_key(msg) && matches!(msg.kind(), Kind::NoteOff { .. }) {
                self.keyboard.owed.push(msg);
            }
        }
    }

    /// Panic lets go of the keyboard's notes first.
    pub(crate) fn release_computer_keyboard(&mut self) {
        let offs = self.keyboard.release_all();
        self.send_keyboard(offs);
    }

    /// The frame's keys while the mode is on, before commands see them:
    /// note, octave and velocity letters play and are taken; Esc leaves the
    /// mode. Text fields, open windows and a rebinding take keys first, and
    /// chords with Ctrl, Cmd, Alt or Shift are never taken. Releases are
    /// always handled, so a note ends on its key-up wherever the keyboard
    /// went, and the window losing focus lets go of everything.
    pub(crate) fn computer_keyboard_keys(&mut self, ctx: &egui::Context) {
        let owed = std::mem::take(&mut self.keyboard.owed);
        self.send_keyboard(owed);
        if !self.keyboard.is_on() && self.keyboard.held.is_empty() {
            return;
        }
        let typing = ctx.text_edit_focused() || self.typing || self.rebind.is_some();
        let free = !typing && !self.window_open();
        let mut out = Vec::new();
        let mut leave = false;
        let keyboard = &mut self.keyboard;
        ctx.input_mut(|input| {
            input.events.retain(|event| match *event {
                egui::Event::WindowFocused(false) => {
                    out.extend(keyboard.release_all());
                    true
                }
                egui::Event::Key {
                    key,
                    pressed: false,
                    ..
                } if keyboard.held.contains_key(&key) => {
                    out.extend(keyboard.key_up(key));
                    false
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat,
                    modifiers,
                    ..
                } if free && keyboard.is_on() => {
                    let plain = !modifiers.command && !modifiers.alt && !modifiers.shift;
                    if plain && key == Key::Escape {
                        leave = true;
                        false
                    } else if plain && is_taken(key) {
                        out.extend(keyboard.key_down(key, repeat));
                        false
                    } else {
                        true
                    }
                }
                // The release of a taken letter that was not held (an
                // octave key, a note past the top) goes with its press.
                egui::Event::Key {
                    key,
                    pressed: false,
                    modifiers,
                    ..
                } if keyboard.is_on() && free && is_taken(key) => !modifiers.is_none(),
                _ => true,
            });
        });
        if leave {
            out.extend(self.keyboard.set_on(false));
            ctx.request_repaint();
        }
        self.send_keyboard(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> ComputerKeyboard {
        let mut k = ComputerKeyboard::default();
        k.set_on(true);
        k
    }

    #[test]
    fn the_letters_play_ableton_s_layout_from_c3() {
        let k = on();
        let notes = [
            Key::A,
            Key::W,
            Key::S,
            Key::E,
            Key::D,
            Key::F,
            Key::T,
            Key::G,
            Key::Y,
            Key::H,
            Key::U,
            Key::J,
            Key::K,
            Key::O,
            Key::L,
        ]
        .map(|key| k.note_for(key).expect("a note"));
        assert_eq!(
            notes,
            [60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74]
        );
        assert_eq!(k.note_for(Key::B), None);
        assert_eq!(k.velocity(), 100);
    }

    #[test]
    fn pressing_a_key_sends_a_note_on_and_releasing_it_the_note_off() {
        let mut k = on();
        assert_eq!(k.key_down(Key::A, false), Some(Msg::note_on(0, 60, 100)));
        assert_eq!(k.held_notes(), [60]);
        assert_eq!(k.key_up(Key::A), Some(Msg::note_off(0, 60)));
        assert!(k.held_notes().is_empty());
        assert_eq!(k.key_up(Key::A), None, "nothing held, nothing to end");
    }

    #[test]
    fn repeats_and_keys_already_down_do_nothing() {
        let mut k = on();
        assert!(k.key_down(Key::A, false).is_some());
        assert_eq!(k.key_down(Key::A, true), None);
        assert_eq!(k.key_down(Key::A, false), None);
        assert_eq!(k.held_notes(), [60]);
        k.key_down(Key::X, false);
        assert_eq!(k.key_down(Key::X, true), None);
        assert_eq!(k.octave(), 4, "a repeat does not move the octave twice");
    }

    #[test]
    fn the_octave_stops_at_c_minus_2_and_c8() {
        let mut k = on();
        for _ in 0..20 {
            k.key_down(Key::Z, false);
        }
        assert_eq!(k.octave(), -2);
        assert_eq!(k.note_for(Key::A), Some(0));
        for _ in 0..20 {
            k.key_down(Key::X, false);
        }
        assert_eq!(k.octave(), 8);
        assert_eq!(k.note_for(Key::A), Some(120));
        assert_eq!(k.note_for(Key::G), Some(127));
        assert_eq!(k.note_for(Key::H), None, "past MIDI's top there is no note");
        assert_eq!(k.key_down(Key::K, false), None);
        assert!(k.held_notes().is_empty());
        assert_eq!(k.readout(), "C8\u{2013}C9 \u{00B7} vel 100");
    }

    #[test]
    fn velocity_steps_through_its_table_and_stops_at_both_ends() {
        let mut k = on();
        k.key_down(Key::V, false);
        assert_eq!(k.velocity(), 120);
        k.key_down(Key::V, false);
        k.key_down(Key::V, false);
        assert_eq!(k.velocity(), 127);
        for _ in 0..12 {
            k.key_down(Key::C, false);
        }
        assert_eq!(k.velocity(), 1);
        assert_eq!(
            k.key_down(Key::A, false).map(|m| m.bytes()[2]),
            Some(1),
            "the note is played at the velocity set"
        );
    }

    #[test]
    fn a_note_sounding_when_the_octave_changes_ends_on_its_own_key_up() {
        let mut k = on();
        k.key_down(Key::A, false);
        k.key_down(Key::X, false);
        assert_eq!(k.key_down(Key::S, false), Some(Msg::note_on(0, 74, 100)));
        assert_eq!(k.key_up(Key::A), Some(Msg::note_off(0, 60)));
        assert_eq!(k.key_up(Key::S), Some(Msg::note_off(0, 74)));
    }

    #[test]
    fn turning_it_off_lets_go_of_everything_held() {
        let mut k = on();
        k.key_down(Key::A, false);
        k.key_down(Key::D, false);
        let offs = k.set_on(false);
        assert_eq!(offs.len(), 2);
        assert!(
            offs.iter()
                .all(|m| matches!(m.kind(), Kind::NoteOff { .. }))
        );
        assert!(k.held_notes().is_empty());
        assert_eq!(k.key_down(Key::A, false), None, "off plays nothing");
    }

    #[test]
    fn the_readout_names_the_octaves_under_a_to_k_and_the_velocity() {
        let mut k = on();
        assert_eq!(k.readout(), "C3\u{2013}C4 \u{00B7} vel 100");
        k.key_down(Key::Z, false);
        k.key_down(Key::C, false);
        assert_eq!(k.readout(), "C2\u{2013}C3 \u{00B7} vel 80");
    }
}
