//! Running commands (M3 slice 7): whether each can run now and why not,
//! what it does, and the keyboard, which runs the command a pressed chord
//! is bound to. The palette and menus run commands the same way.

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::TrackKind;

use super::panes;
use crate::commands::{Chord, Command};
use crate::{App, widgets};

pub(crate) mod availability;
mod run;

pub(crate) use availability::wait;

impl App {
    /// Clips or a time range are selected.
    fn clips_chosen(&self) -> bool {
        !self.arrangement.clips.is_empty() || self.arrangement.range.is_some()
    }

    /// Where the selected clips or the time range start and end.
    fn selection_span(&self) -> Option<(Ticks, Ticks)> {
        if let Some(range) = &self.arrangement.range {
            return Some((range.start, range.end));
        }
        let clips = self
            .document
            .project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| self.arrangement.clips.contains(&c.id));
        clips.fold(None, |span, c| {
            let (start, end) = (c.start, c.start + c.length);
            Some(span.map_or((start, end), |(a, b): (Ticks, Ticks)| {
                (a.min(start), b.max(end))
            }))
        })
    }

    /// A window is open (the shortcuts, Preferences, history, or the
    /// unsaved-changes question): it takes Esc, and Shift+Tab moves focus in
    /// it.
    pub(crate) fn window_open(&self) -> bool {
        self.shortcuts.open
            || self.preferences.open
            || self.show_history
            || self.show_bounce
            || self.export.open
            || self.project_settings.is_open()
            || self.controllers.window
            || self.prompt.is_some()
    }

    /// Selections are one kind at a time: selecting tracks deselects clips
    /// (and a range on other tracks), and selecting anything deselects the
    /// locator.
    pub(crate) fn sync_selection(&mut self) {
        if !self.arrangement.selected.is_empty() {
            self.arrangement.clips.clear();
            // A range lives on the selected tracks (ADR-0049): a range that
            // covers other tracks goes when the selection changes.
            let keep = self
                .arrangement
                .range
                .as_ref()
                .is_none_or(|r| r.covers_exactly(&self.arrangement.selected));
            if !keep {
                self.arrangement.range = None;
            }
        }
        if !self.arrangement.selected.is_empty() || self.clips_chosen() {
            self.locators.selected = None;
        }
    }

    /// The selected tracks that are MIDI tracks.
    pub(crate) fn selected_midi_tracks(&self) -> Vec<lumen_project::model::Id> {
        let selected = &self.arrangement.selected;
        let tracks = &self.document.project().tracks;
        tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Midi && selected.contains(&t.id))
            .map(|t| t.id)
            .collect()
    }

    /// A menu row for `command` named `label`, with its shortcut as
    /// keycaps from the keymap, greyed when it cannot run now; runs it when
    /// clicked. True when it ran.
    pub(crate) fn menu_command(
        &mut self,
        ui: &mut egui::Ui,
        command: Command,
        label: &str,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let ready = self.availability(&ctx, command).is_ok();
        let keys = self.keymap.keys(command);
        crate::testing::coverage::drawn(&ctx, label, command);
        let clicked = widgets::menu_item(ui, label, keys.as_ref(), ready).clicked();
        if clicked {
            self.run(&ctx, command);
        }
        clicked
    }

    /// A context-menu row for `command` (the interface audit's M3): a
    /// command that cannot act on what was clicked is left out; one that
    /// could but is waiting (a join, a bounce or an export under way; the
    /// reasons in [`wait`]) stays, greyed, and its hover says why. True
    /// when it ran.
    pub(crate) fn context_command(
        &mut self,
        ui: &mut egui::Ui,
        command: Command,
        label: &str,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let why = match ContextRow::of(self.availability(&ctx, command)) {
            ContextRow::Ready => None,
            ContextRow::Waiting(why) => Some(why),
            ContextRow::Left => return false,
        };
        let keys = self.keymap.keys(command);
        crate::testing::coverage::drawn(&ctx, label, command);
        let row = widgets::menu_item(ui, label, keys.as_ref(), why.is_none());
        if let Some(why) = why {
            row.on_disabled_hover_text(why);
            return false;
        }
        if row.clicked() {
            self.run(&ctx, command);
            return true;
        }
        false
    }

    /// The keyboard: each chord pressed this frame runs the command it is
    /// bound to, when that can run now, and is then taken from the input so
    /// nothing else sees it. Not while typing in a field, which has its own
    /// keys (an Esc just typed included: `App::typing`). With another
    /// control focused (a button, a slider, a pane), the keys it uses
    /// itself go to it and the rest still run commands: egui's
    /// `egui_wants_keyboard_input` is true for any focused widget, which
    /// stopped every shortcut once a button had been reached with Tab.
    pub(crate) fn command_keys(&mut self, ctx: &egui::Context) {
        self.sync_selection();
        if ctx.text_edit_focused() || self.typing || self.rebind.is_some() {
            return;
        }
        // The clip editor, held by the keyboard through its pane, is not a
        // control that wants the arrows: they are its notes' keys.
        let editor_owns = self.note_editor.live && panes::focused(ctx) == Some(panes::Pane::Detail);
        let focused = ctx.memory(|m| m.focused().is_some()) && !editor_owns;
        let pressed: Vec<(Chord, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match *event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        repeat,
                        ..
                    } => Some((Chord::pressed(modifiers, key), repeat)),
                    _ => None,
                })
                .collect()
        });
        for (chord, repeat) in pressed {
            let Some(command) = self.keymap.command_for(chord) else {
                continue;
            };
            if (focused && chord.used_by_controls()) || self.availability(ctx, command).is_err() {
                continue;
            }
            // A key held down taps once: its repeats are not taps.
            let ignored = repeat && command == Command::TapTempo;
            // While a window is up (the save question, the bounce popover,
            // Export, Project settings, ...), a plain Enter runs no command:
            // it belongs to the window, and would otherwise open the
            // selected clip behind it. A popped-out editor has its own
            // keyboard, and there Enter does nothing.
            if self.window_open()
                && chord == Chord::pressed(egui::Modifiers::NONE, egui::Key::Enter)
            {
                continue;
            }
            ctx.input_mut(|i| {
                let at = i.events.iter().position(|event| {
                    matches!(*event, egui::Event::Key { key, pressed: true, modifiers, .. }
                        if Chord::pressed(modifiers, key) == chord)
                });
                if let Some(at) = at {
                    i.events.remove(at);
                }
            });
            if !ignored {
                self.run(ctx, command);
            }
        }
    }
}

/// How a context menu shows a command (the interface audit's M3).
#[derive(Debug, PartialEq, Eq)]
enum ContextRow {
    Ready,
    /// Greyed, saying why: work under way holds it back.
    Waiting(&'static str),
    /// Left out: it cannot act on what was clicked.
    Left,
}

impl ContextRow {
    fn of(availability: Result<(), &'static str>) -> Self {
        match availability {
            Ok(()) => Self::Ready,
            Err(why) if wait::is(why) => Self::Waiting(why),
            Err(_) => Self::Left,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_context_menu_keeps_a_waiting_row_and_leaves_out_one_that_cannot_act() {
        assert_eq!(ContextRow::of(Ok(())), ContextRow::Ready);
        assert_eq!(
            ContextRow::of(Err(wait::JOIN)),
            ContextRow::Waiting(wait::JOIN)
        );
        assert_eq!(
            ContextRow::of(Err("Select an audio clip first")),
            ContextRow::Left
        );
    }
}
