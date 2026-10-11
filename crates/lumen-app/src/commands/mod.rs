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
    ImportAudio,
    OpenShortcuts,
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
    /// The default shortcuts, Ableton's where it has one; the first is the
    /// one shown.
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
    info(Command::ZoomIn, "view.zoom_in", "Zoom in", Area::View, &[plain(Key::Plus), shift(Key::Plus), plain(Key::Equals), shift(Key::Equals)], "Zooms in around the pointer, or the playhead."),
    info(Command::ZoomOut, "view.zoom_out", "Zoom out", Area::View, &[plain(Key::Minus)], "Zooms out around the pointer, or the playhead."),
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
    info(Command::ImportAudio, "file.import_audio", "Import audio…", Area::File, &[ctrl_shift(Key::I)], "Adds audio files onto the selected audio track at the playhead, or onto new tracks after the selection."),
    info(Command::OpenShortcuts, "file.shortcuts", "Keyboard shortcuts", Area::File, &[], "Shows every shortcut on a keyboard, and changes them."),
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
            Command::OpenShortcuts as usize + 1,
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
