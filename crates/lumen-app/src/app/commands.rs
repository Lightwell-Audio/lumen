//! Running commands (M3 slice 7): whether each can run now and why not,
//! what it does, and the keyboard, which runs the command a pressed chord
//! is bound to. The palette and menus run commands the same way.

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::TrackKind;

use super::editing::Edit;
use super::navigate::ViewStep;
use super::projects::Next;
use crate::App;
use crate::commands::{Chord, Command};

impl App {
    /// Clips or a time range are selected.
    fn clips_chosen(&self) -> bool {
        !self.arrangement.clips.is_empty() || self.arrangement.range.is_some()
    }

    /// Selections are one kind at a time: selecting tracks deselects clips
    /// and the range, and selecting anything deselects the locator.
    pub(crate) fn sync_selection(&mut self) {
        if !self.arrangement.selected.is_empty() {
            self.arrangement.clips.clear();
            self.arrangement.range = None;
        }
        if !self.arrangement.selected.is_empty() || self.clips_chosen() {
            self.locators.selected = None;
        }
    }

    /// Whether `command` can run now; if not, why, in a few words.
    pub(crate) fn availability(
        &self,
        ctx: &egui::Context,
        command: Command,
    ) -> Result<(), &'static str> {
        let clips = self.clips_chosen();
        let tracks = !self.arrangement.selected.is_empty();
        let locator = self.locators.selected.is_some();
        let clip = !self.arrangement.clips.is_empty();
        let need = |ok: bool, why| if ok { Ok(()) } else { Err(why) };
        match command {
            Command::Undo => need(self.document.history().can_undo(), "Nothing to undo"),
            Command::Redo => need(self.document.history().can_redo(), "Nothing to redo"),
            Command::Cut | Command::Copy => need(clips, "Select clips or a time range first"),
            Command::Paste => need(
                self.clipboard.is_some(),
                "Nothing to paste: copy or cut first",
            ),
            Command::Duplicate => need(clips || tracks, "Select clips, a time range or tracks"),
            Command::Delete => need(
                clips || tracks || locator,
                "Select clips, a time range, tracks or a locator",
            ),
            Command::DeleteTime | Command::InsertTime => need(
                self.arrangement.range.is_some(),
                "Select a time range first",
            ),
            Command::Deselect => need(
                (clips || locator) && !egui::Popup::is_any_open(ctx),
                "Nothing is selected",
            ),
            Command::Rename => need(
                clip || tracks || locator,
                "Select a clip, a track or a locator",
            ),
            Command::MuteClips | Command::ClipProperties => need(clip, "Select a clip first"),
            Command::MoveTracksUp
            | Command::MoveTracksDown
            | Command::FoldTracks
            | Command::UnfoldTracks => need(tracks, "Select tracks first"),
            Command::SelectTrackAbove
            | Command::SelectTrackBelow
            | Command::ExtendSelectionUp
            | Command::ExtendSelectionDown => need(
                !self.document.project().tracks.is_empty(),
                "There are no tracks yet",
            ),
            Command::PlayStop
            | Command::PauseContinue
            | Command::GoToStart
            | Command::ToggleFollow
            | Command::AddAudioTrack
            | Command::AddMidiTrack
            | Command::ZoomIn
            | Command::ZoomOut
            | Command::ZoomBack
            | Command::FitWidth
            | Command::FitHeight
            | Command::ToggleDetail
            | Command::GridNarrower
            | Command::GridWider
            | Command::TripletGrid
            | Command::SnapToGrid
            | Command::FixedGrid
            | Command::SetLocator
            | Command::PreviousLocator
            | Command::NextLocator
            | Command::NewProject
            | Command::OpenProject
            | Command::Save
            | Command::SaveAs => Ok(()),
        }
    }

    /// Runs `command`, whether it came from a key, the palette or a menu.
    /// Callers check [`App::availability`] first.
    pub(crate) fn run(&mut self, ctx: &egui::Context, command: Command) {
        let clips = self.clips_chosen();
        let doc = &mut self.document;
        match command {
            Command::PlayStop => self.transport.play_stop(),
            Command::PauseContinue => self.transport.pause_continue(),
            Command::GoToStart => self.transport.locate(Ticks::ZERO),
            Command::ToggleFollow => self.transport.toggle_follow(),
            Command::Undo => self.undo(false),
            Command::Redo => self.undo(true),
            Command::Cut => self.edit(Edit::Cut),
            Command::Copy => self.edit(Edit::Copy),
            Command::Paste => self.edit(Edit::Paste),
            Command::Duplicate if clips => self.edit(Edit::Duplicate),
            Command::Duplicate => self.arrangement.duplicate_selected(doc),
            Command::Delete if clips => self.edit(Edit::Delete),
            Command::Delete => match self.locators.selected.take() {
                Some(id) => doc.delete_marker(id),
                None => self.arrangement.delete_selected(doc),
            },
            Command::DeleteTime => self.edit(Edit::DeleteTime),
            Command::InsertTime => self.edit(Edit::InsertTime),
            Command::Deselect => {
                self.arrangement.clips.clear();
                self.arrangement.range = None;
                self.locators.selected = None;
            }
            Command::Rename => {
                if let Some(clip) = self.first_selected_clip() {
                    self.start_clip_rename(clip);
                } else if let Some(id) = self.locators.selected {
                    self.start_locator_rename(id);
                } else {
                    self.arrangement.rename_selected(&self.document);
                }
            }
            Command::MuteClips => self.toggle_clip_mute(),
            Command::ClipProperties => {
                if let Some(clip) = self.first_selected_clip() {
                    self.open_clip_card(clip);
                }
            }
            Command::AddAudioTrack => self.arrangement.add_track(doc, TrackKind::Audio),
            Command::AddMidiTrack => self.arrangement.add_track(doc, TrackKind::Midi),
            Command::MoveTracksUp => self.arrangement.move_selected(doc, -1),
            Command::MoveTracksDown => self.arrangement.move_selected(doc, 1),
            Command::SelectTrackAbove => self.arrangement.step_selection(doc, -1, false),
            Command::SelectTrackBelow => self.arrangement.step_selection(doc, 1, false),
            Command::ExtendSelectionUp => self.arrangement.step_selection(doc, -1, true),
            Command::ExtendSelectionDown => self.arrangement.step_selection(doc, 1, true),
            Command::FoldTracks => self.arrangement.fold_selected(true),
            Command::UnfoldTracks => self.arrangement.fold_selected(false),
            Command::ZoomIn => self.nav.steps.push(ViewStep::ZoomIn),
            Command::ZoomOut => self.nav.steps.push(ViewStep::ZoomOut),
            Command::ZoomBack => self.nav.steps.push(ViewStep::Back),
            Command::FitWidth => self.nav.steps.push(ViewStep::FitWidth),
            Command::FitHeight => self.nav.steps.push(ViewStep::FitHeight),
            Command::ToggleDetail => self.toggle_detail_requested = true,
            Command::GridNarrower => self.grid.narrower(),
            Command::GridWider => self.grid.wider(),
            Command::TripletGrid => self.grid.toggle_triplet(),
            Command::SnapToGrid => self.grid.snap = !self.grid.snap,
            Command::FixedGrid => {
                let step = self.grid_step();
                self.grid.toggle_fixed(step);
            }
            Command::SetLocator => self.set_locator(),
            Command::PreviousLocator => self.jump_to_locator(false),
            Command::NextLocator => self.jump_to_locator(true),
            Command::NewProject => self.request(ctx, Next::New),
            Command::OpenProject => self.request(ctx, Next::Open(None)),
            Command::Save => self.save(ctx, false),
            Command::SaveAs => self.save(ctx, true),
        }
        ctx.request_repaint();
    }

    /// The keyboard: each chord pressed this frame runs the command it is
    /// bound to, when that can run now, and is then taken from the input so
    /// nothing else sees it. Not while typing in a field, which has its own
    /// keys (an Esc just typed included: `App::typing`).
    pub(crate) fn command_keys(&mut self, ctx: &egui::Context) {
        self.sync_selection();
        if ctx.egui_wants_keyboard_input() || self.typing {
            return;
        }
        let focused = ctx.memory(|m| m.focused().is_some());
        let pressed: Vec<Chord> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match *event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some(Chord::pressed(modifiers, key)),
                    _ => None,
                })
                .collect()
        });
        for chord in pressed {
            let Some(command) = self.keymap.command_for(chord) else {
                continue;
            };
            if (focused && command.yields_to_focus()) || self.availability(ctx, command).is_err() {
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
            self.run(ctx, command);
        }
    }
}
