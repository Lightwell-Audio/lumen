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
    fn clips_chosen(&self) -> bool {
        !self.arrangement.clips.is_empty() || self.arrangement.range.is_some()
    }

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

    pub(crate) fn window_open(&self) -> bool {
        self.other_window_open() || self.welcome.open
    }

    pub(crate) fn other_window_open(&self) -> bool {
        self.shortcuts.open
            || self.preferences.open
            || self.show_history
            || self.show_bounce
            || self.export.open
            || self.project_settings.is_open()
            || self.controllers.window
            || self.drum_choice.is_some()
            || self.prompt.is_some()
    }

    pub(crate) fn sync_selection(&mut self) {
        if !self.arrangement.selected.is_empty() {
            self.arrangement.clips.clear();
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

    pub(crate) fn selected_midi_tracks(&self) -> Vec<lumen_project::model::Id> {
        let selected = &self.arrangement.selected;
        let tracks = &self.document.project().tracks;
        tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Midi && selected.contains(&t.id))
            .map(|t| t.id)
            .collect()
    }

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

    pub(crate) fn command_keys(&mut self, ctx: &egui::Context) {
        self.sync_selection();
        if ctx.text_edit_focused() || self.typing || self.rebind.is_some() {
            return;
        }
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
            if matches!(command, Command::SlipClipEarlier | Command::SlipClipLater)
                && !self.audio_editor_live(ctx)
            {
                continue;
            }
            let ignored = repeat && command == Command::TapTempo;
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

#[derive(Debug, PartialEq, Eq)]
enum ContextRow {
    Ready,
    Waiting(&'static str),
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
