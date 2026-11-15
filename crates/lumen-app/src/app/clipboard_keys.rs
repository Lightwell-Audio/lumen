use eframe::egui::{self, Event, Key, Modifiers};

use crate::App;

impl App {
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pending {
    pasted: bool,
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

fn with_command(modifiers: Modifiers) -> Modifiers {
    Modifiers {
        command: true,
        ctrl: modifiers.ctrl || !cfg!(target_os = "macos"),
        mac_cmd: modifiers.mac_cmd || cfg!(target_os = "macos"),
        ..modifiers
    }
}

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
        let events = run(vec![release(Key::V, ctrl)], ctrl, &mut pending);
        assert!(presses(&events).is_empty());
    }

    #[test]
    fn ctrl_v_with_no_text_on_the_clipboard_presses_v_on_release() {
        let mut pending = Pending::default();
        let ctrl = Modifiers::COMMAND;
        let events = run(vec![release(Key::V, ctrl)], ctrl, &mut pending);
        assert_eq!(presses(&events), [(Key::V, ctrl)]);
        assert_eq!(events.len(), 2, "the release stays");
        let events = run(
            vec![release(Key::V, Modifiers::NONE)],
            Modifiers::NONE,
            &mut pending,
        );
        assert!(presses(&events).is_empty());
    }

    #[test]
    fn a_cut_with_shift_waits_for_its_key() {
        let mut pending = Pending::default();
        let chord = Modifiers::COMMAND | Modifiers::SHIFT;
        let events = run(vec![Event::Cut], chord, &mut pending);
        assert!(presses(&events).is_empty(), "not known yet");
        let events = run(vec![release(Key::Delete, chord)], chord, &mut pending);
        assert_eq!(presses(&events), [(Key::Delete, chord)]);
        run(vec![Event::Cut], chord, &mut pending);
        let events = run(vec![release(Key::X, chord)], chord, &mut pending);
        assert_eq!(presses(&events), [(Key::X, chord)]);
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
