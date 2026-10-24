//! Whether each command can run now, and why not (M3 slice 7).

use eframe::egui;
use lumen_project::model::TrackKind;

use crate::App;
use crate::app::panes;
use crate::commands::Command;

impl App {
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
            Command::LeaveSafeMode => need(self.safe.on, "Safe mode is not on"),
            Command::DevicePreset | Command::DeviceSavePreset => need(
                self.chosen_has_presets(),
                "Choose a plugin that is running first",
            ),
            Command::DeviceOutputs => need(
                self.chosen_has_outputs(),
                "Choose a plugin instrument that has several outputs first",
            ),
            Command::DeviceSidechain => need(
                self.chosen_has_sidechain(),
                "Choose a plugin effect that has a sidechain input first",
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
            Command::Copy if self.auto_keys_picked() => Ok(()),
            Command::Paste if self.auto_can_paste() => Ok(()),
            Command::Cut | Command::Copy => need(clips, "Select clips or a time range first"),
            Command::Paste => need(
                self.clipboard.is_some(),
                "Nothing to paste: copy or cut first",
            ),
            Command::Duplicate => need(
                clips || tracks || notes || self.auto_keys_picked(),
                "Select clips, a time range or tracks",
            ),
            Command::Delete => need(
                clips
                    || tracks
                    || locator
                    || notes
                    || self.arrangement.gesture.is_some()
                    || self.song_keys_picked()
                    || self.lane_keys_picked()
                    || self.auto_keys_picked()
                    || self.chain.live,
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
                    || (self.auto_keys_lane() && self.auto_chosen_has_lane())
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
                (clips
                    || self.arrangement.tool != crate::app::tools::ArrangementTool::Pointer
                    || locator
                    || notes
                    || self.arrangement.auto.picks.is_some()
                    || self.note_editor.gesture.is_some())
                    && !egui::Popup::is_any_open(ctx)
                    && !self.window_open(),
                "Nothing is selected",
            ),
            Command::Rename => need(
                clip || tracks || locator,
                "Select a clip, a track or a locator",
            ),
            Command::MuteClips => need(clip || notes, "Select a clip first"),
            Command::ToggleCrossfade => {
                need(self.selection_has_audio(), "Select an audio clip first")
            }
            Command::ToolPointer
            | Command::ToolDraw
            | Command::ToolErase
            | Command::ToolSplit
            | Command::ToolMute
            | Command::ToolZoom => Ok(()),
            Command::CropToSelection => need(
                self.arrangement.range.is_some(),
                "Select a time range first",
            ),
            Command::UnloopClips => need(
                self.document
                    .project()
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .any(|c| {
                        self.arrangement.clips.contains(&c.id)
                            && !c.locked
                            && c.loop_length.is_some_and(|l| l.0 > 0)
                    }),
                "Select a looping clip first",
            ),
            Command::NudgeClipsEarlier
            | Command::NudgeClipsLater
            | Command::NudgeClipsEarlierFine
            | Command::NudgeClipsLaterFine
            | Command::SelectAllInTrack
            | Command::SelectToStart
            | Command::SelectToEnd => need(clip, "Select a clip first"),
            Command::ConsolidateClips => need(
                self.document
                    .project()
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .any(|c| self.arrangement.clips.contains(&c.id) && !c.locked),
                "Select a clip first",
            ),
            Command::ReverseClips | Command::InvertPolarity | Command::NormalizeClips => need(
                self.document
                    .project()
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .any(|c| {
                        self.arrangement.clips.contains(&c.id)
                            && !c.locked
                            && matches!(c.content, lumen_project::model::ClipContent::Audio { .. })
                    }),
                "Select an audio clip first",
            ),
            Command::InvertSelection => need(
                self.document
                    .project()
                    .tracks
                    .iter()
                    .any(|t| !t.clips.is_empty()),
                "There are no clips yet",
            ),
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
            Command::DrawTool => Ok(()),
            Command::PopOutEditor
            | Command::Quantize
            | Command::QuantizeOptions
            | Command::HumanizeNotes
            | Command::FixedLengthNotes
            | Command::ScaleConstrainNotes
            | Command::ReverseNotes => {
                need(self.note_editor.clip.is_some(), "Open a MIDI clip first")
            }
            Command::SplitNotes => need(
                (self.notes_are_picked() && self.split_point().is_some()) || clips,
                if self.notes_are_picked() {
                    "Move the playhead into the clip, or point at the clip editor"
                } else {
                    "Select clips or a time range first"
                },
            ),
            Command::GlueNotes => need(
                self.notes_are_picked() || self.arrangement.clips.len() > 1,
                "Select two or more clips, or notes, first",
            ),
            Command::NudgeEarlierFine | Command::NudgeLaterFine
                if !self.notes_are_picked() && clip =>
            {
                Ok(())
            }
            Command::LegatoNotes
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
            Command::FoldTracks | Command::UnfoldTracks => need(
                tracks || clip || notes || self.auto_keys_picked(),
                "Select a clip or tracks first",
            ),
            Command::AutomationShow => need(!self.auto_tracks().is_empty(), "Select a track first"),
            Command::AutomationChoose | Command::AutomationAddPoint => {
                need(self.auto_chosen().is_some(), "Show a lane first (A)")
            }
            Command::AutomationSegmentLinear
            | Command::AutomationSegmentStep
            | Command::AutomationSegmentCurve => {
                need(self.auto_keys_picked(), "Pick automation points first")
            }
            Command::AutomationToolRamp
            | Command::AutomationToolSine
            | Command::AutomationToolTriangle
            | Command::AutomationToolSaw
            | Command::AutomationToolSquare => {
                need(self.auto_chosen().is_some(), "Show a lane first (A)")
            }
            Command::AutomationModeRead
            | Command::AutomationModeTouch
            | Command::AutomationModeLatch
            | Command::AutomationModeWrite => {
                need(!self.auto_tracks().is_empty(), "Select a track first")
            }
            Command::AutomationReenable => need(
                self.auto_chosen()
                    .is_some_and(|(t, target)| self.document.overrides().contains(t, target)),
                "The shown lane is not taken over",
            ),
            Command::AutomationReenableAll => need(
                !self.document.overrides().is_empty(),
                "No lane is taken over",
            ),
            Command::ControllerLearn => need(
                self.auto_chosen().is_some(),
                "Show a lane first (A): it names the control to map",
            ),
            Command::ControllerClear => need(
                self.auto_chosen()
                    .is_some_and(|(t, target)| self.document.mapping_of(t, target).is_some()),
                "No controller moves the shown lane's control",
            ),
            Command::ControllerMappings => Ok(()),
            Command::SongLanesShow | Command::TempoChangeAdd | Command::MeterChangeAdd => Ok(()),
            Command::TempoRamp => need(
                self.song_ramp_target().is_some(),
                "Pick a tempo change that has another after it",
            ),
            Command::LockEnvelopes => Ok(()),
            Command::ClipEnvelope => need(
                self.selected_audio_clip().is_some(),
                "Select one audio clip first",
            ),
            Command::AutomationToggleLane | Command::AutomationRemoveLane => need(
                self.auto_chosen_has_lane(),
                "The shown lane has no points yet: click it to add one",
            ),
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
            | Command::ToggleAutoCrossfade
            | Command::Save
            | Command::SaveAs => Ok(()),
        }
    }
}
