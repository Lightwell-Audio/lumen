//! What each command does (M3 slice 7).

use eframe::egui;
use lumen_core::automation::Shape;
use lumen_core::pan::PanLaw;
use lumen_core::time::Ticks;
use lumen_devices::Preset;
use lumen_plugin_catalog::scan::Mode;
use lumen_project::edit::automation::Wave;
use lumen_project::model::{FromSource, MidiFrom, Monitor, TrackKind};

use crate::App;
use crate::app::editing::Edit;
use crate::app::navigate::ViewStep;
use crate::app::projects::Next;
use crate::arrangement::auto_state::Tool as AutoTool;
use crate::arrangement::select::ClipStep;
use crate::commands::Command;
use crate::note_editor::lane::geometry::LaneKind;
use crate::note_editor::lane::gesture::LaneTool;
use crate::note_editor::{EditorZoom, NoteOp, Tool, VELOCITY_STEP};
use crate::preferences::Page;
use crate::record_settings::CountIn;

impl App {
    /// Runs `command`, whether it came from a key, the palette or a menu.
    /// Callers check [`App::availability`] first.
    pub(crate) fn run(&mut self, ctx: &egui::Context, command: Command) {
        let clips = self.clips_chosen();
        let clip = !self.arrangement.clips.is_empty();
        let notes = self.note_keys_picked();
        let lane_points = self.lane_keys_picked();
        let auto_point = self.auto_keys_picked();
        let song_point = self.song_keys_picked();
        let auto_lane = self.auto_keys_lane();
        let auto_paste = self.auto_can_paste();
        let auto_draw = self.auto_chosen().is_some();
        let note_keys = self.note_keys();
        if command == Command::Deselect && self.note_editor.cancel_gesture() {
            self.end_audition();
            ctx.request_repaint();
            return;
        }
        let doc = &mut self.document;
        match command {
            Command::ToolPointer => {
                self.choose_arrangement_tool(crate::app::tools::ArrangementTool::Pointer)
            }
            Command::ToolDraw => {
                if auto_draw && !note_keys {
                    self.auto_toggle_draw();
                } else if self.note_editor.clip.is_some() {
                    self.note_editor.tool = match self.note_editor.tool {
                        Tool::Pointer => Tool::Draw,
                        Tool::Draw => Tool::Pointer,
                    };
                } else {
                    self.choose_arrangement_tool(crate::app::tools::ArrangementTool::Draw);
                }
            }
            Command::ToolErase => {
                self.choose_arrangement_tool(crate::app::tools::ArrangementTool::Erase)
            }
            Command::ToolSplit => {
                self.choose_arrangement_tool(crate::app::tools::ArrangementTool::Split)
            }
            Command::ToolMute => {
                self.choose_arrangement_tool(crate::app::tools::ArrangementTool::Mute)
            }
            Command::ToolZoom => {
                self.choose_arrangement_tool(crate::app::tools::ArrangementTool::Zoom)
            }
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
            Command::Copy if auto_point => self.auto_copy(),
            Command::Paste if auto_paste => self.auto_paste(),
            Command::Duplicate if auto_point && !notes => self.auto_duplicate(),
            Command::SelectAll if auto_lane => self.auto_select_all(),
            Command::DrawTool if auto_draw && !note_keys => {
                self.auto_toggle_draw();
            }
            Command::Cut => self.edit(Edit::Cut),
            Command::Copy => self.edit(Edit::Copy),
            Command::Paste => self.edit(Edit::Paste),
            // With the clip editor holding the keyboard and notes picked,
            // the keys that act on a selection act on them.
            Command::Delete if song_point => self.song_delete(),
            Command::Delete if lane_points => self.lane_delete(),
            Command::Delete if auto_point && !notes => self.auto_delete(),
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
            // A picked automation point: the arrows move it, Shift for
            // fine value steps.
            Command::FoldTracks if auto_point => self.auto_nudge(-1, 0, false),
            Command::UnfoldTracks if auto_point => self.auto_nudge(1, 0, false),
            Command::SelectTrackAbove if auto_point => self.auto_nudge(0, 1, false),
            Command::SelectTrackBelow if auto_point => self.auto_nudge(0, -1, false),
            Command::ExtendSelectionUp if auto_point => self.auto_nudge(0, 1, true),
            Command::ExtendSelectionDown if auto_point => self.auto_nudge(0, -1, true),
            Command::AutomationShow => self.auto_show(),
            Command::AutomationChoose => self.auto_choose(ctx),
            Command::AutomationAddPoint => self.auto_add_point(),
            Command::AutomationReenable => {
                if let Some((track, target)) = self.auto_chosen() {
                    self.document.reenable_lane(track, target);
                }
            }
            Command::AutomationReenableAll => {
                self.document.reenable_all();
            }
            Command::AutomationModeRead => self.auto_set_mode(crate::auto_record::Mode::Read),
            Command::AutomationModeTouch => self.auto_set_mode(crate::auto_record::Mode::Touch),
            Command::AutomationModeLatch => self.auto_set_mode(crate::auto_record::Mode::Latch),
            Command::AutomationModeWrite => self.auto_set_mode(crate::auto_record::Mode::Write),
            Command::ControllerLearn => {
                if let Some((track, target)) = self.auto_chosen() {
                    if self.controllers.learning == Some((track, target)) {
                        self.stop_learning();
                    } else {
                        self.start_learning(track, target);
                    }
                }
            }
            Command::ControllerClear => {
                if let Some((track, target)) = self.auto_chosen() {
                    self.clear_controller(track, target);
                }
            }
            Command::ControllerMappings => {
                self.controllers.window = !self.controllers.window;
            }
            Command::SongLanesShow => self.song_lanes_toggle(),
            Command::TempoChangeAdd => self.song_add_tempo_at_playhead(),
            Command::MeterChangeAdd => self.song_add_meter_at_playhead(),
            Command::TempoRamp => self.song_toggle_ramp(),
            Command::ClipEnvelope => self.clip_envelope_toggle(),
            Command::LockEnvelopes => {
                let lock = &mut self.settings.editing.lock_envelopes;
                *lock = !*lock;
                self.settings_changed = Some(std::time::Instant::now());
            }
            Command::AutomationToggleLane => self.auto_toggle_lane(),
            Command::AutomationRemoveLane => self.auto_remove_lane(),
            Command::AutomationSegmentLinear => self.auto_shape_segments(Shape::Linear),
            Command::AutomationSegmentStep => self.auto_shape_segments(Shape::Step),
            Command::AutomationSegmentCurve => self.auto_shape_segments(Self::auto_curve()),
            Command::AutomationToolRamp => self.auto_use_tool(AutoTool::Shape(Wave::Ramp)),
            Command::AutomationToolSine => self.auto_use_tool(AutoTool::Shape(Wave::Sine)),
            Command::AutomationToolTriangle => self.auto_use_tool(AutoTool::Shape(Wave::Triangle)),
            Command::AutomationToolSaw => self.auto_use_tool(AutoTool::Shape(Wave::Saw)),
            Command::AutomationToolSquare => self.auto_use_tool(AutoTool::Shape(Wave::Square)),
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
            Command::NudgeEarlierFine if !notes => self.nudge_selected_clips(-1, false),
            Command::NudgeLaterFine if !notes => self.nudge_selected_clips(1, false),
            Command::NudgeClipsEarlier => self.nudge_selected_clips(-1, false),
            Command::NudgeClipsLater => self.nudge_selected_clips(1, false),
            Command::NudgeClipsEarlierFine => self.nudge_selected_clips(-1, true),
            Command::NudgeClipsLaterFine => self.nudge_selected_clips(1, true),
            Command::CropToSelection => self.crop_to_range(),
            Command::UnloopClips => self.unloop_selected_clips(),
            Command::InvertSelection => self.invert_clip_selection(),
            Command::SelectAllInTrack => self.select_clips_in_track(),
            Command::SelectToStart => self.select_clips_toward(false),
            Command::SelectToEnd => self.select_clips_toward(true),
            Command::ReverseClips => self.reverse_selected_clips(),
            Command::InvertPolarity => self.invert_selected_clips(),
            Command::NormalizeClips => self.normalize_selected_clips(ctx),
            Command::ConsolidateClips => self.consolidate_selected_clips(ctx),
            Command::Bounce => self.open_bounce(),
            Command::FreezeTracks => self.freeze_selected_tracks(ctx),
            Command::UnfreezeTracks => self.unfreeze_selected_tracks(),
            Command::FlattenTracks => self.flatten_selected_tracks(),
            Command::NextTrackInput => self.next_input_for_selected(),
            Command::NextTrackMonitor => self.next_monitor_for_selected(),
            Command::TakeNext => self.step_selected_takes(1),
            Command::TakePrevious => self.step_selected_takes(-1),
            Command::TakeKeepOnly => self.keep_only_selected_take(),
            Command::CaptureMidi => self.capture_midi(),
            Command::KeepPlayedNotes => {
                let record = &mut self.settings.record;
                record.keep_played = !record.keep_played;
            }
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
            Command::GlueNotes if notes => self.note_op(NoteOp::Glue),
            Command::GlueNotes => self.join_selected_clips(ctx),
            Command::LegatoNotes => self.note_op(NoteOp::Legato),
            Command::SplitNotes if notes => {
                if let Some(at) = self.split_point() {
                    self.note_op(NoteOp::Split(at));
                }
            }
            Command::SplitNotes => self.split_at(self.transport.position()),
            Command::CreateMidiClip => self.create_midi_clip_here(),
            Command::OpenClipEditor => {
                if let Some(clip) = self.first_selected_clip() {
                    self.open_clip(clip);
                }
            }
            Command::PopOutEditor => self.note_editor.popped = !self.note_editor.popped,
            Command::DrawTool => {
                if auto_lane && !note_keys {
                    self.auto_toggle_draw();
                } else if self.note_editor.clip.is_some()
                    && (self.note_editor.live || !self.arrangement.tool_keys)
                {
                    self.note_editor.tool = match self.note_editor.tool {
                        Tool::Pointer => Tool::Draw,
                        Tool::Draw => Tool::Pointer,
                    };
                } else {
                    let next = if self.arrangement.tool == crate::app::tools::ArrangementTool::Draw
                    {
                        crate::app::tools::ArrangementTool::Pointer
                    } else {
                        crate::app::tools::ArrangementTool::Draw
                    };
                    self.choose_arrangement_tool(next);
                }
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
                if self.arrangement.tool != crate::app::tools::ArrangementTool::Pointer
                    && !note_keys
                    && !auto_lane
                {
                    self.choose_arrangement_tool(crate::app::tools::ArrangementTool::Pointer);
                }
                self.arrangement.auto.picks = None;
                self.arrangement.gesture = None;
                self.arrangement.marquee = None;
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
            Command::ToggleCrossfade => self.toggle_selected_crossfade(),
            Command::ToggleAutoCrossfade => {
                let enabled = !self.document.project().auto_crossfade;
                self.document.set_auto_crossfade(enabled);
            }
            Command::ClipProperties => {
                if let Some(clip) = self.first_selected_clip() {
                    self.open_clip_card(clip);
                }
            }
            Command::AddAudioTrack => self.arrangement.add_track(doc, TrackKind::Audio),
            Command::AddMidiTrack => self.arrangement.add_track(doc, TrackKind::Midi),
            Command::AddReturnTrack => self.arrangement.add_track(doc, TrackKind::Return),
            Command::AddGroupTrack => self.arrangement.add_track(doc, TrackKind::Group),
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
            Command::SaveCopy => self.save_copy(ctx),
            Command::CollectAndSave => self.collect_and_save(ctx),
            Command::SaveTemplate => self.open_save_template(ctx),
            Command::OpenTemplatesFolder => self.open_templates_folder(),
            Command::Revert => self.begin_revert(),
            Command::ProjectSettings => self.open_project_settings(),
            Command::ExportAudio => self.open_audio_export(),
            Command::ShowDevices => self.show_detail_tab(crate::app::detail::DetailTab::Devices),
            Command::ShowClip => self.show_detail_tab(crate::app::detail::DetailTab::Clip),
            Command::ShowMixer => self.show_detail_tab(crate::app::detail::DetailTab::Mixer),
            Command::MeasureLatency => self.preferences.show_measure(),
            Command::Preferences => self.preferences.show(),
            Command::PreferencesAudio => self.preferences.show_page(Page::Audio),
            Command::PreferencesMidi => self.preferences.show_page(Page::Midi),
            Command::PreferencesPlugins => self.preferences.show_page(Page::Plugins),
            Command::History => self.show_history = true,
            Command::TapTempo => self.tap_tempo(ctx),
            Command::ToggleLoop => {
                let on = self.document.project().transport.loop_enabled;
                self.document.set_loop_enabled(!on);
            }
            Command::ToggleMetronome => self.transport.toggle_metronome(),
            Command::PunchIn | Command::PunchOut => {
                let t = self.document.project().transport;
                let (punch_in, punch_out) = if command == Command::PunchIn {
                    (!t.punch_in, t.punch_out)
                } else {
                    (t.punch_in, !t.punch_out)
                };
                self.document.set_punch(punch_in, punch_out);
            }
            Command::GoToEnd => {
                if let Some(end) = crate::app::navigate::end_of_song(self.document.project()) {
                    self.transport.locate(end);
                }
            }
            Command::AudioOnOff => {
                let on = self.transport.audio.is_on();
                self.transport.audio.set_on(!on);
            }
            Command::NextTimeFormat => {
                self.transport.time_format = self.transport.time_format.next();
            }
            Command::ToggleBrowser => self.show_browser = !self.show_browser,
            Command::CueMode => {
                let mixing = &mut self.settings.mixing;
                mixing.cue_mode = !mixing.cue_mode;
                self.settings_changed = Some(std::time::Instant::now());
            }
            Command::MediaPool => self.show_media_pool(),
            Command::LocateMedia => {
                if let Some(id) = self.selected_missing_media() {
                    self.locate_media(ctx, id);
                }
            }
            Command::SearchFolderForMedia => self.search_folder_for_media(ctx),
            Command::RemoveUnusedMedia => self.remove_unused_media(),
            Command::ClearStrayFiles => {
                self.show_media_pool();
                self.clear_stray_files(ctx);
            }
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
            Command::LeaveSafeMode => self.leave_safe_mode(ctx),
            Command::DevicePreset => {
                if let Some(device) = self.chain.selected {
                    self.open_preset_menu(ctx, device, false);
                }
            }
            Command::DeviceSavePreset => self.save_chosen_preset(ctx),
            Command::DeviceSidechain => self.open_sidechain_menu(ctx),
            Command::DeviceOutputs => self.open_outputs_menu(ctx),
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
}
