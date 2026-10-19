//! Every command the app runs from the keyboard, the palette and menus
//! (M3 slice 7): one table with a stable id, a name, an area and default
//! shortcuts for each. What a command does, and whether it can run now,
//! is `app/commands.rs`; which keys run it is `keys.rs`.
//!
//! A key that acts on what is selected is one command: Delete deletes the
//! selected clips, time range, locator or tracks. So a shortcut always
//! means one command, and clashes can be checked.

pub(crate) mod keys;

pub(crate) use keys::{Chord, Keymap};

/// A command. Its place in [`ALL`] is its discriminant (a test checks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Command {
    PlayStop,
    PauseContinue,
    GoToStart,
    Panic,
    Record,
    Overdub,
    CountInOff,
    CountInOneBar,
    CountInTwoBars,
    CountInFourBars,
    RecordOnlyMetronome,
    ToggleFollow,
    LoopSelection,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    DeleteTime,
    InsertTime,
    Deselect,
    SelectAll,
    Rename,
    MuteClips,
    ClipProperties,
    AddPreviousClip,
    AddNextClip,
    AddAudioTrack,
    AddMidiTrack,
    InstrumentPoly,
    InstrumentKeys,
    InstrumentBass,
    InstrumentPluck,
    InstrumentPad,
    MidiToInstrument,
    ComputerKeyboard,
    MidiFromAll,
    MidiFromKeyboard,
    MonitorIn,
    MonitorAuto,
    MonitorOff,
    GroupTracks,
    Ungroup,
    MoveTracksUp,
    MoveTracksDown,
    SelectTrackAbove,
    SelectTrackBelow,
    ExtendSelectionUp,
    ExtendSelectionDown,
    FoldTracks,
    UnfoldTracks,
    FoldAllGroups,
    UnfoldAllGroups,
    SoloTracks,
    MuteTracks,
    ClearSolos,
    AdditiveSolo,
    ClearClips,
    TrackColourMeters,
    PanLawConstantPower,
    PanLawCompromise,
    PanLawLinear,
    PanLawBalance,
    ZoomIn,
    ZoomOut,
    ZoomBack,
    FitWidth,
    FitHeight,
    ZoomToSelection,
    FitLoop,
    ToggleDetail,
    ToggleInfoLine,
    OpenPalette,
    NextPane,
    PreviousPane,
    GridNarrower,
    GridWider,
    TripletGrid,
    SnapToGrid,
    FixedGrid,
    SetLocator,
    PreviousLocator,
    NextLocator,
    NewProject,
    OpenProject,
    Save,
    SaveAs,
    Import,
    ExportMidiFile,
    OpenShortcuts,
    CreateMidiClip,
    ExportMidiClip,
    OpenClipEditor,
    PopOutEditor,
    DrawTool,
    SplitNotes,
    GlueNotes,
    LegatoNotes,
    TransposeUp,
    TransposeDown,
    OctaveUp,
    OctaveDown,
    NudgeEarlier,
    NudgeLater,
    NudgeEarlierFine,
    NudgeLaterFine,
    ToggleLane,
    LaneVelocity,
    LaneModWheel,
    LanePitchBend,
    LanePressure,
    LanePencil,
    LaneLine,
    LaneCurve,
    RaiseVelocity,
    LowerVelocity,
    Quantize,
    QuantizeOptions,
    HumanizeNotes,
    FixedLengthNotes,
    ScaleConstrainNotes,
    ReverseNotes,
    DevicesSearch,
    DevicesRescan,
    DevicesRescanAll,
    DevicesAddFolder,
    DevicesFavorite,
    DevicesScanAtStartup,
    DevicesAdd,
    DeviceRemove,
    DeviceBypass,
    DeviceMoveEarlier,
    DeviceMoveLater,
    DeviceCopy,
    DevicePaste,
    DeviceDuplicate,
    DeviceFold,
    DeviceParameters,
    DeviceEditor,
    DeviceLearn,
    LeaveSafeMode,
    DevicePreset,
    DeviceSavePreset,
    DeviceSidechain,
    DeviceOutputs,
    AutomationShow,
    AutomationChoose,
    AutomationAddPoint,
    AutomationToggleLane,
    AutomationRemoveLane,
    AutomationSegmentLinear,
    AutomationSegmentStep,
    AutomationSegmentCurve,
    AutomationToolRamp,
    AutomationToolSine,
    AutomationToolTriangle,
    AutomationToolSaw,
    AutomationToolSquare,
}

/// Where a command belongs: how the palette and the shortcut list group
/// it, and the colour of its light on a keycap (the owner's K5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Area {
    Transport,
    Edit,
    Clip,
    Track,
    Mixer,
    View,
    Grid,
    Locators,
    Midi,
    File,
    Devices,
}

impl Area {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Area::Transport => "Transport",
            Area::Edit => "Edit",
            Area::Clip => "Clip",
            Area::Track => "Track",
            Area::Mixer => "Mixer",
            Area::View => "View",
            Area::Grid => "Grid",
            Area::Locators => "Locators",
            Area::Midi => "MIDI",
            Area::File => "File",
            Area::Devices => "Devices",
        }
    }

    /// The light under a keycap's key for commands of this area (the
    /// owner's K5): one of the track colours, so areas read apart the way
    /// tracks do.
    pub(crate) fn light(self) -> eframe::egui::Color32 {
        let index = match self {
            Area::Edit => 0,
            Area::Track => 1,
            Area::Mixer => 6,
            Area::View => 2,
            Area::Clip => 3,
            Area::Transport => 4,
            Area::Grid => 5,
            Area::Locators => 9,
            Area::Midi => 7,
            Area::File => 11,
            Area::Devices => 8,
        };
        crate::theme::CLIP_COLORS[index]
    }
}

/// What the table knows about a command.
#[derive(Debug)]
pub(crate) struct Info {
    pub command: Command,
    /// Stable, for saved keymaps: never renamed once shipped.
    pub id: &'static str,
    pub name: &'static str,
    pub area: Area,
    /// The default shortcuts; the first is the one shown.
    pub keys: &'static [Chord],
    /// What it does, in one sentence (the palette's detail strip).
    pub about: &'static str,
}

use eframe::egui::Key;
use keys::{alt, ctrl, ctrl_shift, plain, shift};

/// Every command, in the order of [`Command`]. Kept as a table, one line
/// per command, so it reads as one.
#[rustfmt::skip]
pub(crate) const ALL: &[Info] = &[
    info(Command::PlayStop, "transport.play_stop", "Play / Stop", Area::Transport, &[plain(Key::Space)], "Plays from the start marker, or stops where playback is."),
    info(Command::PauseContinue, "transport.pause_continue", "Stop here / Continue", Area::Transport, &[shift(Key::Space)], "Stops in place, or carries on from the playhead."),
    info(Command::GoToStart, "transport.go_to_start", "Go to start", Area::Transport, &[plain(Key::Home)], "Moves the playhead to the start of the song."),
    info(Command::Panic, "transport.panic", "Panic: silence everything", Area::Transport, &[], "Stops playback and cuts every sound, MIDI notes included; sends All Notes Off on every channel of every MIDI output in use."),
    info(Command::Record, "transport.record", "Record", Area::Transport, &[plain(Key::F9)], "Records the armed MIDI tracks from the play marker, after the count-in; press again to stop recording."),
    info(Command::Overdub, "transport.overdub", "Overdub", Area::Transport, &[], "Switches between Replace (a take replaces the clip under it) and Overdub (a take keeps what is under it and adds to it)."),
    info(Command::CountInOff, "transport.count_in_off", "Count-in: off", Area::Transport, &[], "Starts recording at once."),
    info(Command::CountInOneBar, "transport.count_in_one_bar", "Count-in: 1 bar", Area::Transport, &[], "Clicks one bar before recording starts."),
    info(Command::CountInTwoBars, "transport.count_in_two_bars", "Count-in: 2 bars", Area::Transport, &[], "Clicks two bars before recording starts."),
    info(Command::CountInFourBars, "transport.count_in_four_bars", "Count-in: 4 bars", Area::Transport, &[], "Clicks four bars before recording starts."),
    info(Command::RecordOnlyMetronome, "transport.metronome_record_only", "Metronome only while recording", Area::Transport, &[], "Keeps the metronome silent except while recording and in the count-in."),
    info(Command::ToggleFollow, "transport.follow", "Follow playhead", Area::Transport, &[ctrl_shift(Key::F)], "Keeps the playhead in view while playing."),
    info(Command::LoopSelection, "transport.loop_selection", "Loop selection", Area::Transport, &[ctrl(Key::L)], "Sets the loop to the selected clips or time range and turns it on."),
    info(Command::Undo, "edit.undo", "Undo", Area::Edit, &[ctrl(Key::Z)], "Takes back the last change."),
    info(Command::Redo, "edit.redo", "Redo", Area::Edit, &[ctrl_shift(Key::Z), ctrl(Key::Y)], "Puts back the change just undone."),
    info(Command::Cut, "edit.cut", "Cut", Area::Edit, &[ctrl(Key::X)], "Copies the selected clips or time range, then removes them."),
    info(Command::Copy, "edit.copy", "Copy", Area::Edit, &[ctrl(Key::C)], "Copies the selected clips or time range."),
    info(Command::Paste, "edit.paste", "Paste", Area::Edit, &[ctrl(Key::V)], "Pastes into the time range, or at the playhead."),
    info(Command::Duplicate, "edit.duplicate", "Duplicate", Area::Edit, &[ctrl(Key::D)], "Copies the selection right after itself."),
    info(Command::Delete, "edit.delete", "Delete", Area::Edit, &[plain(Key::Delete), plain(Key::Backspace)], "Deletes the selected clips, time range, tracks or locator."),
    info(Command::DeleteTime, "edit.delete_time", "Delete time", Area::Edit, &[ctrl_shift(Key::Delete)], "Removes the time range, pulling everything after it back."),
    info(Command::InsertTime, "edit.insert_time", "Insert time", Area::Edit, &[ctrl(Key::I)], "Inserts empty time the length of the range, pushing everything after it on."),
    info(Command::Deselect, "edit.deselect", "Deselect", Area::Edit, &[plain(Key::Escape)], "Clears the clip, range or locator selection."),
    info(Command::SelectAll, "edit.select_all", "Select all", Area::Edit, &[ctrl(Key::A)], "Selects every clip."),
    info(Command::Rename, "edit.rename", "Rename", Area::Edit, &[ctrl(Key::R), plain(Key::F2)], "Renames the selected clip, track or locator."),
    info(Command::MuteClips, "clip.mute", "Mute clip", Area::Clip, &[plain(Key::Num0)], "Mutes the selected clips, or unmutes them."),
    info(Command::ClipProperties, "clip.properties", "Clip properties", Area::Clip, &[alt(Key::Enter)], "Pins the properties card beside the clip."),
    info(Command::AddPreviousClip, "clip.add_previous", "Add previous clip to selection", Area::Clip, &[shift(Key::ArrowLeft)], "Adds the clip before the last one chosen, on its track, to the selection."),
    info(Command::AddNextClip, "clip.add_next", "Add next clip to selection", Area::Clip, &[shift(Key::ArrowRight)], "Adds the clip after the last one chosen, on its track, to the selection."),
    info(Command::AddAudioTrack, "track.add_audio", "Add audio track", Area::Track, &[ctrl(Key::T)], "Adds an audio track after the selection."),
    info(Command::AddMidiTrack, "track.add_midi", "Add MIDI track", Area::Track, &[ctrl_shift(Key::T)], "Adds a MIDI track after the selection."),
    info(Command::InstrumentPoly, "track.instrument_poly", "Instrument: Poly", Area::Track, &[], "Gives the selected MIDI tracks the built-in synth's Poly sound: two detuned saws, a warm low-pass, a soft attack."),
    info(Command::InstrumentKeys, "track.instrument_keys", "Instrument: Keys", Area::Track, &[], "Gives the selected MIDI tracks the built-in synth's Keys sound: soft, quick to speak, fading while held."),
    info(Command::InstrumentBass, "track.instrument_bass", "Instrument: Bass", Area::Track, &[], "Gives the selected MIDI tracks the built-in synth's Bass sound: a saw and a square an octave down, closed low-pass."),
    info(Command::InstrumentPluck, "track.instrument_pluck", "Instrument: Pluck", Area::Track, &[], "Gives the selected MIDI tracks the built-in synth's Pluck sound: short and bright, no sustain."),
    info(Command::InstrumentPad, "track.instrument_pad", "Instrument: Pad", Area::Track, &[], "Gives the selected MIDI tracks the built-in synth's Pad sound: slow attack, long release."),
    info(Command::MidiToInstrument, "track.midi_to_instrument", "MIDI To: Instrument", Area::Track, &[], "Plays the selected MIDI tracks on their own instrument, not on a MIDI output port."),
    info(Command::ComputerKeyboard, "midi.computer_keyboard", "Computer keyboard", Area::Midi, &[ctrl_shift(Key::K)], "Plays notes from the letter keys: A to K, W E T Y U for the black keys, Z X octave, C V velocity."),
    info(Command::MidiFromAll, "track.midi_from_all", "MIDI From: All inputs", Area::Track, &[], "Lets the selected MIDI tracks listen to every MIDI input switched on for tracks, and the computer keyboard."),
    info(Command::MidiFromKeyboard, "track.midi_from_keyboard", "MIDI From: Computer keyboard", Area::Track, &[], "Lets the selected MIDI tracks listen to the computer keyboard only."),
    info(Command::MonitorIn, "track.monitor_in", "Monitor: In", Area::Track, &[], "The selected MIDI tracks always play what is played live."),
    info(Command::MonitorAuto, "track.monitor_auto", "Monitor: Auto", Area::Track, &[], "The selected MIDI tracks play what is played live while they are armed or selected."),
    info(Command::MonitorOff, "track.monitor_off", "Monitor: Off", Area::Track, &[], "The selected MIDI tracks never play what is played live."),
    info(Command::GroupTracks, "track.group", "Group tracks", Area::Track, &[ctrl(Key::G)], "Puts the selected tracks into a new group track."),
    info(Command::Ungroup, "track.ungroup", "Ungroup", Area::Track, &[ctrl_shift(Key::G)], "Removes the selected group, keeping its tracks; or takes the selected tracks out of their group."),
    info(Command::MoveTracksUp, "track.move_up", "Move track up", Area::Track, &[alt(Key::ArrowUp)], "Moves the selected tracks one place up, within their group."),
    info(Command::MoveTracksDown, "track.move_down", "Move track down", Area::Track, &[alt(Key::ArrowDown)], "Moves the selected tracks one place down, within their group."),
    info(Command::SelectTrackAbove, "track.select_above", "Select above", Area::Track, &[plain(Key::ArrowUp)], "Selects the track above; with a clip selected, the nearest clip on the track above."),
    info(Command::SelectTrackBelow, "track.select_below", "Select below", Area::Track, &[plain(Key::ArrowDown)], "Selects the track below; with a clip selected, the nearest clip on the track below."),
    info(Command::ExtendSelectionUp, "track.extend_up", "Add above to selection", Area::Track, &[shift(Key::ArrowUp)], "Adds the track above to the selection; with clips selected, the nearest clip above."),
    info(Command::ExtendSelectionDown, "track.extend_down", "Add below to selection", Area::Track, &[shift(Key::ArrowDown)], "Adds the track below to the selection; with clips selected, the nearest clip below."),
    info(Command::FoldTracks, "track.fold", "Previous clip or fold track", Area::Track, &[plain(Key::ArrowLeft)], "With a clip selected, selects the clip before it on its track; with tracks selected, folds them (a group hides its tracks)."),
    info(Command::UnfoldTracks, "track.unfold", "Next clip or unfold track", Area::Track, &[plain(Key::ArrowRight)], "With a clip selected, selects the clip after it on its track; with tracks selected, unfolds them (a group shows its tracks)."),
    info(Command::FoldAllGroups, "track.fold_groups", "Fold all groups", Area::Track, &[], "Hides the tracks inside every group track."),
    info(Command::UnfoldAllGroups, "track.unfold_groups", "Unfold all groups", Area::Track, &[], "Shows the tracks inside every group track."),
    info(Command::SoloTracks, "mixer.solo", "Solo selected tracks", Area::Mixer, &[], "Solos the selected tracks, or unsolos them."),
    info(Command::MuteTracks, "mixer.mute", "Mute selected tracks", Area::Mixer, &[], "Mutes the selected tracks, or unmutes them."),
    info(Command::ClearSolos, "mixer.clear_solos", "Clear all solos", Area::Mixer, &[], "Unsolos every track, so everything not muted is heard again."),
    info(Command::AdditiveSolo, "mixer.additive_solo", "Additive solo", Area::Mixer, &[], "A click on Solo adds the track to what is soloed; Ctrl+click solos it alone."),
    info(Command::ClearClips, "mixer.clear_clips", "Clear clip lights", Area::Mixer, &[], "Turns off every meter's red clip light and resets the peak holds."),
    info(Command::TrackColourMeters, "mixer.track_colour_meters", "Meters in track colours", Area::Mixer, &[], "Meters take their track's colour instead of the accent; above −6 dB they still turn amber, and red past full scale."),
    info(Command::PanLawConstantPower, "mixer.pan_law_constant_power", "Pan law: −3 dB, constant power", Area::Mixer, &[], "A centred mono track plays 3 dB down per side, so its level stays even across the sweep. The default. Stereo tracks keep their level at the centre."),
    info(Command::PanLawCompromise, "mixer.pan_law_compromise", "Pan law: −4.5 dB", Area::Mixer, &[], "A centred mono track plays 4.5 dB down per side: between constant power and linear. Stereo tracks keep their level at the centre."),
    info(Command::PanLawLinear, "mixer.pan_law_linear", "Pan law: −6 dB, linear", Area::Mixer, &[], "A centred mono track plays 6 dB down per side; the two sides always add up to unity. Stereo tracks keep their level at the centre."),
    info(Command::PanLawBalance, "mixer.pan_law_balance", "Pan law: 0 dB, balance", Area::Mixer, &[], "A centred mono track plays at unity on both sides; panning turns only the far side down. Stereo tracks always pan this way."),
    info(Command::ZoomIn, "view.zoom_in", "Zoom in", Area::View, &[plain(Key::Plus), shift(Key::Plus), plain(Key::Equals), shift(Key::Equals)], "Zooms in around the pointer, or the playhead. With the clip editor's keys live, zooms its piano roll."),
    info(Command::ZoomOut, "view.zoom_out", "Zoom out", Area::View, &[plain(Key::Minus)], "Zooms out around the pointer, or the playhead. With the clip editor's keys live, zooms its piano roll."),
    info(Command::ZoomBack, "view.zoom_back", "Zoom back", Area::View, &[plain(Key::X)], "Goes back to the view before the last zoom."),
    info(Command::FitWidth, "view.fit_width", "Fit song to width", Area::View, &[plain(Key::W)], "Fits the whole song to the window's width."),
    info(Command::FitHeight, "view.fit_height", "Fit tracks to height", Area::View, &[plain(Key::H)], "Fits every track into the window's height, or back."),
    info(Command::ZoomToSelection, "view.zoom_selection", "Zoom to selection", Area::View, &[plain(Key::Z)], "Fits the selected clips or time range to the window."),
    info(Command::FitLoop, "view.fit_loop", "Fit loop", Area::View, &[], "Fits the loop to the window."),
    info(Command::ToggleDetail, "view.detail_panel", "Show / hide detail panel", Area::View, &[shift(Key::Tab)], "Shows or hides the detail panel."),
    info(Command::ToggleInfoLine, "view.info_line", "Show info line", Area::View, &[], "Shows or hides the line along the bottom that says what is under the pointer."),
    info(Command::OpenPalette, "view.palette", "Command palette", Area::View, &[ctrl(Key::K)], "Finds any command by name, and runs it."),
    info(Command::NextPane, "view.next_pane", "Next pane", Area::View, &[plain(Key::F6)], "Moves the keyboard to the next pane: browser, arrangement, detail."),
    info(Command::PreviousPane, "view.previous_pane", "Previous pane", Area::View, &[shift(Key::F6)], "Moves the keyboard to the previous pane."),
    info(Command::GridNarrower, "grid.narrower", "Grid narrower", Area::Grid, &[ctrl(Key::Num1)], "Makes the grid finer."),
    info(Command::GridWider, "grid.wider", "Grid wider", Area::Grid, &[ctrl(Key::Num2)], "Makes the grid coarser."),
    info(Command::TripletGrid, "grid.triplet", "Triplet grid", Area::Grid, &[ctrl(Key::Num3)], "Switches the grid to triplets, or back."),
    info(Command::SnapToGrid, "grid.snap", "Snap to grid", Area::Grid, &[ctrl(Key::Num4)], "Turns snapping to the grid on or off."),
    info(Command::FixedGrid, "grid.fixed", "Fixed grid", Area::Grid, &[ctrl(Key::Num5)], "Keeps the grid at its size whatever the zoom, or lets it adapt."),
    info(Command::SetLocator, "locators.set", "Set locator", Area::Locators, &[plain(Key::M)], "Adds a locator at the playhead, or removes the one there."),
    info(Command::PreviousLocator, "locators.previous", "Previous locator", Area::Locators, &[ctrl(Key::ArrowLeft)], "Jumps to the previous locator."),
    info(Command::NextLocator, "locators.next", "Next locator", Area::Locators, &[ctrl(Key::ArrowRight)], "Jumps to the next locator."),
    info(Command::NewProject, "file.new", "New project", Area::File, &[ctrl(Key::N)], "Starts a new project."),
    info(Command::OpenProject, "file.open", "Open…", Area::File, &[ctrl(Key::O)], "Opens a project."),
    info(Command::Save, "file.save", "Save", Area::File, &[ctrl(Key::S)], "Saves the project."),
    info(Command::SaveAs, "file.save_as", "Save as…", Area::File, &[ctrl_shift(Key::S)], "Saves the project under a new name or place."),
    // The id stays `file.import_audio` so shortcuts saved before MIDI files (M5 slice 7) keep working.
    info(Command::Import, "file.import_audio", "Import audio or MIDI…", Area::File, &[ctrl_shift(Key::I)], "Adds audio or MIDI files at the playhead: onto the selected track when it takes them (audio on audio tracks, MIDI on MIDI tracks), or onto new tracks after the selection."),
    info(Command::ExportMidiFile, "file.export_midi", "Export MIDI file…", Area::File, &[], "Writes the project's MIDI tracks to a Standard MIDI File: a track for each, with the tempo and time signature."),
    info(Command::OpenShortcuts, "file.shortcuts", "Keyboard shortcuts", Area::File, &[], "Shows every shortcut on a keyboard, and changes them."),
    info(Command::CreateMidiClip, "midi.create_clip", "Create MIDI clip", Area::Midi, &[ctrl_shift(Key::M)], "Puts an empty MIDI clip on the first selected MIDI track, else the first MIDI track of the time range, else the track last clicked. Over the time range if there is one, else one bar at the edit position. Opens it in the clip editor."),
    info(Command::ExportMidiClip, "midi.export_clip", "Export MIDI clip…", Area::Midi, &[], "Writes the selected MIDI clips, as they play, to a Standard MIDI File."),
    info(Command::OpenClipEditor, "midi.open_editor", "Edit MIDI clip", Area::Midi, &[plain(Key::Enter)], "Opens the selected MIDI clip in the clip editor, in the detail panel."),
    info(Command::PopOutEditor, "midi.pop_out_editor", "Pop out clip editor", Area::Midi, &[ctrl_shift(Key::E)], "Moves the clip editor into its own window, or docks it back into the detail panel."),
    info(Command::DrawTool, "midi.draw_tool", "Draw tool", Area::Midi, &[plain(Key::B)], "Switches the clip editor between the pointer and the draw tool: click to add a note, drag to set its length, click a note to remove it."),
    info(Command::SplitNotes, "midi.split_notes", "Split notes", Area::Midi, &[ctrl(Key::E)], "Cuts the selected notes in two at the playhead. If the playhead is outside the clip, cuts at the pointer over the clip editor instead."),
    info(Command::GlueNotes, "midi.glue_notes", "Glue notes", Area::Midi, &[ctrl(Key::J)], "Joins selected notes of one pitch that touch or overlap into one."),
    info(Command::LegatoNotes, "midi.legato", "Legato", Area::Midi, &[plain(Key::L)], "Lengthens each selected note to the start of the next one."),
    info(Command::TransposeUp, "midi.transpose_up", "Transpose up", Area::Midi, &[], "Moves the selected notes up a semitone. In the clip editor the Up arrow does it."),
    info(Command::TransposeDown, "midi.transpose_down", "Transpose down", Area::Midi, &[], "Moves the selected notes down a semitone. In the clip editor the Down arrow does it."),
    info(Command::OctaveUp, "midi.octave_up", "Transpose up an octave", Area::Midi, &[], "Moves the selected notes up an octave. In the clip editor Shift+Up does it."),
    info(Command::OctaveDown, "midi.octave_down", "Transpose down an octave", Area::Midi, &[], "Moves the selected notes down an octave. In the clip editor Shift+Down does it."),
    info(Command::NudgeEarlier, "midi.nudge_earlier", "Nudge notes earlier", Area::Midi, &[], "Moves the selected notes one grid step earlier. In the clip editor the Left arrow does it."),
    info(Command::NudgeLater, "midi.nudge_later", "Nudge notes later", Area::Midi, &[], "Moves the selected notes one grid step later. In the clip editor the Right arrow does it."),
    info(Command::NudgeEarlierFine, "midi.nudge_earlier_fine", "Nudge notes earlier, finely", Area::Midi, &[alt(Key::ArrowLeft)], "Moves the selected notes earlier by the shortest note, a 128th."),
    info(Command::NudgeLaterFine, "midi.nudge_later_fine", "Nudge notes later, finely", Area::Midi, &[alt(Key::ArrowRight)], "Moves the selected notes later by the shortest note, a 128th."),
    info(Command::ToggleLane, "midi.toggle_lane", "Velocity and controller lane", Area::Midi, &[], "Shows or hides the lane under the notes in the clip editor, where velocities and controllers are drawn."),
    info(Command::LaneVelocity, "midi.lane_velocity", "Velocity lane", Area::Midi, &[], "Shows the notes' velocities in the lane under the clip editor."),
    info(Command::LaneModWheel, "midi.lane_mod_wheel", "Mod wheel lane", Area::Midi, &[], "Shows the mod wheel (controller 1) in the lane under the clip editor."),
    info(Command::LanePitchBend, "midi.lane_pitch_bend", "Pitch bend lane", Area::Midi, &[], "Shows pitch bend in the lane under the clip editor."),
    info(Command::LanePressure, "midi.lane_pressure", "Pressure lane", Area::Midi, &[], "Shows channel pressure (aftertouch) in the lane under the clip editor."),
    info(Command::LanePencil, "midi.lane_pencil", "Lane pencil", Area::Midi, &[plain(Key::Num1)], "Draws values freehand in the lane. A click sets one value."),
    info(Command::LaneLine, "midi.lane_line", "Lane line", Area::Midi, &[plain(Key::Num2)], "Draws a straight line between two values in the lane."),
    info(Command::LaneCurve, "midi.lane_curve", "Lane curve", Area::Midi, &[plain(Key::Num3)], "Draws a line in the lane, then drag its dot to bend it."),
    info(Command::RaiseVelocity, "midi.raise_velocity", "Raise velocity", Area::Midi, &[], "Raises the velocity of the selected notes by 8."),
    info(Command::LowerVelocity, "midi.lower_velocity", "Lower velocity", Area::Midi, &[], "Lowers the velocity of the selected notes by 8."),
    info(Command::Quantize, "midi.quantize", "Quantize", Area::Midi, &[plain(Key::Q)], "Moves the selected notes (every note if none are selected) toward the grid with the last quantize settings."),
    info(Command::QuantizeOptions, "midi.quantize_options", "Quantize options", Area::Midi, &[shift(Key::Q)], "Opens the quantize settings, and shows what they will do before you apply them."),
    info(Command::HumanizeNotes, "midi.humanize", "Humanize notes", Area::Midi, &[], "Loosens the timing and velocity of the selected notes (every note if none are selected) a little, at random."),
    info(Command::FixedLengthNotes, "midi.fixed_length", "Fixed length", Area::Midi, &[], "Gives the selected notes (every note if none are selected) the length of one grid step."),
    info(Command::ScaleConstrainNotes, "midi.scale_constrain", "Constrain to scale", Area::Midi, &[], "Moves the selected notes (every note if none are selected) onto the last scale chosen from the editor's menu, C major to begin with."),
    info(Command::ReverseNotes, "midi.reverse", "Reverse notes", Area::Midi, &[], "Plays the selected notes (every note if none are selected) backwards: the last becomes the first."),
    info(Command::DevicesSearch, "devices.search", "Search devices", Area::Devices, &[ctrl(Key::F)], "Opens the browser and puts the keyboard in its search field."),
    info(Command::DevicesRescan, "devices.rescan", "Rescan plugins", Area::Devices, &[], "Looks for new and changed plugins in the plugin folders. Plugins that did not change are not loaded again."),
    info(Command::DevicesRescanAll, "devices.rescan_all", "Rescan all plugins", Area::Devices, &[], "Loads every plugin again, ignoring what was learned before, blocked ones included."),
    info(Command::DevicesAddFolder, "devices.add_folder", "Add plugin folder…", Area::Devices, &[], "Adds a folder to search for plugins, besides the standard ones, and scans it."),
    info(Command::DevicesFavorite, "devices.favorite", "Favorite device", Area::Devices, &[plain(Key::F)], "Stars or unstars the device selected in the browser."),
    info(Command::DevicesScanAtStartup, "devices.scan_at_startup", "Scan plugins at startup", Area::Devices, &[], "Looks for new and changed plugins in the background each time the app starts. Off, the list changes only when you rescan."),
    info(Command::DevicesAdd, "devices.add", "Add device to selected track", Area::Devices, &[], "Puts the device chosen in the browser on the selected track (after the chosen card), or on a new track when none fits."),
    info(Command::DeviceRemove, "devices.remove", "Remove device", Area::Devices, &[], "Takes the chosen device off its track. Undo puts it back with its settings."),
    info(Command::DeviceBypass, "devices.bypass", "Bypass device", Area::Devices, &[], "Turns the chosen device off, or on again. A bypassed device keeps its delay, so nothing shifts in time."),
    info(Command::DeviceMoveEarlier, "devices.move_earlier", "Move device earlier", Area::Devices, &[], "Moves the chosen device one place toward the start of its chain. Alt+Left does the same while the Devices tab has the keyboard."),
    info(Command::DeviceMoveLater, "devices.move_later", "Move device later", Area::Devices, &[], "Moves the chosen device one place toward the end of its chain. Alt+Right does the same while the Devices tab has the keyboard."),
    info(Command::DeviceCopy, "devices.copy", "Copy device", Area::Devices, &[], "Keeps the chosen device with its settings, to paste on any track. Ctrl+C does the same while the Devices tab has the keyboard."),
    info(Command::DevicePaste, "devices.paste", "Paste device", Area::Devices, &[], "Puts the copied device on the shown track, after the chosen card. It is a new device with the copied settings. Ctrl+V does the same while the Devices tab has the keyboard."),
    info(Command::DeviceDuplicate, "devices.duplicate", "Duplicate device", Area::Devices, &[], "Copies the chosen device right after itself. Ctrl+D does the same while the Devices tab has the keyboard."),
    info(Command::DeviceFold, "devices.fold", "Fold device", Area::Devices, &[], "Folds the chosen card to a narrow strip, or opens it again. Double-clicking a card does the same."),
    info(Command::DeviceParameters, "devices.parameters", "Device parameters", Area::Devices, &[], "Opens every parameter of the chosen plugin, with search. A star puts a parameter on the card's strip."),
    info(Command::DeviceEditor, "devices.editor", "Open or close the plugin's own window", Area::Devices, &[], "Opens the chosen plugin's own window where it was last, or closes it. The window belongs to the plugin; a plugin that stops answering can be ended from its card."),
    info(Command::DeviceLearn, "devices.learn", "Learn a parameter from the plugin's window", Area::Devices, &[], "Opens the plugin's window and puts the next control you touch there on the next free slot of the card's strip. Run it again, or press Esc, to stop."),
    info(Command::LeaveSafeMode, "devices.leave_safe_mode", "Leave safe mode", Area::Devices, &[], "Ends safe mode: the plugins start loading, each as it would have, and the app looks for new ones as set in the settings. A plugin that stopped the app loading last time may do so again."),
    info(Command::DevicePreset, "devices.preset", "Choose a preset for the plugin", Area::Devices, &[], "Opens the chosen plugin's preset menu: its own factory programs and the preset files saved for it, with a search. The arrows beside the name step to the previous and next."),
    info(Command::DeviceSavePreset, "devices.save_preset", "Save the plugin's sound as a preset", Area::Devices, &[], "Saves the chosen plugin's sound as a preset file. A sound that came from one of your own preset files is saved over it; otherwise it asks for a name and never writes over a file."),
    info(Command::DeviceSidechain, "devices.sidechain", "Choose the sidechain of the effect", Area::Devices, &[], "Opens the chosen plugin effect's sidechain menu: which track keys it, and where on that track the signal is taken (before its devices, after them, or what it plays). Only an effect with a sidechain input has one."),
    info(Command::DeviceOutputs, "devices.outputs", "Choose the outputs of the instrument", Area::Devices, &[], "Opens the chosen plugin instrument's outputs menu: a switch for each output besides the main one. Turning one on makes an audio track for it under the instrument's track; turning it off takes the track away. Only an instrument with several outputs has one."),
    info(Command::AutomationShow, "view.automation", "Show or hide automation lanes", Area::View, &[plain(Key::A)], "Shows the automation lane over the selected tracks' clips, or hides it. The lane draws one parameter of the track, volume to begin with; click it to add points."),
    info(Command::AutomationChoose, "view.automation_parameter", "Choose the automation parameter", Area::View, &[], "Opens the list of parameters the shown lane can draw: volume, pan, trim, mute and the track's sends."),
    info(Command::AutomationAddPoint, "edit.automation_point", "Add an automation point at the playhead", Area::Edit, &[], "Adds a point to the shown lane at the playhead, holding the value the lane has there. With the point picked, the arrows move it and Delete takes it away."),
    info(Command::AutomationToggleLane, "view.automation_on", "Turn the automation lane on or off", Area::View, &[], "Switches the shown lane on or off, keeping its points. A lane that is off plays nothing and the control keeps the value it was set to."),
    info(Command::AutomationRemoveLane, "edit.automation_remove", "Remove the automation lane", Area::Edit, &[], "Takes the shown lane away, points and all."),
    info(Command::AutomationSegmentLinear, "edit.automation_segment_linear", "Make the automation segments straight", Area::Edit, &[], "The segments leaving the picked automation points run in a straight line to the next point."),
    info(Command::AutomationSegmentStep, "edit.automation_segment_step", "Make the automation segments steps", Area::Edit, &[], "The segments leaving the picked automation points hold their value until the next point, then jump."),
    info(Command::AutomationSegmentCurve, "edit.automation_segment_curve", "Make the automation segments curved", Area::Edit, &[], "The segments leaving the picked automation points follow a smooth S curve. Pick one point and drag the dot on its segment to bend it."),
    info(Command::AutomationToolRamp, "edit.automation_tool_ramp", "Automation tool: ramp", Area::Edit, &[], "Drag across the shown lane to draw a straight ramp from where you press to where you let go."),
    info(Command::AutomationToolSine, "edit.automation_tool_sine", "Automation tool: sine", Area::Edit, &[], "Drag across the shown lane to draw a sine wave, one cycle to a grid step, between the height you press at and the height you let go at."),
    info(Command::AutomationToolTriangle, "edit.automation_tool_triangle", "Automation tool: triangle", Area::Edit, &[], "Drag across the shown lane to draw a triangle wave, one cycle to a grid step."),
    info(Command::AutomationToolSaw, "edit.automation_tool_saw", "Automation tool: saw", Area::Edit, &[], "Drag across the shown lane to draw a saw wave that rises over each grid step and drops at its end."),
    info(Command::AutomationToolSquare, "edit.automation_tool_square", "Automation tool: square", Area::Edit, &[], "Drag across the shown lane to draw a square wave, one cycle to a grid step, as steps."),
];

const fn info(
    command: Command,
    id: &'static str,
    name: &'static str,
    area: Area,
    keys: &'static [Chord],
    about: &'static str,
) -> Info {
    Info {
        command,
        id,
        name,
        area,
        keys,
        about,
    }
}

/// The command with saved id `id`.
pub(crate) fn by_id(id: &str) -> Option<Command> {
    ALL.iter()
        .find(|info| info.id == id)
        .map(|info| info.command)
}

impl Command {
    pub(crate) fn info(self) -> &'static Info {
        &ALL[self as usize]
    }

    pub(crate) fn name(self) -> &'static str {
        self.info().name
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn the_table_is_in_command_order() {
        for (i, info) in ALL.iter().enumerate() {
            assert_eq!(info.command as usize, i, "{} is out of place", info.id);
        }
        assert_eq!(
            Command::AutomationToolSquare as usize + 1,
            ALL.len(),
            "a command is missing"
        );
    }

    #[test]
    fn ids_and_names_are_unique_and_ids_are_area_dot_name() {
        let ids: HashSet<_> = ALL.iter().map(|i| i.id).collect();
        let names: HashSet<_> = ALL.iter().map(|i| i.name).collect();
        assert_eq!(ids.len(), ALL.len());
        assert_eq!(names.len(), ALL.len());
        for info in ALL {
            let (area, rest) = info.id.split_once('.').expect("area.name");
            assert_eq!(area, info.area.name().to_lowercase(), "{}", info.id);
            assert!(
                rest.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{}",
                info.id
            );
        }
    }
}
