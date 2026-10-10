//! The clipboard chords, back as keys (owner, after M4 slice 7: Ctrl+C and
//! Ctrl+V did nothing in the arrangement).
//!
//! egui-winit turns the clipboard chords into clipboard events and drops
//! the key press: Ctrl/Cmd+C becomes `Event::Copy`, Ctrl/Cmd+X
//! `Event::Cut`, and Ctrl/Cmd+V `Event::Paste` only when the system
//! clipboard holds text (otherwise nothing at all). On Windows it does the
//! same with Ctrl+Insert, Shift+Insert and Shift+Delete, Ctrl+Shift+Delete
//! (Delete time) included. Only the keys' releases come through as keys.
//!
//! Text fields want those events as they are. Everywhere else Lumen's
//! keymap decides what a chord does, so this puts the key presses back
//! before anything reads the frame's keys: Copy as C, Paste as V, Cut as
//! X; a Cut with Shift held (Ctrl+Shift+X or a Shift+Delete chord) waits
//! for its release to say which key it was; a V released with no Paste
//! seen (the clipboard held no text) is pressed then.

use eframe::egui::{self, Event, Key, Modifiers};

use crate::App;

impl App {
    /// Puts this frame's clipboard chords back as keys, unless a text
    /// field has the keyboard. Call before anything reads the keys.
    pub(crate) fn restore_clipboard_keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.text_edit_focused() || self.typing;
        let mut pending = self.clipboard_keys;
        ctx.input_mut(|i| {
            let modifiers = i.modifiers;
            restore(&mut i.events, modifiers, typing, &mut pending);
        });
        self.clipboard_keys = pending;
    }
}

/// What the last clipboard chords left to settle.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pending {
    /// A Paste event came for the V being held.
    pasted: bool,
    /// A Cut came with Shift held: its key is known when it is released.
    cut: bool,
}

fn press(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// With the command modifier, as the chord was pressed (a media Copy key
/// has none).
fn with_command(modifiers: Modifiers) -> Modifiers {
    Modifiers {
        command: true,
        ctrl: modifiers.ctrl || !cfg!(target_os = "macos"),
        mac_cmd: modifiers.mac_cmd || cfg!(target_os = "macos"),
        ..modifiers
    }
}

/// Puts the clipboard chords in `events` back as key presses. `modifiers`
/// are the frame's; `typing` leaves the events to a text field.
pub(crate) fn restore(
    events: &mut Vec<Event>,
    modifiers: Modifiers,
    typing: bool,
    pending: &mut Pending,
) {
    let mut out = Vec::with_capacity(events.len());
    for event in events.drain(..) {
        match event {
            Event::Key {
                key,
                pressed: false,
                modifiers: held,
                ..
            } if matches!(key, Key::V | Key::X | Key::Delete) => {
                if !typing {
                    if key == Key::V && held.command && !pending.pasted {
                        out.push(press(Key::V, held));
                    }
                    if key != Key::V && pending.cut {
                        out.push(press(key, held));
                    }
                }
                if key == Key::V {
                    pending.pasted = false;
                } else {
                    pending.cut = false;
                }
                out.push(event);
            }
            _ if typing => out.push(event),
            Event::Copy => out.push(press(Key::C, with_command(modifiers))),
            Event::Paste(_) => {
                pending.pasted = true;
                out.push(press(Key::V, with_command(modifiers)));
            }
            Event::Cut if modifiers.shift => pending.cut = true,
            Event::Cut => out.push(press(Key::X, with_command(modifiers))),
            other => out.push(other),
        }
    }
    *events = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers,
        }
    }

    /// The key presses in `events`, as (key, modifiers).
    fn presses(events: &[Event]) -> Vec<(Key, Modifiers)> {
        events
            .iter()
            .filter_map(|e| match *e {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((key, modifiers)),
                _ => None,
            })
            .collect()
    }

    fn run(events: Vec<Event>, modifiers: Modifiers, pending: &mut Pending) -> Vec<Event> {
        let mut events = events;
        restore(&mut events, modifiers, false, pending);
        events
    }

    #[test]
    fn copy_cut_and_paste_events_are_pressed_keys_again() {
        let mut pending = Pending::default();
        let ctrl = Modifiers::COMMAND;
        let events = run(
            vec![Event::Copy, Event::Cut, Event::Paste("text".into())],
            ctrl,
            &mut pending,
        );
        let keys: Vec<Key> = presses(&events).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, [Key::C, Key::X, Key::V]);
        assert!(presses(&events).iter().all(|(_, m)| m.command));
        // The V's release after a Paste presses nothing more.
        let events = run(vec![release(Key::V, ctrl)], ctrl, &mut pending);
        assert!(presses(&events).is_empty());
    }

    #[test]
    fn ctrl_v_with_no_text_on_the_clipboard_presses_v_on_release() {
        // egui-winit sends nothing for the press, only the release.
        let mut pending = Pending::default();
        let ctrl = Modifiers::COMMAND;
        let events = run(vec![release(Key::V, ctrl)], ctrl, &mut pending);
        assert_eq!(presses(&events), [(Key::V, ctrl)]);
        assert_eq!(events.len(), 2, "the release stays");
        // A plain V released is a V, nothing more.
        let events = run(
            vec![release(Key::V, Modifiers::NONE)],
            Modifiers::NONE,
            &mut pending,
        );
        assert!(presses(&events).is_empty());
    }

    #[test]
    fn a_cut_with_shift_waits_for_its_key() {
        // Ctrl+Shift+Delete (Delete time) arrives as a Cut on Windows.
        let mut pending = Pending::default();
        let chord = Modifiers::COMMAND | Modifiers::SHIFT;
        let events = run(vec![Event::Cut], chord, &mut pending);
        assert!(presses(&events).is_empty(), "not known yet");
        let events = run(vec![release(Key::Delete, chord)], chord, &mut pending);
        assert_eq!(presses(&events), [(Key::Delete, chord)]);
        // And Ctrl+Shift+X is an X.
        run(vec![Event::Cut], chord, &mut pending);
        let events = run(vec![release(Key::X, chord)], chord, &mut pending);
        assert_eq!(presses(&events), [(Key::X, chord)]);
        // Delete released without a Cut is only a release.
        let events = run(vec![release(Key::Delete, chord)], chord, &mut pending);
        assert!(presses(&events).is_empty());
    }

    #[test]
    fn a_text_field_keeps_the_clipboard_events() {
        let mut pending = Pending::default();
        let ctrl = Modifiers::COMMAND;
        let mut events = vec![Event::Copy, Event::Paste("a".into()), release(Key::V, ctrl)];
        restore(&mut events, ctrl, true, &mut pending);
        assert!(matches!(events[0], Event::Copy));
        assert!(matches!(events[1], Event::Paste(_)));
        assert!(presses(&events).is_empty());
        assert_eq!(pending, Pending::default());
    }
}
