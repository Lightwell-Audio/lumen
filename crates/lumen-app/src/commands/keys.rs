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

pub(crate) const fn alt_shift(key: Key) -> Chord {
    Chord {
        alt: true,
        shift: true,
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
        modifier_names(self.ctrl, self.shift, self.alt)
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

    /// Whether a focused control uses this chord itself: Space and Enter
    /// press a button, the arrows, Home, End and the page keys move a
    /// slider or a list, Tab moves on; without Ctrl or Alt. With a control
    /// focused, these go to it rather than run a command.
    pub(crate) fn used_by_controls(self) -> bool {
        !self.ctrl
            && !self.alt
            && matches!(
                self.key,
                Key::Space
                    | Key::Enter
                    | Key::Tab
                    | Key::ArrowUp
                    | Key::ArrowDown
                    | Key::ArrowLeft
                    | Key::ArrowRight
                    | Key::Home
                    | Key::End
                    | Key::PageUp
                    | Key::PageDown
            )
    }

    /// How a chord is saved: "Ctrl+Shift+Z", with Ctrl for the command key
    /// on every system and egui's key names, which it reads back.
    pub(crate) fn saved(self) -> String {
        let mut parts = Vec::new();
        for (on, name) in [
            (self.ctrl, "Ctrl"),
            (self.shift, "Shift"),
            (self.alt, "Alt"),
        ] {
            if on {
                parts.push(name);
            }
        }
        parts.push(self.key.name());
        parts.join("+")
    }

    /// Reads a saved chord; `None` for one this version does not know.
    pub(crate) fn from_saved(text: &str) -> Option<Self> {
        let (modifiers, key) = match text.rsplit_once('+') {
            // "Ctrl++" is Ctrl and the plus key.
            Some((rest, "")) => (rest.strip_suffix('+')?, "+"),
            Some((modifiers, key)) => (modifiers, key),
            None => ("", text),
        };
        let mut chord = plain(Key::from_name(key)?);
        for part in modifiers.split('+').filter(|p| !p.is_empty()) {
            match part {
                "Ctrl" => chord.ctrl = true,
                "Shift" => chord.shift = true,
                "Alt" => chord.alt = true,
                _ => return None,
            }
        }
        Some(chord)
    }
}

/// Which chords run which commands.
#[derive(Debug, Clone)]
pub(crate) struct Keymap {
    chords: BTreeMap<Command, Vec<Chord>>,
}

impl Default for Keymap {
    /// The default keys.
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

    /// The shortcut to show for `command` as keycaps, if it has one.
    pub(crate) fn keys(&self, command: Command) -> Option<crate::widgets::Keys> {
        let chord = *self.chords(command).first()?;
        Some(crate::widgets::Keys {
            modifiers: chord.modifier_names(),
            key: chord.key_name(),
            light: command.info().area.light(),
            text: chord.text(),
        })
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

    /// Whether `command` has its default chords.
    pub(crate) fn is_default(&self, command: Command) -> bool {
        self.chords(command) == command.info().keys
    }

    /// Sets chord `slot` of `command` to `chord` (`None` adds a chord). If
    /// another command has `chord`, nothing changes and that command is
    /// returned: the editor asks before taking it.
    pub(crate) fn set(
        &mut self,
        command: Command,
        slot: Option<usize>,
        chord: Chord,
    ) -> Result<(), Command> {
        if let Some(other) = self.command_for(chord)
            && other != command
        {
            return Err(other);
        }
        let chords = self.chords.entry(command).or_default();
        let slot = slot.filter(|&i| i < chords.len());
        // The chord may already be in another of its slots.
        if let Some(at) = chords.iter().position(|&c| c == chord) {
            if slot.is_some_and(|i| i != at) {
                chords.remove(at);
            } else {
                return Ok(());
            }
        }
        match slot {
            Some(i) if i < chords.len() => chords[i] = chord,
            _ => chords.push(chord),
        }
        Ok(())
    }

    /// Sets the chord as [`Keymap::set`] does, taking it from whichever
    /// command had it.
    pub(crate) fn reassign(&mut self, command: Command, slot: Option<usize>, chord: Chord) {
        for (&other, chords) in &mut self.chords {
            if other != command {
                chords.retain(|&c| c != chord);
            }
        }
        // Nothing else has the chord now, so this cannot clash.
        let _ = self.set(command, slot, chord);
    }

    /// Removes chord `slot` of `command`.
    pub(crate) fn remove(&mut self, command: Command, slot: usize) {
        if let Some(chords) = self.chords.get_mut(&command)
            && slot < chords.len()
        {
            chords.remove(slot);
        }
    }

    /// Gives `command` its default chords back, taking them from any
    /// command that has one now.
    pub(crate) fn reset(&mut self, command: Command) {
        self.chords.insert(command, Vec::new());
        for &chord in command.info().keys {
            self.reassign(command, None, chord);
        }
    }

    /// What is saved: the chords of every command not at its defaults, by
    /// command id.
    pub(crate) fn saved(&self) -> BTreeMap<String, Vec<String>> {
        ALL.iter()
            .filter(|info| !self.is_default(info.command))
            .map(|info| {
                let chords = self.chords(info.command).iter().map(|c| c.saved());
                (info.id.to_owned(), chords.collect())
            })
            .collect()
    }

    /// The defaults with `saved` changes on top. Unknown ids and chords
    /// are skipped; a saved chord wins over a default one that clashes
    /// with it (a default added by a later version, say).
    pub(crate) fn from_saved(saved: &BTreeMap<String, Vec<String>>) -> Self {
        let mut keymap = Self::default();
        let changed: Vec<(Command, Vec<Chord>)> = ALL
            .iter()
            .filter_map(|info| {
                let chords = saved.get(info.id)?;
                let chords = chords.iter().filter_map(|c| Chord::from_saved(c));
                Some((info.command, chords.collect()))
            })
            .collect();
        for (command, _) in &changed {
            keymap.chords.insert(*command, Vec::new());
        }
        for (command, chords) in changed {
            for chord in chords {
                keymap.reassign(command, None, chord);
            }
        }
        keymap
    }

    /// Chords bound to more than one command, with those commands.
    #[cfg(test)]
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

/// Modifiers as they are written: "Ctrl", "Shift", "Alt" (Cmd and Option
/// on macOS), in that order.
pub(crate) fn modifier_names(ctrl: bool, shift: bool, alt: bool) -> Vec<&'static str> {
    let mac = cfg!(target_os = "macos");
    let mut names = Vec::new();
    if ctrl {
        names.push(if mac { "Cmd" } else { "Ctrl" });
    }
    if shift {
        names.push("Shift");
    }
    if alt {
        names.push(if mac { "Option" } else { "Alt" });
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_focused_control_keeps_its_own_keys() {
        assert!(plain(Key::Space).used_by_controls());
        assert!(shift(Key::ArrowUp).used_by_controls(), "fine steps");
        assert!(!ctrl(Key::ArrowLeft).used_by_controls(), "locators");
        assert!(!alt(Key::ArrowUp).used_by_controls(), "moving tracks");
        assert!(!plain(Key::F6).used_by_controls(), "panes");
        assert!(!plain(Key::Z).used_by_controls());
    }

    #[test]
    fn every_default_chord_is_saved_and_read_back() {
        for info in ALL {
            for &chord in info.keys {
                assert_eq!(Chord::from_saved(&chord.saved()), Some(chord), "{chord:?}");
            }
        }
        assert_eq!(Chord::from_saved("Ctrl+Shift+Z"), Some(ctrl_shift(Key::Z)));
        assert_eq!(Chord::from_saved("Ctrl++"), Some(ctrl(Key::Plus)));
        assert_eq!(Chord::from_saved("Hyper+Z"), None);
        assert_eq!(Chord::from_saved("Ctrl+Nonsense"), None);
    }

    #[test]
    fn rebinding_asks_before_taking_a_chord_and_resets() {
        let mut keymap = Keymap::default();
        // Ctrl+D is Duplicate's: setting it on Zoom in is refused.
        assert_eq!(
            keymap.set(Command::ZoomIn, Some(0), ctrl(Key::D)),
            Err(Command::Duplicate)
        );
        assert!(keymap.is_default(Command::ZoomIn));
        // Reassigned, it moves.
        keymap.reassign(Command::ZoomIn, Some(0), ctrl(Key::D));
        assert_eq!(keymap.command_for(ctrl(Key::D)), Some(Command::ZoomIn));
        assert!(keymap.chords(Command::Duplicate).is_empty());
        assert!(keymap.clashes().is_empty());
        // A free chord is added with no slot; a slot is replaced.
        assert_eq!(keymap.set(Command::FitWidth, None, alt(Key::W)), Ok(()));
        assert_eq!(
            keymap.chords(Command::FitWidth),
            [plain(Key::W), alt(Key::W)]
        );
        assert_eq!(
            keymap.set(Command::FitWidth, Some(0), shift(Key::W)),
            Ok(())
        );
        assert_eq!(
            keymap.chords(Command::FitWidth),
            [shift(Key::W), alt(Key::W)]
        );
        keymap.remove(Command::FitWidth, 0);
        assert_eq!(keymap.chords(Command::FitWidth), [alt(Key::W)]);
        // Reset gives Duplicate its chord back, from Zoom in.
        keymap.reset(Command::Duplicate);
        assert_eq!(keymap.command_for(ctrl(Key::D)), Some(Command::Duplicate));
        assert!(keymap.clashes().is_empty());
    }

    #[test]
    fn only_changes_are_saved_and_they_win_when_read() {
        let mut keymap = Keymap::default();
        assert!(keymap.saved().is_empty(), "defaults save nothing");
        keymap.reassign(Command::ZoomIn, Some(0), ctrl(Key::D));
        let saved = keymap.saved();
        assert_eq!(saved.len(), 2, "Zoom in and Duplicate changed: {saved:?}");
        assert_eq!(saved["edit.duplicate"], Vec::<String>::new());
        let read = Keymap::from_saved(&saved);
        assert_eq!(read.command_for(ctrl(Key::D)), Some(Command::ZoomIn));
        assert_eq!(read.saved(), saved);
        // A saved chord wins over a default that clashes with it.
        let mut later = BTreeMap::new();
        later.insert("view.fit_width".to_owned(), vec!["Space".to_owned()]);
        later.insert("not.a_command".to_owned(), vec!["F".to_owned()]);
        let read = Keymap::from_saved(&later);
        assert_eq!(read.command_for(plain(Key::Space)), Some(Command::FitWidth));
        assert!(read.chords(Command::PlayStop).is_empty());
        assert!(read.clashes().is_empty());
    }

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
