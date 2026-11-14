//! The drum map editor (M8 slice D1): the window that shows a kit and lets
//! every field of every sound be edited where it is, plus the map itself:
//! its name, which map a track reads (a kit, a map the project holds or one
//! saved in the user's folder), Make unique, and the files.
//!
//! The window is a `widgets::Dialog` like the Mappings window, opened from a
//! MIDI track's menu, the palette or the Attach command (which opens it).
//! It edits the map through the `Document`'s named edits (one undo step
//! each) and never touches the project itself: [`table`] draws the rows and
//! says what the person did, and this module applies it.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use eframe::egui::{self, Align2, RichText, Sense, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_project::drums::presets::Preset;
use lumen_project::model::{DrumMap, Id, Sound};

use crate::app::projects::FileDialog;
use crate::widgets::{self, Dialog, Menu, raised};
use crate::{App, theme};

pub(crate) mod files;
pub(crate) mod table;

/// The window's width: the table's columns and the map's controls. It fits
/// the shortest window the app opens at (960 pt).
pub(crate) const WIDTH: f32 = 780.0;
/// How tall the table scrolls before it scrolls.
const TABLE_HEIGHT: f32 = 360.0;

/// What the table's rows did, or one of the window's own controls: the
/// window applies each as one `Document` edit.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    RenameMap(String),
    Rename {
        input: u8,
        name: String,
    },
    Input {
        input: u8,
        to: u8,
    },
    Output {
        input: u8,
        to: u8,
    },
    Channel {
        input: u8,
        channel: Option<u8>,
    },
    Mute {
        input: u8,
        on: bool,
    },
    Solo {
        input: u8,
        on: bool,
    },
    Quantize {
        input: u8,
        quantize: Option<lumen_project::model::Quantize>,
    },
    Length {
        input: u8,
        length: Option<lumen_core::time::Ticks>,
    },
    Choke {
        input: u8,
        group: Option<u8>,
    },
    Colour {
        input: u8,
        color: lumen_project::model::Color,
    },
    Icon {
        input: u8,
        icon: lumen_project::model::DrumIcon,
    },
    /// The next note played becomes this sound's input note.
    Learn {
        input: u8,
    },
    Add,
    Remove {
        input: u8,
    },
    Move {
        from: usize,
        to: usize,
    },
    /// The map the window should show: a kit's copy, a map the project
    /// holds, or one of the user's files.
    Choose(Choice),
    /// Give the track its own copy of the map it names.
    Unique,
    /// Save the map in the user's folder, export it, or import one.
    SaveUser,
    Export,
    Import,
}

/// A map to switch the track to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Choice {
    Preset(Preset),
    Project(Id),
    User(PathBuf),
}

/// What a file task found, for the window.
enum Done {
    Listed(Vec<files::Saved>),
    Saved(Result<PathBuf, String>),
    /// A map written where the person chose (an export).
    Exported(Result<PathBuf, String>),
    Read {
        path: PathBuf,
        map: Result<DrumMap, String>,
    },
}

/// A file task running on a thread of its own (a read or a write: never a
/// frame). The user's folder is small, so a worker's answer is a frame or
/// two away.
struct Job {
    rx: Receiver<Done>,
}

/// A message under the table: what a file did, or why it did not. It is read
/// in full (the dialog wraps it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Notice {
    pub words: String,
    /// A failure: drawn in the warning colour.
    pub bad: bool,
}

impl Notice {
    fn good(words: impl Into<String>) -> Self {
        Self {
            words: words.into(),
            bad: false,
        }
    }

    fn bad(words: impl Into<String>) -> Self {
        Self {
            words: words.into(),
            bad: true,
        }
    }
}

/// The editor's state: which map it shows and for which tracks, the note it
/// waits for, and the file work in flight.
#[derive(Default)]
pub(crate) struct State {
    open: Option<Open>,
}

struct Open {
    /// The map the window edits (none until the track has one).
    map: Option<Id>,
    /// The MIDI tracks the window was opened for: a chosen map goes to them.
    tracks: Vec<Id>,
    /// The keyboard starts in the first row (the window just opened).
    fresh: bool,
    /// The user's saved maps, read when the window opens.
    users: Vec<files::Saved>,
    /// A file task, while one runs.
    job: Option<Job>,
    /// What the last file did, or why it could not.
    notice: Option<Notice>,
    /// The sound waiting for a note, by its input note.
    learn: Option<u8>,
    /// The map the export dialog is for, kept while the dialog is open.
    pending_export: Option<DrumMap>,
}

impl State {
    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The map the window edits.
    pub(crate) fn map(&self) -> Option<Id> {
        self.open.as_ref().and_then(|open| open.map)
    }

    /// The sound the editor waits for a note on: its map and input note.
    pub(crate) fn learning(&self) -> Option<(Id, u8)> {
        let open = self.open.as_ref()?;
        Some((open.map?, open.learn?))
    }

    pub(crate) fn notice(&self) -> Option<&Notice> {
        self.open.as_ref().and_then(|open| open.notice.as_ref())
    }
}

impl App {
    /// Opens the editor for the selected MIDI tracks: the map the first one
    /// reads, or the list of maps to give it when it has none.
    pub(crate) fn open_drum_map_editor(&mut self) {
        let tracks = self.selected_midi_tracks();
        if tracks.is_empty() {
            self.say("Select a MIDI track first".to_owned());
            return;
        }
        let map = tracks
            .iter()
            .find_map(|track| self.document.drum_map_of(*track).map(|m| m.id));
        self.show_drum_map_editor(map, tracks);
    }

    /// Opens the editor on the selected track's map, or on the window's when
    /// it is open already: what the map's file commands act on.
    pub(crate) fn drum_map_for_command(&mut self) -> Option<Id> {
        if let Some(map) = self.drum_map.map() {
            return Some(map);
        }
        let track = self.selected_midi_tracks().first().copied()?;
        let map = self.document.drum_map_of(track).map(|m| m.id)?;
        self.show_drum_map_editor(Some(map), vec![track]);
        Some(map)
    }

    fn show_drum_map_editor(&mut self, map: Option<Id>, tracks: Vec<Id>) {
        let mut open = Open {
            map,
            tracks,
            fresh: true,
            users: Vec::new(),
            job: None,
            notice: None,
            learn: None,
            pending_export: None,
        };
        open.job = self.drum_map_listing();
        self.drum_map.open = Some(open);
    }

    /// Reads the user's saved maps on a worker: the list offers them.
    fn drum_map_listing(&self) -> Option<Job> {
        let folder = self.drum_maps_folder()?;
        Some(spawn(move || Done::Listed(files::list(&folder))))
    }

    /// The folder the user's saved maps live in (`<settings folder>/Drum
    /// Maps`).
    pub(crate) fn drum_maps_folder(&self) -> Option<PathBuf> {
        files::user_folder(self.settings_path.as_deref())
    }

    /// A note was played on an input: the sound the editor waits for takes
    /// it as its input note (the row's Learn button).
    pub(crate) fn drum_map_heard(&mut self, key: u8) {
        let Some((map, input)) = self.drum_map.learning() else {
            return;
        };
        if let Some(open) = &mut self.drum_map.open {
            open.learn = None;
        }
        if input == key {
            self.say(format!(
                "That sound already answers to note {key}; play another note"
            ));
            return;
        }
        if self.document.set_sound_input(map, input, key) {
            self.say(format!(
                "The sound now answers to note {key}, one undo step"
            ));
        } else {
            self.say(format!(
                "Another sound already answers to note {key}, so nothing changed"
            ));
        }
    }

    /// The editor, while it is open: the map's controls, the table and the
    /// notice under it.
    pub(crate) fn drum_map_window(&mut self, ctx: &egui::Context) {
        let Some(mut open) = self.drum_map.open.take() else {
            return;
        };
        self.poll_drum_map_files(&mut open, ctx);
        let map = open.map.and_then(|id| self.document.drum_map(id).cloned());
        if open.map.is_some() && map.is_none() {
            // Undone away (an attach or an import taken back): the chooser
            // shows again rather than a map that is not there.
            open.map = None;
        }
        let tracks = self.drum_map_track_names(&open.tracks);
        let title = match &map {
            Some(map) => format!("Drum map \u{00B7} {}", map.name),
            None => "Drum map".to_owned(),
        };
        let mut action = None;
        let close = Dialog::new("drum map", &title, WIDTH)
            .at(
                pos2(ctx.content_rect().center().x, 64.0),
                Align2::CENTER_TOP,
            )
            .show(ctx, |ui| {
                match &map {
                    Some(map) => action = self.drum_map_controls(ui, map, &open, &tracks),
                    None => action = self.drum_map_chooser(ui, &open, &tracks),
                }
                if let Some(notice) = &open.notice {
                    ui.add_space(theme::space::S);
                    let colour = if notice.bad {
                        theme::p().warning
                    } else {
                        theme::p().text_secondary
                    };
                    ui.label(RichText::new(&notice.words).small().color(colour));
                }
            });
        open.fresh = false;
        if close {
            self.drum_map.open = None;
            return;
        }
        if let Some(action) = action {
            self.apply_drum_map_action(action, &mut open, ctx);
        }
        self.drum_map.open = Some(open);
    }

    /// The top controls over the table: the map's name, the list of maps and
    /// the map's actions. Gives what was done.
    fn drum_map_controls(
        &mut self,
        ui: &mut egui::Ui,
        map: &DrumMap,
        open: &Open,
        tracks: &str,
    ) -> Option<Action> {
        let mut action = None;
        ui.label(RichText::new(tracks).small().weak());
        ui.add_space(theme::space::XS);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Name").small().weak());
            let (rect, _) =
                ui.allocate_exact_size(vec2(200.0, theme::size::CONTROL), egui::Sense::hover());
            if let Some(name) = text_field(
                ui,
                ui.id().with("drum map name"),
                rect,
                &map.name,
                "Map name",
            ) && name != map.name
            {
                action = Some(Action::RenameMap(name));
            }
            ui.add_space(theme::space::S);
        });
        ui.add_space(theme::space::XS);
        ui.horizontal_wrapped(|ui| {
            let project: Vec<(Id, String, usize)> = self
                .document
                .project()
                .drum_maps
                .iter()
                .map(|m| (m.id, m.name.clone(), m.sounds.len()))
                .collect();
            let list = map_list(ui, &map.name, &project, &open.users, open.map);
            if let Some(choice) = list {
                action = Some(Action::Choose(choice));
            }
            if raised::button(ui, "Add sound").clicked() {
                action = Some(Action::Add);
            }
            if raised::button(ui, "Make unique")
                .on_hover_text(
                    "Gives this track its own copy of the map, so edits stop reaching the other tracks that share it",
                )
                .clicked()
            {
                action = Some(Action::Unique);
            }
            if raised::button(ui, "Save as user map\u{2026}")
                .on_hover_text(
                    "Keeps the map in your drum maps folder, to attach to any project",
                )
                .clicked()
            {
                action = Some(Action::SaveUser);
            }
            if raised::button(ui, "Import\u{2026}").clicked() {
                action = Some(Action::Import);
            }
            if raised::button(ui, "Export\u{2026}").clicked() {
                action = Some(Action::Export);
            }
        });
        ui.add_space(theme::space::S);
        egui::ScrollArea::vertical()
            .id_salt("drum map rows")
            .max_height(TABLE_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if let Some(done) = table::table(
                    ui,
                    table::Input {
                        sounds: &map.sounds,
                        learning: open.learn,
                        focus_first: open.fresh,
                    },
                ) {
                    action = Some(done);
                }
            });
        ui.add_space(theme::space::XS);
        ui.label(RichText::new(table::hint()).small().weak());
        action
    }

    /// The list of maps, when the track has none yet: the kits, the
    /// project's maps and the user's files.
    fn drum_map_chooser(&mut self, ui: &mut egui::Ui, open: &Open, tracks: &str) -> Option<Action> {
        let mut action = None;
        ui.label(
            RichText::new(format!(
                "{tracks} reads no drum map yet. Choose one: the clips keep their notes and play through it."
            ))
            .small()
            .weak(),
        );
        ui.add_space(theme::space::S);
        ui.label(RichText::new("Kits").small().weak());
        for preset in Preset::ALL {
            let label = format!("{} ({} sounds)", preset.name, preset.sounds().len());
            if raised::button(ui, &label).clicked() {
                action = Some(Action::Choose(Choice::Preset(preset)));
            }
        }
        let project: Vec<(Id, String, usize)> = self
            .document
            .project()
            .drum_maps
            .iter()
            .map(|m| (m.id, m.name.clone(), m.sounds.len()))
            .collect();
        if !project.is_empty() {
            ui.add_space(theme::space::S);
            ui.label(RichText::new("This project's maps").small().weak());
            for (id, name, sounds) in project {
                if raised::button(ui, &format!("{name} ({sounds} sounds)")).clicked() {
                    action = Some(Action::Choose(Choice::Project(id)));
                }
            }
        }
        if !open.users.is_empty() {
            ui.add_space(theme::space::S);
            ui.label(RichText::new("Your maps").small().weak());
            for saved in &open.users {
                if raised::button(ui, &saved.name).clicked() {
                    action = Some(Action::Choose(Choice::User(saved.path.clone())));
                }
            }
        }
        ui.add_space(theme::space::S);
        ui.horizontal(|ui| {
            if raised::button(ui, "Import\u{2026}").clicked() {
                action = Some(Action::Import);
            }
            if open.job.is_some() {
                ui.label(RichText::new("Reading your maps\u{2026}").small().weak());
            }
        });
        action
    }

    /// The names of the tracks the window is for: one in quotes, or a count.
    fn drum_map_track_names(&self, tracks: &[Id]) -> String {
        let names: Vec<&str> = tracks
            .iter()
            .filter_map(|id| {
                self.document
                    .project()
                    .tracks
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| t.name.as_str())
            })
            .collect();
        match names.as_slice() {
            [] => "No track".to_owned(),
            [one] => format!("\u{201c}{one}\u{201d}"),
            [first, rest @ ..] => format!("\u{201c}{first}\u{201d} and {} more", rest.len()),
        }
    }

    /// Applies one thing the window's controls did, as one named edit.
    fn apply_drum_map_action(&mut self, action: Action, open: &mut Open, ctx: &egui::Context) {
        let Some(map) = open.map else {
            // The chooser: a choice, an import or a file action.
            self.apply_drum_map_file_action(action, open, ctx);
            return;
        };
        match action {
            Action::RenameMap(name) => {
                self.document.rename_drum_map(map, &name);
            }
            Action::Rename { input, name } => {
                self.document.set_sound_name(map, input, &name);
            }
            Action::Input { input, to } => {
                if !self.document.set_sound_input(map, input, to) {
                    open.notice = Some(Notice::bad(format!(
                        "Another sound already answers to note {to}, so note {input} stays"
                    )));
                }
            }
            Action::Output { input, to } => {
                self.document.set_sound_output(map, input, to);
            }
            Action::Channel { input, channel } => {
                self.document.set_sound_channel(map, input, channel);
            }
            Action::Mute { input, on } => {
                self.document.set_sound_mute(map, input, on);
            }
            Action::Solo { input, on } => {
                self.document.set_sound_solo(map, input, on);
            }
            Action::Quantize { input, quantize } => {
                self.document.set_sound_quantize(map, input, quantize);
            }
            Action::Length { input, length } => {
                self.document.set_sound_length(map, input, length);
            }
            Action::Choke { input, group } => {
                self.document.set_sound_choke(map, input, group);
            }
            Action::Colour { input, color } => {
                self.document.set_sound_color(map, input, color);
            }
            Action::Icon { input, icon } => {
                self.document.set_sound_icon(map, input, icon);
            }
            Action::Learn { input } => {
                open.learn = if open.learn == Some(input) {
                    None
                } else {
                    Some(input)
                };
                open.notice = Some(if open.learn.is_some() {
                    Notice::good(
                        "Waiting for a note: play it on a MIDI input, and it becomes this sound's note",
                    )
                } else {
                    Notice::good("No longer listening for a note")
                });
            }
            Action::Add => {
                if let Some(sound) = self.new_drum_sound(map) {
                    let note = sound.input;
                    self.document.add_drum_sound(map, sound, None);
                    open.notice = Some(Notice::good(format!(
                        "Added a sound on note {note}: give it a name and a note in its row"
                    )));
                } else {
                    open.notice = Some(Notice::bad(
                        "This map has no free note left for another sound",
                    ));
                }
            }
            Action::Remove { input } => {
                self.document.remove_drum_sound(map, input);
                if open.learn == Some(input) {
                    open.learn = None;
                }
            }
            Action::Move { from, to } => {
                self.document.move_drum_sound(map, from, to);
            }
            Action::Choose(choice) => self.choose_drum_map(open, choice),
            Action::Unique => {
                let Some(track) = open.tracks.first().copied() else {
                    return;
                };
                if let Some(copy) = self.document.make_drum_map_unique(track) {
                    open.map = Some(copy);
                    open.notice = Some(Notice::good(
                        "This track has its own copy of the map; the other tracks keep the original",
                    ));
                }
            }
            Action::SaveUser | Action::Export | Action::Import => {
                self.apply_drum_map_file_action(action, open, ctx);
            }
        }
    }

    /// The file actions and the choice of a map (which a track without one
    /// needs first).
    fn apply_drum_map_file_action(&mut self, action: Action, open: &mut Open, ctx: &egui::Context) {
        match action {
            Action::Choose(choice) => self.choose_drum_map(open, choice),
            Action::SaveUser => self.save_user(open),
            Action::Export => self.start_export(open, ctx),
            Action::Import => self.start_import(open, ctx),
            _ => {}
        }
    }

    /// Keeps the map in the user's folder, on a worker.
    fn save_user(&mut self, open: &mut Open) {
        let Some(map) = open.map.and_then(|id| self.document.drum_map(id).cloned()) else {
            return;
        };
        let Some(folder) = self.drum_maps_folder() else {
            open.notice = Some(Notice::bad(format!(
                "There is no folder to keep drum maps in: {} is running without settings of its own",
                lumen_core::product_name!()
            )));
            return;
        };
        open.job = Some(spawn(move || Done::Saved(files::save_user(&map, &folder))));
    }

    /// Asks where to write the map, and writes it when the dialog answers.
    fn start_export(&mut self, open: &mut Open, ctx: &egui::Context) {
        let Some(map) = open.map.and_then(|id| self.document.drum_map(id).cloned()) else {
            return;
        };
        let name = format!(
            "{}.{}",
            files::file_stem(&map.name, "Drum map"),
            files::EXTENSION
        );
        open.pending_export = Some(map);
        self.start_drum_map_dialog(ctx, name, true);
    }

    /// Asks for a file to read, and reads it when the dialog answers.
    fn start_import(&mut self, open: &mut Open, ctx: &egui::Context) {
        open.notice = Some(Notice::good(
            "Choose a .lumendrums file to read into this project",
        ));
        self.start_drum_map_dialog(ctx, String::new(), false);
    }

    /// Switches the window's tracks to `choice`: a kit's copy, a map the
    /// project holds, or a file of the user's (read on a worker).
    fn choose_drum_map(&mut self, open: &mut Open, choice: Choice) {
        match choice {
            Choice::Preset(preset) => {
                if let Some(map) = self
                    .document
                    .attach_drum_map_preset_to(&open.tracks, preset)
                {
                    open.map = Some(map);
                    open.notice = Some(Notice::good(format!(
                        "{} attached: the clips keep their notes and play through it",
                        preset.name
                    )));
                }
            }
            Choice::Project(map) => {
                self.document.attach_drum_map_to(&open.tracks, map);
                open.map = Some(map);
                open.notice = Some(Notice::good(
                    "The track plays that map; the clips keep their notes",
                ));
            }
            Choice::User(path) => {
                open.job = Some(spawn(move || {
                    let read = std::fs::read(&path)
                        .map_err(|e| format!("{} could not be read: {e}", path.display()))
                        .and_then(|bytes| files::decode(&bytes, &files::file_name(&path)));
                    Done::Read { path, map: read }
                }));
            }
        }
    }

    /// Takes the map off the selected MIDI tracks that have one: their notes
    /// are untouched and play as written again.
    pub(crate) fn detach_drum_map_command(&mut self) {
        let tracks = self.selected_midi_tracks();
        let with: Vec<Id> = tracks
            .into_iter()
            .filter(|id| self.document.drum_map_of(*id).is_some())
            .collect();
        if with.is_empty() {
            return;
        }
        self.document.detach_drum_maps(&with);
        if let Some(open) = &mut self.drum_map.open
            && open.tracks.iter().any(|t| with.contains(t))
        {
            open.map = None;
            open.notice = Some(Notice::good(
                "The drum map is off these tracks; their notes play as they are",
            ));
        }
        self.say(if with.len() == 1 {
            "Drum map detached: the notes play as they are".to_owned()
        } else {
            format!(
                "Drum map detached from {} tracks: the notes play as they are",
                with.len()
            )
        });
    }

    /// The map's file commands: save it in the user's folder, export it to a
    /// file the person chooses, or import one. A track without a map gets
    /// the editor's chooser first.
    pub(crate) fn drum_map_save_command(&mut self) {
        if self.drum_map_for_command().is_none() {
            self.say("Select a MIDI track with a drum map first".to_owned());
            return;
        }
        if let Some(mut open) = self.drum_map.open.take() {
            self.save_user(&mut open);
            self.drum_map.open = Some(open);
        }
    }

    /// Make unique: the selected track gets its own copy of its map.
    pub(crate) fn drum_map_unique_command(&mut self) {
        let tracks = self.selected_midi_tracks();
        let Some(track) = tracks
            .iter()
            .copied()
            .find(|id| self.document.drum_map_of(*id).is_some())
        else {
            self.say("Select a MIDI track with a drum map first".to_owned());
            return;
        };
        let map = self.document.drum_map_of(track).map(|m| m.id);
        self.show_drum_map_editor(map, vec![track]);
        if let Some(copy) = self.document.make_drum_map_unique(track)
            && let Some(open) = &mut self.drum_map.open
        {
            open.map = Some(copy);
            open.notice = Some(Notice::good(
                "This track has its own copy of the map; the other tracks keep the original",
            ));
        }
    }

    /// Exports the map the editor shows (or the selected track's) to a file.
    pub(crate) fn drum_map_export_command(&mut self, ctx: &egui::Context) {
        if self.drum_map_for_command().is_none() {
            self.say("Select a MIDI track with a drum map first".to_owned());
            return;
        }
        if let Some(mut open) = self.drum_map.open.take() {
            self.start_export(&mut open, ctx);
            self.drum_map.open = Some(open);
        }
    }

    /// Imports a `.lumendrums` file into the project, and gives it to the
    /// selected tracks.
    pub(crate) fn drum_map_import_command(&mut self, ctx: &egui::Context) {
        if !self.drum_map.is_open() {
            let tracks = self.selected_midi_tracks();
            let map = tracks
                .iter()
                .find_map(|track| self.document.drum_map_of(*track).map(|m| m.id));
            self.show_drum_map_editor(map, tracks);
        }
        if let Some(mut open) = self.drum_map.open.take() {
            self.start_import(&mut open, ctx);
            self.drum_map.open = Some(open);
        }
    }

    /// A new sound for `map`: the first free note from 36 up, named for its
    /// note, drawn as percussion.
    fn new_drum_sound(&self, map: Id) -> Option<Sound> {
        let held = self.document.drum_map(map)?;
        let free = (36..=127).find(|note| held.sound(*note).is_none())?;
        Some(Sound::new(
            format!("Sound {free}"),
            free,
            free,
            lumen_project::model::DrumIcon::Percussion,
        ))
    }

    /// Polls the file worker; a finished task becomes a notice, and an
    /// imported map joins the project (and the window's tracks).
    fn poll_drum_map_files(&mut self, open: &mut Open, ctx: &egui::Context) {
        let Some(job) = &open.job else {
            return;
        };
        let done = match job.rx.try_recv() {
            Ok(done) => done,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(30));
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                open.job = None;
                open.notice = Some(Notice::bad(
                    "The file work stopped without an answer, so nothing was changed",
                ));
                return;
            }
        };
        open.job = None;
        match done {
            Done::Listed(users) => {
                open.users = users;
            }
            Done::Saved(Ok(path)) => {
                open.notice = Some(Notice::good(format!(
                    "Saved as {} in your drum maps folder",
                    files::file_name(&path)
                )));
                // The list shows it straight away (the folder is the one the
                // file went to).
                if let Some(folder) = path.parent().map(Path::to_path_buf) {
                    open.job = Some(spawn(move || Done::Listed(files::list(&folder))));
                }
            }
            Done::Saved(Err(why)) => {
                open.notice = Some(Notice::bad(why));
            }
            Done::Exported(Ok(path)) => {
                open.notice = Some(Notice::good(format!("Exported to {}", path.display())));
            }
            Done::Exported(Err(why)) => {
                open.notice = Some(Notice::bad(why));
            }
            Done::Read { path, map } => match map {
                Ok(map) => {
                    let name = map.name.clone();
                    let imported = self.document.import_drum_map(map, &open.tracks);
                    open.map = Some(imported);
                    open.notice = Some(Notice::good(format!(
                        "\u{201c}{name}\u{201d} imported from {}; it is part of the project now",
                        files::file_name(&path)
                    )));
                }
                Err(why) => {
                    open.notice = Some(Notice::bad(why));
                }
            },
        }
    }

    /// The export dialog answered (or was cancelled): writes the map kept
    /// when the dialog opened.
    pub(crate) fn drum_map_export_chosen(&mut self, path: Option<PathBuf>) {
        let Some(path) = path else {
            return;
        };
        let Some(open) = &mut self.drum_map.open else {
            return;
        };
        let Some(map) = open.pending_export.take() else {
            return;
        };
        open.job = Some(spawn(move || {
            let done = files::encode(&map)
                .and_then(|bytes| {
                    std::fs::write(&path, bytes)
                        .map_err(|e| format!("{} could not be written: {e}", path.display()))
                })
                .map(|()| path.clone());
            Done::Exported(done)
        }));
    }

    /// The import dialog answered (or was cancelled): reads the file on a
    /// worker.
    pub(crate) fn drum_map_import_chosen(&mut self, path: Option<PathBuf>) {
        let Some(path) = path else {
            return;
        };
        let Some(open) = &mut self.drum_map.open else {
            return;
        };
        open.job = Some(spawn(move || {
            let read = std::fs::read(&path)
                .map_err(|e| format!("{} could not be read: {e}", path.display()))
                .and_then(|bytes| files::decode(&bytes, &files::file_name(&path)));
            Done::Read { path, map: read }
        }));
    }

    /// Starts the system dialog for the map's export (`save` true) or an
    /// import; its answer comes back through `poll_files`.
    fn start_drum_map_dialog(&mut self, ctx: &egui::Context, name: String, save: bool) {
        let folder = self
            .drum_maps_folder()
            .filter(|folder| folder.is_dir())
            .or_else(|| {
                self.document
                    .path
                    .as_ref()
                    .and_then(|path| path.parent().map(Path::to_path_buf))
            });
        self.file_dialog(
            ctx,
            move || {
                let mut dialog = rfd::FileDialog::new()
                    .set_title(if save {
                        "Export the drum map"
                    } else {
                        "Import a drum map"
                    })
                    .add_filter("Drum maps", &[files::EXTENSION]);
                if let Some(folder) = folder {
                    dialog = dialog.set_directory(folder);
                }
                if !name.is_empty() {
                    dialog = dialog.set_file_name(name);
                }
                if save {
                    FileDialog::DrumMapExport(dialog.save_file())
                } else {
                    FileDialog::DrumMapImport(dialog.pick_file())
                }
            },
            move |paths| {
                let path = paths.into_iter().next();
                if save {
                    FileDialog::DrumMapExport(path)
                } else {
                    FileDialog::DrumMapImport(path)
                }
            },
        );
    }
}

/// A text field showing `current`, edited in place: a draft while it is
/// focused, applied when it is left (or Enter, which leaves it), so a rename
/// is one edit. Esc keeps the old text.
pub(crate) fn text_field(
    ui: &mut egui::Ui,
    id: egui::Id,
    rect: egui::Rect,
    current: &str,
    hint: &str,
) -> Option<String> {
    let draft = id.with("draft");
    let edit = id.with("edit");
    let editing: Option<String> = ui.data(|d| d.get_temp(draft));
    let mut text = editing.unwrap_or_else(|| current.to_owned());
    raised::paint_sunken(ui.painter(), rect, theme::radius::for_height(rect.height()));
    let response = ui.put(
        rect.shrink2(vec2(theme::space::XS, 3.0)),
        egui::TextEdit::singleline(&mut text)
            .id(edit)
            .font(egui::FontId::proportional(theme::size::BODY))
            .text_color(theme::p().text)
            .frame(egui::Frame::NONE)
            .margin(vec2(theme::space::XXS, 0.0))
            .hint_text(hint),
    );
    if ui.is_rect_visible(rect) {
        raised::focus_ring(ui.painter(), &response, rect);
    }
    if response.has_focus() {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.data_mut(|d| d.remove::<String>(draft));
            ui.ctx().memory_mut(|m| m.surrender_focus(response.id));
            return None;
        }
        if text != current {
            ui.data_mut(|d| d.insert_temp(draft, text.clone()));
        }
        return None;
    }
    if response.lost_focus() {
        let text = text.trim().to_owned();
        ui.data_mut(|d| d.remove::<String>(draft));
        if !text.is_empty() && text != current {
            return Some(text);
        }
    }
    None
}

/// The map list: a button showing the map's name and, under it, the kits,
/// this project's maps and the user's saved files.
fn map_list(
    ui: &mut egui::Ui,
    showing: &str,
    project: &[(Id, String, usize)],
    users: &[files::Saved],
    current: Option<Id>,
) -> Option<Choice> {
    let width = 200.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, theme::size::CONTROL), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Map: {showing}"))
    });
    let response = response.on_hover_text("The map this track reads its notes through");
    if ui.is_rect_visible(rect) {
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let radius = theme::radius::for_height(rect.height());
        let face = raised::Face::new(theme::controls(false))
            .hover(crate::motion::hover(
                ui.ctx(),
                response.id.with("hover"),
                response.hovered(),
            ))
            .pressed(response.is_pointer_button_down_on());
        let body = raised::paint(ui.painter(), rect, radius, &face);
        let pad = theme::space::S;
        let chevron = 8.0;
        let text = widgets::fit(
            ui.painter(),
            showing,
            egui::FontId::proportional(theme::size::BODY),
            body.width() - 2.0 * pad - chevron - pad,
        );
        let galley = ui.painter().layout_no_wrap(
            text,
            egui::FontId::proportional(theme::size::BODY),
            face.text(),
        );
        let at = egui::Align2::LEFT_CENTER
            .anchor_size(pos2(body.left() + pad, body.center().y), galley.size())
            .min;
        ui.painter().galley(at, galley, face.text());
        widgets::draw_arrow(
            ui.painter(),
            pos2(body.right() - pad - chevron / 2.0, body.center().y),
            1.0,
            theme::p().text_secondary,
        );
        raised::focus_ring(ui.painter(), &response, rect);
    }
    let picked = Menu::below(&response).show(|ui| {
        let mut picked = None;
        widgets::menu_caption(ui, "Kits");
        for preset in Preset::ALL {
            if widgets::choice_row(ui, preset.name, preset.name == showing).clicked() {
                picked = Some(Choice::Preset(preset));
                ui.close();
            }
        }
        if !project.is_empty() {
            widgets::menu_caption(ui, "This project's maps");
            for (id, name, sounds) in project {
                let label = format!("{name} ({sounds} sounds)");
                if widgets::choice_row(ui, &label, Some(*id) == current).clicked() {
                    picked = Some(Choice::Project(*id));
                    ui.close();
                }
            }
        }
        if !users.is_empty() {
            widgets::menu_caption(ui, "Your maps");
            for saved in users {
                if widgets::choice_row(ui, &saved.name, false).clicked() {
                    picked = Some(Choice::User(saved.path.clone()));
                    ui.close();
                }
            }
        }
        picked
    });
    picked.flatten()
}

/// Runs `task` on a thread of its own and hands back its answer.
fn spawn(task: impl FnOnce() -> Done + Send + 'static) -> Job {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = crate::spawn::spawn_named("lumen-drum-map-file", move || {
        let _ = tx.send(task());
    });
    Job { rx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_wide_enough_for_its_table_and_fits_the_shortest_window() {
        assert!(
            WIDTH >= table::Columns::least_width(),
            "{WIDTH} against {}",
            table::Columns::least_width()
        );
        // It fits the shortest window the app opens at (960 pt).
        assert_eq!(WIDTH.min(960.0), WIDTH);
    }

    #[test]
    fn a_notice_says_which_it_is_and_is_never_cut() {
        let good = Notice::good("Saved as Kit.lumendrums");
        assert!(!good.bad);
        let bad = Notice::bad(
            "Broken.lumendrums is not a drum map Lumen can read: expected value at line 1 column 2",
        );
        assert!(bad.bad);
        assert!(!bad.words.ends_with('\u{2026}'), "read in full");
    }
}
