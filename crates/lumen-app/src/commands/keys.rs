//! Shortcuts: a chord (modifiers and a key) and the keymap from chords to
//! commands. Chords match their modifiers exactly. egui's own matching
//! lets a plain key match with Shift or Alt held, which made every
//! shortcut handler order its checks (Alt+Up before Up); here Alt+Up and
//! Up are simply different chords.

use std::collections::BTreeMap;

use eframe::egui::{Key, Modifiers};

use super::{ALL, Command};

/// Modifiers and a key. Ctrl is the command key: Cmd on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

pub(crate) const fn plain(key: Key) -> Chord {
    Chord {
        ctrl: false,
        shift: false,
        alt: false,
        key,
    }
}

pub(crate) const fn ctrl(key: Key) -> Chord {
    Chord {
        ctrl: true,
        ..plain(key)
    }
}

pub(crate) const fn shift(key: Key) -> Chord {
    Chord {
        shift: true,
        ..plain(key)
    }
}

pub(crate) const fn alt(key: Key) -> Chord {
    Chord {
        alt: true,
        ..plain(key)
    }
}

pub(crate) const fn ctrl_shift(key: Key) -> Chord {
    Chord {
        ctrl: true,
        shift: true,
        ..plain(key)
    }
}

impl Chord {
    /// The chord of a key pressed with `modifiers`.
    pub(crate) fn pressed(modifiers: Modifiers, key: Key) -> Self {
        Self {
            ctrl: modifiers.command,
            shift: modifiers.shift,
            alt: modifiers.alt,
            key,
        }
    }

    /// The modifiers held, in the order they are written: "Ctrl", "Shift",
    /// "Alt" (Cmd and Option on macOS).
    pub(crate) fn modifier_names(self) -> Vec<&'static str> {
        let mac = cfg!(target_os = "macos");
        let mut names = Vec::new();
        if self.ctrl {
            names.push(if mac { "Cmd" } else { "Ctrl" });
        }
        if self.shift {
            names.push("Shift");
        }
        if self.alt {
            names.push(if mac { "Option" } else { "Alt" });
        }
        names
    }

    /// The key's name on a keycap: "D", "Space", "Delete", "Up", "+".
    pub(crate) fn key_name(self) -> &'static str {
        match self.key {
            Key::Escape => "Esc",
            Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight => self.key.name(),
            key => key.symbol_or_name(),
        }
    }

    /// "Ctrl+Shift+Z".
    pub(crate) fn text(self) -> String {
        let mut parts = self.modifier_names();
        parts.push(self.key_name());
        parts.join("+")
    }
}

/// Which chords run which commands.
#[derive(Debug, Clone)]
pub(crate) struct Keymap {
    chords: BTreeMap<Command, Vec<Chord>>,
}

impl Default for Keymap {
    /// The defaults: Ableton's keys where it has them.
    fn default() -> Self {
        Self {
            chords: ALL
                .iter()
                .map(|info| (info.command, info.keys.to_vec()))
                .collect(),
        }
    }
}

impl Keymap {
    /// The chords that run `command`, the one to show first.
    pub(crate) fn chords(&self, command: Command) -> &[Chord] {
        self.chords.get(&command).map_or(&[], Vec::as_slice)
    }

    /// The shortcut to show for `command` ("Ctrl+D"), if it has one.
    pub(crate) fn text(&self, command: Command) -> Option<String> {
        self.chords(command).first().map(|c| c.text())
    }

    /// The command `chord` runs.
    pub(crate) fn command_for(&self, chord: Chord) -> Option<Command> {
        self.chords
            .iter()
            .find(|(_, chords)| chords.contains(&chord))
            .map(|(&command, _)| command)
    }

    /// Chords bound to more than one command, with those commands.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the shortcut editor shows clashes (slice 7c)")
    )]
    pub(crate) fn clashes(&self) -> Vec<(Chord, Vec<Command>)> {
        let mut by_chord: BTreeMap<Chord, Vec<Command>> = BTreeMap::new();
        for (&command, chords) in &self.chords {
            for &chord in chords {
                by_chord.entry(chord).or_default().push(command);
            }
        }
        by_chord
            .into_iter()
            .filter(|(_, commands)| commands.len() > 1)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_default_shortcuts_clash() {
        let clashes = Keymap::default().clashes();
        assert!(clashes.is_empty(), "clashing defaults: {clashes:?}");
    }

    #[test]
    fn chords_match_their_modifiers_exactly() {
        let keymap = Keymap::default();
        let up = |modifiers| keymap.command_for(Chord::pressed(modifiers, Key::ArrowUp));
        assert_eq!(up(Modifiers::NONE), Some(Command::SelectTrackAbove));
        assert_eq!(up(Modifiers::SHIFT), Some(Command::ExtendSelectionUp));
        assert_eq!(up(Modifiers::ALT), Some(Command::MoveTracksUp));
        assert_eq!(up(Modifiers::ALT | Modifiers::SHIFT), None);
        let delete = |modifiers| keymap.command_for(Chord::pressed(modifiers, Key::Delete));
        assert_eq!(delete(Modifiers::SHIFT), None, "Shift+Delete is not Delete");
        assert_eq!(
            delete(Modifiers::COMMAND | Modifiers::SHIFT),
            Some(Command::DeleteTime)
        );
    }

    #[test]
    fn chords_read_as_written_on_the_keys() {
        if cfg!(target_os = "macos") {
            assert_eq!(ctrl_shift(Key::Z).text(), "Cmd+Shift+Z");
            return;
        }
        assert_eq!(ctrl_shift(Key::Z).text(), "Ctrl+Shift+Z");
        assert_eq!(plain(Key::Space).text(), "Space");
        assert_eq!(plain(Key::Escape).text(), "Esc");
        assert_eq!(alt(Key::ArrowUp).text(), "Alt+Up");
        assert_eq!(plain(Key::Num0).text(), "0");
        assert_eq!(plain(Key::Plus).text(), "+");
        let keymap = Keymap::default();
        assert_eq!(keymap.text(Command::Duplicate).as_deref(), Some("Ctrl+D"));
        assert_eq!(keymap.text(Command::Redo).as_deref(), Some("Ctrl+Shift+Z"));
    }
}
