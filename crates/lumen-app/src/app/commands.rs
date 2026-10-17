//! Running commands (M3 slice 7): whether each can run now and why not,
//! what it does, and the keyboard, which runs the command a pressed chord
//! is bound to. The palette and menus run commands the same way.

use eframe::egui;
use lumen_core::pan::PanLaw;
use lumen_core::time::Ticks;
use lumen_devices::Preset;
use lumen_plugin_catalog::scan::Mode;
use lumen_project::model::{FromSource, MidiFrom, Monitor, TrackKind};

use super::editing::Edit;
use super::navigate::ViewStep;
use super::panes;
use super::projects::Next;
use crate::arrangement::select::ClipStep;
use crate::commands::{Chord, Command};
use crate::note_editor::lane::geometry::LaneKind;
use crate::note_editor::lane::gesture::LaneTool;
use crate::note_editor::{EditorZoom, NoteOp, Tool, VELOCITY_STEP};
use crate::record_settings::CountIn;
use crate::{App, widgets};

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

    /// A window is open (the shortcuts, audio settings, history, or the
    /// unsaved-changes question): it takes Esc, and Shift+Tab moves focus in
    /// it.
    pub(crate) fn window_open(&self) -> bool {
        self.shortcuts.open
            || self.transport.settings_window.open
            || self.show_history
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
        // The clip editor has the keyboard and notes are picked: the keys
        // that act on a selection act on them.
        let notes = self.note_keys_picked();
        let need = |ok: bool, why| if ok { Ok(()) } else { Err(why) };
        match command {
            Command::Undo | Command::Redo if self.is_recording() => Err("Stop recording first"),
            Command::Undo => need(self.document.history().can_undo(), "Nothing to undo"),
            Command::Redo => need(self.document.history().can_redo(), "Nothing to redo"),
            // The Devices tab holding the keyboard: the editing keys act on
            // the chosen card.
            Command::Copy
            | Command::Duplicate
            | Command::NudgeEarlierFine
            | Command::NudgeLaterFine
                if self.chain.live =>
            {
                need(self.device_chosen(), "Choose a device on a track first")
            }
            Command::Paste if self.chain.live => need(
                self.device_copied(),
                "Nothing to paste: copy a device first",
            ),
            Command::DeviceMoveEarlier
            | Command::DeviceMoveLater
            | Command::DeviceCopy
            | Command::DeviceDuplicate
            | Command::DeviceFold => need(self.device_chosen(), "Choose a device on a track first"),
            Command::DeviceParameters => need(
                self.device_chosen() && self.chosen_has_parameters(),
                "Choose a plugin that has parameters first",
            ),
            Command::DeviceEditor | Command::DeviceLearn => need(
                self.device_chosen() && self.chosen_has_editor(),
                "Choose a plugin that is running first",
            ),
            Command::DevicePaste => need(
                self.device_copied() && self.chain_track().is_some(),
                if self.device_copied() {
                    "Select a track first"
                } else {
                    "Nothing to paste: copy a device first"
                },
            ),
            Command::Cut | Command::Copy => need(clips, "Select clips or a time range first"),
            Command::Paste => need(
                self.clipboard.is_some(),
                "Nothing to paste: copy or cut first",
            ),
            Command::Duplicate => need(
                clips || tracks || notes,
                "Select clips, a time range or tracks",
            ),
            Command::Delete => need(
                clips || tracks || locator || notes || self.lane_keys_picked() || self.chain.live,
                "Select clips, a time range, tracks or a locator",
            ),
            Command::DeleteTime | Command::InsertTime => need(
                self.arrangement.range.is_some(),
                "Select a time range first",
            ),
            Command::ZoomToSelection | Command::LoopSelection => need(
                self.selection_span().is_some(),
                "Select clips or a time range first",
            ),
            Command::SelectAll => need(
                self.note_keys()
                    || self
                        .document
                        .project()
                        .tracks
                        .iter()
                        .any(|t| !t.clips.is_empty()),
                "There are no clips yet",
            ),
            Command::FitLoop => {
                let t = &self.document.project().transport;
                need(t.loop_end > t.loop_start, "There is no loop yet")
            }
            Command::Import => need(self.dialog.is_none(), "A file dialog is open"),
            Command::DevicesSearch | Command::DevicesScanAtStartup => Ok(()),
            Command::DevicesRescan | Command::DevicesRescanAll => {
                need(!self.plugins.is_off(), "Plugins are off for this run")?;
                need(!self.plugins.busy(), "A scan is under way")
            }
            Command::DevicesAddFolder => {
                need(!self.plugins.is_off(), "Plugins are off for this run")?;
                need(self.dialog.is_none(), "A file dialog is open")
            }
            Command::DevicesFavorite => need(
                panes::focused(ctx) == Some(panes::Pane::Browser)
                    && matches!(
                        self.browser.view.selected,
                        Some(crate::browser::view::Selection::Device(_))
                    ),
                "Select a device in the browser first",
            ),
            Command::DevicesAdd => need(
                matches!(
                    self.browser.view.selected,
                    Some(crate::browser::view::Selection::Device(_))
                ),
                "Select a device in the browser first",
            ),
            Command::DeviceRemove | Command::DeviceBypass => {
                need(self.device_chosen(), "Choose a device on a track first")
            }
            Command::ExportMidiFile => match self.dialog {
                Some(_) => need(false, "A file dialog is open"),
                None if self.midi_exporting.is_some() => need(false, "An export is under way"),
                // Cheap enough for every frame: a MIDI track with an
                // unmuted clip (an empty one is refused when it runs).
                None => need(
                    self.document
                        .project()
                        .tracks
                        .iter()
                        .any(|t| t.kind == TrackKind::Midi && t.clips.iter().any(|c| !c.muted)),
                    "There are no MIDI clips to export",
                ),
            },
            Command::ExportMidiClip => match self.dialog {
                Some(_) => need(false, "A file dialog is open"),
                None if self.midi_exporting.is_some() => need(false, "An export is under way"),
                None => need(
                    self.document.project().tracks.iter().any(|t| {
                        t.kind == TrackKind::Midi
                            && t.clips
                                .iter()
                                .any(|c| self.arrangement.clips.contains(&c.id))
                    }),
                    "Select a MIDI clip first",
                ),
            },
            // An open menu or window takes Esc first (MT-APP-14 run 2; the
            // slice 8 accessibility test: Esc deselected a clip instead of
            // closing the shortcuts window).
            Command::Deselect => need(
                (clips || locator || notes || self.note_editor.gesture.is_some())
                    && !egui::Popup::is_any_open(ctx)
                    && !self.window_open(),
                "Nothing is selected",
            ),
            Command::Rename => need(
                clip || tracks || locator,
                "Select a clip, a track or a locator",
            ),
            Command::MuteClips => need(clip || notes, "Select a clip first"),
            Command::ClipProperties | Command::AddPreviousClip | Command::AddNextClip => {
                need(clip, "Select a clip first")
            }
            Command::CreateMidiClip => need(
                self.midi_track_for_new_clip().is_some(),
                "Select a MIDI track first",
            ),
            Command::OpenClipEditor => need(
                self.selected_midi_clip().is_some(),
                "Select a MIDI clip first",
            ),
            Command::PopOutEditor
            | Command::DrawTool
            | Command::Quantize
            | Command::QuantizeOptions
            | Command::HumanizeNotes
            | Command::FixedLengthNotes
            | Command::ScaleConstrainNotes
            | Command::ReverseNotes => {
                need(self.note_editor.clip.is_some(), "Open a MIDI clip first")
            }
            Command::SplitNotes => need(
                self.notes_are_picked() && self.split_point().is_some(),
                if self.notes_are_picked() {
                    "Move the playhead into the clip, or point at the clip editor"
                } else {
                    "Select notes first"
                },
            ),
            Command::GlueNotes
            | Command::LegatoNotes
            | Command::TransposeUp
            | Command::TransposeDown
            | Command::OctaveUp
            | Command::OctaveDown
            | Command::NudgeEarlier
            | Command::NudgeLater
            | Command::NudgeEarlierFine
            | Command::NudgeLaterFine
            | Command::RaiseVelocity
            | Command::LowerVelocity => need(self.notes_are_picked(), "Select notes first"),
            Command::ToggleLane
            | Command::LaneVelocity
            | Command::LaneModWheel
            | Command::LanePitchBend
            | Command::LanePressure
            | Command::LanePencil
            | Command::LaneLine
            | Command::LaneCurve => need(self.note_editor.clip.is_some(), "Open a MIDI clip first"),
            Command::MoveTracksUp | Command::MoveTracksDown => need(tracks, "Select tracks first"),
            Command::GroupTracks => need(
                self.arrangement.selected.iter().any(|id| {
                    self.document
                        .project()
                        .tracks
                        .iter()
                        .any(|t| t.id == *id && lumen_project::groups::can_be_grouped(t.kind))
                }),
                "Select audio, MIDI or group tracks first",
            ),
            Command::Ungroup => {
                need(
                    self.arrangement.selected.iter().any(|id| {
                        self.document.project().tracks.iter().any(|t| {
                            t.id == *id && (t.kind == TrackKind::Group || t.parent.is_some())
                        })
                    }),
                    "Select a group, or tracks in a group",
                )
            }
            Command::InstrumentPoly
            | Command::InstrumentKeys
            | Command::InstrumentBass
            | Command::InstrumentPluck
            | Command::InstrumentPad
            | Command::MidiToInstrument
            | Command::MidiFromAll
            | Command::MidiFromKeyboard
            | Command::MonitorIn
            | Command::MonitorAuto
            | Command::MonitorOff => need(
                !self.selected_midi_tracks().is_empty(),
                "Select a MIDI track first",
            ),
            Command::SoloTracks | Command::MuteTracks => need(tracks, "Select tracks first"),
            Command::ClearSolos => need(
                lumen_project::solo::any_soloed(self.document.project()),
                "Nothing is soloed",
            ),
            Command::ClearClips => need(
                self.arrangement.meters.any_clipped(),
                "No meter has clipped",
            ),
            Command::FoldAllGroups | Command::UnfoldAllGroups => need(
                self.document
                    .project()
                    .tracks
                    .iter()
                    .any(|t| t.kind == TrackKind::Group),
                "There are no group tracks yet",
            ),
            Command::FoldTracks | Command::UnfoldTracks => {
                need(tracks || clip || notes, "Select a clip or tracks first")
            }
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
            | Command::Panic
            | Command::Record
            | Command::Overdub
            | Command::CountInOff
            | Command::CountInOneBar
            | Command::CountInTwoBars
            | Command::CountInFourBars
            | Command::RecordOnlyMetronome
            | Command::ComputerKeyboard
            | Command::ToggleFollow
            | Command::AddAudioTrack
            | Command::AddMidiTrack
            | Command::ZoomIn
            | Command::ZoomOut
            | Command::ZoomBack
            | Command::FitWidth
            | Command::FitHeight
            | Command::ToggleDetail
            | Command::ToggleInfoLine
            | Command::AdditiveSolo
            | Command::TrackColourMeters
            | Command::PanLawConstantPower
            | Command::PanLawCompromise
            | Command::PanLawLinear
            | Command::PanLawBalance
            | Command::OpenPalette
            | Command::OpenShortcuts
            | Command::NextPane
            | Command::PreviousPane
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
        let clip = !self.arrangement.clips.is_empty();
        let notes = self.note_keys_picked();
        let lane_points = self.lane_keys_picked();
        let note_keys = self.note_keys();
        if command == Command::Deselect && self.note_editor.cancel_gesture() {
            self.end_audition();
            ctx.request_repaint();
            return;
        }
        let doc = &mut self.document;
        match command {
            Command::PlayStop => self.transport.play_stop(),
            Command::PauseContinue => self.transport.pause_continue(),
            Command::GoToStart => self.transport.locate(Ticks::ZERO),
            Command::Panic => self.panic(),
            Command::Record => self.toggle_record(),
            Command::Overdub => self.settings.record.overdub = !self.settings.record.overdub,
            Command::CountInOff => self.settings.record.count_in = CountIn::Off,
            Command::CountInOneBar => self.settings.record.count_in = CountIn::OneBar,
            Command::CountInTwoBars => self.settings.record.count_in = CountIn::TwoBars,
            Command::CountInFourBars => self.settings.record.count_in = CountIn::FourBars,
            Command::RecordOnlyMetronome => {
                let record = &mut self.settings.record;
                record.only_while_recording = !record.only_while_recording;
            }
            Command::ComputerKeyboard => {
                let on = !self.keyboard.is_on();
                self.set_computer_keyboard(on);
            }
            Command::InstrumentPoly => self.choose_instrument(Preset::Poly),
            Command::InstrumentKeys => self.choose_instrument(Preset::Keys),
            Command::InstrumentBass => self.choose_instrument(Preset::Bass),
            Command::InstrumentPluck => self.choose_instrument(Preset::Pluck),
            Command::InstrumentPad => self.choose_instrument(Preset::Pad),
            Command::MidiToInstrument => {
                let ids = self.selected_midi_tracks();
                self.document.set_midi_to(&ids, None);
            }
            Command::MidiFromAll | Command::MidiFromKeyboard => {
                let ids = self.selected_midi_tracks();
                let source = if command == Command::MidiFromAll {
                    FromSource::All
                } else {
                    FromSource::Keyboard
                };
                self.document.set_midi_from(
                    &ids,
                    MidiFrom {
                        source,
                        channel: None,
                    },
                );
            }
            Command::MonitorIn | Command::MonitorAuto | Command::MonitorOff => {
                let ids = self.selected_midi_tracks();
                self.document.set_monitor(
                    &ids,
                    match command {
                        Command::MonitorIn => Monitor::In,
                        Command::MonitorAuto => Monitor::Auto,
                        _ => Monitor::Off,
                    },
                );
            }
            Command::ToggleFollow => self.transport.toggle_follow(),
            Command::Undo => self.undo(false),
            Command::Redo => self.undo(true),
            Command::Copy if self.chain.live => self.copy_chain_device(),
            Command::Paste if self.chain.live => self.paste_chain_device(),
            Command::Duplicate if self.chain.live && !notes => self.duplicate_chain_device(),
            Command::NudgeEarlierFine if self.chain.live && !notes => self.move_chain_device(-1),
            Command::NudgeLaterFine if self.chain.live && !notes => self.move_chain_device(1),
            Command::FoldTracks if self.chain.live && !notes => self.step_chain_choice(-1),
            Command::UnfoldTracks if self.chain.live && !notes => self.step_chain_choice(1),
            Command::Cut => self.edit(Edit::Cut),
            Command::Copy => self.edit(Edit::Copy),
            Command::Paste => self.edit(Edit::Paste),
            // With the clip editor holding the keyboard and notes picked,
            // the keys that act on a selection act on them.
            Command::Delete if lane_points => self.lane_delete(),
            Command::Delete if notes => self.note_op(NoteOp::Delete),
            Command::Delete if self.chain.live => self.remove_chain_device(),
            Command::Duplicate if notes => self.note_op(NoteOp::Duplicate),
            Command::MuteClips if notes => self.note_op(NoteOp::ToggleMute),
            Command::SelectAll if note_keys => self.note_op(NoteOp::SelectAll),
            Command::Deselect if notes => {
                let mut editor = std::mem::take(&mut self.note_editor);
                if let Some(selection) = &mut editor.selection {
                    selection.rebind(self.document.project(), Vec::new());
                }
                self.note_editor = editor;
            }
            Command::SelectTrackAbove if notes => self.note_op(NoteOp::Transpose(1)),
            Command::SelectTrackBelow if notes => self.note_op(NoteOp::Transpose(-1)),
            Command::ExtendSelectionUp if notes => self.note_op(NoteOp::Transpose(12)),
            Command::ExtendSelectionDown if notes => self.note_op(NoteOp::Transpose(-12)),
            Command::FoldTracks if notes => self.note_op(NoteOp::Nudge {
                direction: -1,
                fine: false,
            }),
            Command::UnfoldTracks if notes => self.note_op(NoteOp::Nudge {
                direction: 1,
                fine: false,
            }),
            Command::TransposeUp => self.note_op(NoteOp::Transpose(1)),
            Command::TransposeDown => self.note_op(NoteOp::Transpose(-1)),
            Command::OctaveUp => self.note_op(NoteOp::Transpose(12)),
            Command::OctaveDown => self.note_op(NoteOp::Transpose(-12)),
            Command::NudgeEarlier => self.note_op(NoteOp::Nudge {
                direction: -1,
                fine: false,
            }),
            Command::NudgeLater => self.note_op(NoteOp::Nudge {
                direction: 1,
                fine: false,
            }),
            Command::NudgeEarlierFine => self.note_op(NoteOp::Nudge {
                direction: -1,
                fine: true,
            }),
            Command::NudgeLaterFine => self.note_op(NoteOp::Nudge {
                direction: 1,
                fine: true,
            }),
            Command::RaiseVelocity => self.note_op(NoteOp::Velocity(VELOCITY_STEP)),
            Command::LowerVelocity => self.note_op(NoteOp::Velocity(-VELOCITY_STEP)),
            Command::Quantize => self.note_op(NoteOp::Quantize),
            Command::QuantizeOptions => self.open_quantize_options(ctx),
            Command::HumanizeNotes => self.note_op(NoteOp::Humanize),
            Command::FixedLengthNotes => self.note_op(NoteOp::FixedLength),
            Command::ScaleConstrainNotes => self.note_op(NoteOp::ScaleConstrain),
            Command::ReverseNotes => self.note_op(NoteOp::Reverse),
            Command::ToggleLane => {
                self.note_editor.lane.hidden = !self.note_editor.lane.hidden;
            }
            Command::LaneVelocity => self.note_editor.lane.choose(LaneKind::Velocity),
            Command::LaneModWheel => self.note_editor.lane.choose(LaneKind::Controller(1)),
            Command::LanePitchBend => self.note_editor.lane.choose(LaneKind::PitchBend),
            Command::LanePressure => self.note_editor.lane.choose(LaneKind::Pressure),
            Command::LanePencil => self.note_editor.lane.use_tool(LaneTool::Pencil),
            Command::LaneLine => self.note_editor.lane.use_tool(LaneTool::Line),
            Command::LaneCurve => self.note_editor.lane.use_tool(LaneTool::Curve),
            Command::GlueNotes => self.note_op(NoteOp::Glue),
            Command::LegatoNotes => self.note_op(NoteOp::Legato),
            Command::SplitNotes => {
                if let Some(at) = self.split_point() {
                    self.note_op(NoteOp::Split(at));
                }
            }
            Command::CreateMidiClip => self.create_midi_clip_here(),
            Command::OpenClipEditor => {
                if let Some(clip) = self.selected_midi_clip() {
                    self.open_note_editor(clip);
                }
            }
            Command::PopOutEditor => self.note_editor.popped = !self.note_editor.popped,
            Command::DrawTool => {
                self.note_editor.tool = match self.note_editor.tool {
                    Tool::Pointer => Tool::Draw,
                    Tool::Draw => Tool::Pointer,
                };
            }
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
            Command::GroupTracks => self.arrangement.group_selected(doc),
            Command::Ungroup => self.arrangement.ungroup_selected(doc),
            Command::FoldAllGroups => self.arrangement.collapse_all(doc.project(), true),
            Command::UnfoldAllGroups => self.arrangement.collapse_all(doc.project(), false),
            Command::MoveTracksUp => self.arrangement.move_selected(doc, -1),
            Command::MoveTracksDown => self.arrangement.move_selected(doc, 1),
            // The arrows follow the selection (the owner's choice for H2):
            // clips when a clip is selected, tracks otherwise.
            Command::SelectTrackAbove if clip => self.step_clips(ClipStep::Above, false),
            Command::SelectTrackBelow if clip => self.step_clips(ClipStep::Below, false),
            Command::ExtendSelectionUp if clip => self.step_clips(ClipStep::Above, true),
            Command::ExtendSelectionDown if clip => self.step_clips(ClipStep::Below, true),
            Command::FoldTracks if clip => self.step_clips(ClipStep::Previous, false),
            Command::UnfoldTracks if clip => self.step_clips(ClipStep::Next, false),
            Command::AddPreviousClip => self.step_clips(ClipStep::Previous, true),
            Command::AddNextClip => self.step_clips(ClipStep::Next, true),
            Command::SelectTrackAbove => self.arrangement.step_selection(doc, -1, false),
            Command::SelectTrackBelow => self.arrangement.step_selection(doc, 1, false),
            Command::ExtendSelectionUp => self.arrangement.step_selection(doc, -1, true),
            Command::ExtendSelectionDown => self.arrangement.step_selection(doc, 1, true),
            Command::FoldTracks => self.arrangement.fold_selected(doc.project(), true),
            Command::UnfoldTracks => self.arrangement.fold_selected(doc.project(), false),
            // With the clip editor's keys live, the zoom keys zoom its roll
            // (its view is its own, ADR-0048); otherwise the arrangement.
            Command::ZoomIn if note_keys => self.note_editor.zooms.push(EditorZoom::In),
            Command::ZoomOut if note_keys => self.note_editor.zooms.push(EditorZoom::Out),
            Command::ZoomIn => self.nav.steps.push(ViewStep::ZoomIn),
            Command::ZoomOut => self.nav.steps.push(ViewStep::ZoomOut),
            Command::ZoomBack => self.nav.steps.push(ViewStep::Back),
            Command::FitWidth => self.nav.steps.push(ViewStep::FitWidth),
            Command::FitHeight => self.nav.steps.push(ViewStep::FitHeight),
            Command::ZoomToSelection => {
                if let Some((start, end)) = self.selection_span() {
                    self.nav.steps.push(ViewStep::Fit(start, end));
                }
            }
            Command::FitLoop => {
                let t = &self.document.project().transport;
                self.nav.steps.push(ViewStep::Fit(t.loop_start, t.loop_end));
            }
            Command::LoopSelection => {
                if let Some((start, end)) = self.selection_span() {
                    self.document
                        .set_loop("Loop selection", None, start, end, true);
                }
            }
            Command::SelectAll => {
                let all = self.document.project().tracks.iter().flat_map(|t| &t.clips);
                self.arrangement.clips = all.map(|c| c.id).collect();
                self.arrangement.clip_cursor = None;
                self.arrangement.range = None;
                self.arrangement.selected.clear();
            }
            Command::NextPane => self.cycle_pane(true),
            Command::PreviousPane => self.cycle_pane(false),
            Command::ToggleDetail => self.toggle_detail_requested = true,
            Command::SoloTracks => {
                let ids = self.arrangement.selected.clone();
                let solo = !doc
                    .project()
                    .tracks
                    .iter()
                    .filter(|t| ids.contains(&t.id))
                    .all(|t| t.solo);
                doc.set_solo(&ids, solo, !self.settings.mixing.additive_solo);
            }
            Command::MuteTracks => {
                let ids = self.arrangement.selected.clone();
                let mute = !doc
                    .project()
                    .tracks
                    .iter()
                    .filter(|t| ids.contains(&t.id))
                    .all(|t| t.mute);
                doc.set_mute(&ids, mute);
            }
            Command::ClearSolos => doc.clear_solos(),
            Command::AdditiveSolo => {
                let additive = &mut self.settings.mixing.additive_solo;
                *additive = !*additive;
                self.settings_changed = Some(std::time::Instant::now());
            }
            Command::ClearClips => self.arrangement.meters.clear_all_clips(),
            Command::PanLawConstantPower => doc.set_pan_law(PanLaw::ConstantPower),
            Command::PanLawCompromise => doc.set_pan_law(PanLaw::Compromise),
            Command::PanLawLinear => doc.set_pan_law(PanLaw::Linear),
            Command::PanLawBalance => doc.set_pan_law(PanLaw::Balance),
            Command::TrackColourMeters => {
                let coloured = &mut self.settings.mixing.track_colour_meters;
                *coloured = !*coloured;
                self.settings_changed = Some(std::time::Instant::now());
            }
            Command::ToggleInfoLine => {
                let shown = &mut self.settings.appearance.info_line;
                *shown = !*shown;
                self.settings_changed = Some(std::time::Instant::now());
            }
            Command::OpenPalette => self.toggle_palette(),
            Command::OpenShortcuts => self.open_shortcuts(),
            Command::GridNarrower => self.grid.narrower(),
            Command::GridWider => self.grid.wider(),
            Command::TripletGrid => self.grid.toggle_triplet(),
            Command::SnapToGrid => self.grid.snap = !self.grid.snap,
            Command::FixedGrid => {
                // Fix the grid the pointer's view shows: the roll's own
                // zoom while the editor holds the keys.
                let step = if note_keys {
                    let t = self.note_editor.time;
                    self.grid_step_at(t.left, t.zoom)
                } else {
                    self.grid_step()
                };
                self.grid.toggle_fixed(step);
            }
            Command::SetLocator => self.set_locator(),
            Command::PreviousLocator => self.jump_to_locator(false),
            Command::NextLocator => self.jump_to_locator(true),
            Command::NewProject => self.request(ctx, Next::New),
            Command::OpenProject => self.request(ctx, Next::Open(None)),
            Command::Save => self.save(ctx, false),
            Command::SaveAs => self.save(ctx, true),
            Command::Import => self.import_files(ctx),
            Command::DevicesSearch => {
                self.show_browser = true;
                self.browser.focus_search = true;
            }
            Command::DevicesRescan => self.rescan_plugins(ctx, Mode::Changed),
            Command::DevicesRescanAll => self.rescan_plugins(ctx, Mode::All),
            Command::DevicesAddFolder => self.pick_plugin_folder(ctx),
            Command::DevicesFavorite => {
                if let Some(crate::browser::view::Selection::Device(id)) =
                    self.browser.view.selected.clone()
                {
                    self.toggle_favorite(&id);
                }
            }
            Command::DevicesAdd => {
                if let Some(crate::browser::view::Selection::Device(id)) =
                    self.browser.view.selected.clone()
                {
                    self.choose(ctx, &id);
                }
            }
            Command::DeviceRemove => self.remove_chain_device(),
            Command::DeviceBypass => self.toggle_chain_bypass(),
            Command::DeviceMoveEarlier => self.move_chain_device(-1),
            Command::DeviceMoveLater => self.move_chain_device(1),
            Command::DeviceCopy => self.copy_chain_device(),
            Command::DevicePaste => self.paste_chain_device(),
            Command::DeviceDuplicate => self.duplicate_chain_device(),
            Command::DeviceFold => self.toggle_chain_fold(),
            Command::DeviceParameters => self.open_parameters(),
            Command::DeviceEditor => {
                if let Some(device) = self.chain.selected {
                    self.toggle_editor(device);
                }
            }
            Command::DeviceLearn => {
                if let Some(device) = self.chain.selected {
                    self.toggle_learn(device);
                }
            }
            Command::DevicesScanAtStartup => {
                let on = &mut self.settings.plugins.scan_at_startup;
                *on = !*on;
                self.settings_changed = Some(std::time::Instant::now());
            }
            Command::ExportMidiFile => self.export_midi(ctx, false),
            Command::ExportMidiClip => self.export_midi(ctx, true),
        }
        ctx.request_repaint();
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
        let clicked = widgets::menu_item(ui, label, keys.as_ref(), ready).clicked();
        if clicked {
            self.run(&ctx, command);
        }
        clicked
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
            if (focused && chord.used_by_controls()) || self.availability(ctx, command).is_err() {
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
