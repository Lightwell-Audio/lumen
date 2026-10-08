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
    ToggleFollow,
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
    Rename,
    MuteClips,
    ClipProperties,
    AddAudioTrack,
    AddMidiTrack,
    MoveTracksUp,
    MoveTracksDown,
    SelectTrackAbove,
    SelectTrackBelow,
    ExtendSelectionUp,
    ExtendSelectionDown,
    FoldTracks,
    UnfoldTracks,
    ZoomIn,
    ZoomOut,
    ZoomBack,
    FitWidth,
    FitHeight,
    ToggleDetail,
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
}

/// Where a command belongs: how the palette and the shortcut list group
/// it, and the colour of its light on a keycap (the owner's K5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Area {
    Transport,
    Edit,
    Clip,
    Track,
    View,
    Grid,
    Locators,
    File,
}

impl Area {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the palette groups by area (slice 7b)")
    )]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Area::Transport => "Transport",
            Area::Edit => "Edit",
            Area::Clip => "Clip",
            Area::Track => "Track",
            Area::View => "View",
            Area::Grid => "Grid",
            Area::Locators => "Locators",
            Area::File => "File",
        }
    }
}

/// What the table knows about a command.
#[derive(Debug)]
pub(crate) struct Info {
    pub command: Command,
    /// Stable, for saved keymaps: never renamed once shipped.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "saved keymaps name commands by id (slice 7c)")
    )]
    pub id: &'static str,
    pub name: &'static str,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the palette groups by area (slice 7b)")
    )]
    pub area: Area,
    /// The default shortcuts, Ableton's where it has one; the first is the
    /// one shown.
    pub keys: &'static [Chord],
}

use eframe::egui::Key;
use keys::{alt, ctrl, ctrl_shift, plain, shift};

/// Every command, in the order of [`Command`]. Kept as a table, one line
/// per command, so it reads as one.
#[rustfmt::skip]
pub(crate) const ALL: &[Info] = &[
    info(Command::PlayStop, "transport.play_stop", "Play / Stop", Area::Transport, &[plain(Key::Space)]),
    info(Command::PauseContinue, "transport.pause_continue", "Stop here / Continue", Area::Transport, &[shift(Key::Space)]),
    info(Command::GoToStart, "transport.go_to_start", "Go to start", Area::Transport, &[plain(Key::Home)]),
    info(Command::ToggleFollow, "transport.follow", "Follow playhead", Area::Transport, &[ctrl_shift(Key::F)]),
    info(Command::Undo, "edit.undo", "Undo", Area::Edit, &[ctrl(Key::Z)]),
    info(Command::Redo, "edit.redo", "Redo", Area::Edit, &[ctrl_shift(Key::Z), ctrl(Key::Y)]),
    info(Command::Cut, "edit.cut", "Cut", Area::Edit, &[ctrl(Key::X)]),
    info(Command::Copy, "edit.copy", "Copy", Area::Edit, &[ctrl(Key::C)]),
    info(Command::Paste, "edit.paste", "Paste", Area::Edit, &[ctrl(Key::V)]),
    info(Command::Duplicate, "edit.duplicate", "Duplicate", Area::Edit, &[ctrl(Key::D)]),
    info(Command::Delete, "edit.delete", "Delete", Area::Edit, &[plain(Key::Delete), plain(Key::Backspace)]),
    info(Command::DeleteTime, "edit.delete_time", "Delete time", Area::Edit, &[ctrl_shift(Key::Delete)]),
    info(Command::InsertTime, "edit.insert_time", "Insert time", Area::Edit, &[ctrl(Key::I)]),
    info(Command::Deselect, "edit.deselect", "Deselect", Area::Edit, &[plain(Key::Escape)]),
    info(Command::Rename, "edit.rename", "Rename", Area::Edit, &[ctrl(Key::R), plain(Key::F2)]),
    info(Command::MuteClips, "clip.mute", "Mute clip", Area::Clip, &[plain(Key::Num0)]),
    info(Command::ClipProperties, "clip.properties", "Clip properties", Area::Clip, &[alt(Key::Enter)]),
    info(Command::AddAudioTrack, "track.add_audio", "Add audio track", Area::Track, &[ctrl(Key::T)]),
    info(Command::AddMidiTrack, "track.add_midi", "Add MIDI track", Area::Track, &[ctrl_shift(Key::T)]),
    info(Command::MoveTracksUp, "track.move_up", "Move track up", Area::Track, &[alt(Key::ArrowUp)]),
    info(Command::MoveTracksDown, "track.move_down", "Move track down", Area::Track, &[alt(Key::ArrowDown)]),
    info(Command::SelectTrackAbove, "track.select_above", "Select track above", Area::Track, &[plain(Key::ArrowUp)]),
    info(Command::SelectTrackBelow, "track.select_below", "Select track below", Area::Track, &[plain(Key::ArrowDown)]),
    info(Command::ExtendSelectionUp, "track.extend_up", "Add track above to selection", Area::Track, &[shift(Key::ArrowUp)]),
    info(Command::ExtendSelectionDown, "track.extend_down", "Add track below to selection", Area::Track, &[shift(Key::ArrowDown)]),
    info(Command::FoldTracks, "track.fold", "Fold track", Area::Track, &[plain(Key::ArrowLeft)]),
    info(Command::UnfoldTracks, "track.unfold", "Unfold track", Area::Track, &[plain(Key::ArrowRight)]),
    info(Command::ZoomIn, "view.zoom_in", "Zoom in", Area::View, &[plain(Key::Plus), shift(Key::Plus), plain(Key::Equals), shift(Key::Equals)]),
    info(Command::ZoomOut, "view.zoom_out", "Zoom out", Area::View, &[plain(Key::Minus)]),
    info(Command::ZoomBack, "view.zoom_back", "Zoom back", Area::View, &[plain(Key::X)]),
    info(Command::FitWidth, "view.fit_width", "Fit song to width", Area::View, &[plain(Key::W)]),
    info(Command::FitHeight, "view.fit_height", "Fit tracks to height", Area::View, &[plain(Key::H)]),
    info(Command::ToggleDetail, "view.detail_panel", "Show / hide detail panel", Area::View, &[shift(Key::Tab)]),
    info(Command::GridNarrower, "grid.narrower", "Grid narrower", Area::Grid, &[ctrl(Key::Num1)]),
    info(Command::GridWider, "grid.wider", "Grid wider", Area::Grid, &[ctrl(Key::Num2)]),
    info(Command::TripletGrid, "grid.triplet", "Triplet grid", Area::Grid, &[ctrl(Key::Num3)]),
    info(Command::SnapToGrid, "grid.snap", "Snap to grid", Area::Grid, &[ctrl(Key::Num4)]),
    info(Command::FixedGrid, "grid.fixed", "Fixed grid", Area::Grid, &[ctrl(Key::Num5)]),
    info(Command::SetLocator, "locators.set", "Set locator", Area::Locators, &[plain(Key::M)]),
    info(Command::PreviousLocator, "locators.previous", "Previous locator", Area::Locators, &[ctrl(Key::ArrowLeft)]),
    info(Command::NextLocator, "locators.next", "Next locator", Area::Locators, &[ctrl(Key::ArrowRight)]),
    info(Command::NewProject, "file.new", "New project", Area::File, &[ctrl(Key::N)]),
    info(Command::OpenProject, "file.open", "Open…", Area::File, &[ctrl(Key::O)]),
    info(Command::Save, "file.save", "Save", Area::File, &[ctrl(Key::S)]),
    info(Command::SaveAs, "file.save_as", "Save as…", Area::File, &[ctrl_shift(Key::S)]),
];

const fn info(
    command: Command,
    id: &'static str,
    name: &'static str,
    area: Area,
    keys: &'static [Chord],
) -> Info {
    Info {
        command,
        id,
        name,
        area,
        keys,
    }
}

impl Command {
    pub(crate) fn info(self) -> &'static Info {
        &ALL[self as usize]
    }

    pub(crate) fn name(self) -> &'static str {
        self.info().name
    }

    /// Keys a focused control uses itself (a slider's arrows): with a
    /// control focused, these go to it rather than run the command.
    pub(crate) fn yields_to_focus(self) -> bool {
        matches!(
            self,
            Command::SelectTrackAbove
                | Command::SelectTrackBelow
                | Command::ExtendSelectionUp
                | Command::ExtendSelectionDown
                | Command::FoldTracks
                | Command::UnfoldTracks
        )
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
            Command::SaveAs as usize + 1,
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
