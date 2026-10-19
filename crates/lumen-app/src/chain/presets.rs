//! A card's preset menu (M6 slice 5f, the owner's A): a field in the card
//! with the name of the sound, arrows to the previous and next, and a menu
//! that lists the plugin's factory programs and the preset files found for it,
//! with a search, Save, Save as… and Show presets folder. A dot marks a sound
//! changed since it was chosen.
//!
//! Choosing is `App::load_program` or `App::load_preset` (slice 5c: one undo
//! step); this module is the list, the name remembered for the card, and saving
//! a file, with the disk work in `preset_files` off the interface's thread.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, FontId, Ui};
use lumen_hosting::{Preset, info_xml, parse_class_id};
use lumen_project::model::{Id, PluginRef};

use crate::App;
use crate::document::find_device;
use crate::hosting::State;
use crate::preset_files::{Done, Entry, Files, NAME_LIMIT, Places};
use crate::{theme, widgets};

/// The menu's width and the height its list may reach.
const WIDTH: f32 = 272.0;
const LIST_HEIGHT: f32 = 264.0;
/// How long a save waits for the plugin to be read.
const SAVE_WAIT: Duration = Duration::from_secs(8);

/// Where the field of the card of `device` is: its id, for opening the menu
/// from a key.
pub(super) fn field_id(device: Id) -> egui::Id {
    egui::Id::new(("device", device.0)).with("preset field")
}

/// What the card's field shows.
pub(crate) struct FieldLook {
    /// The sound's name, when one was chosen.
    pub name: Option<String>,
    /// The sound has been changed since it was chosen.
    pub changed: bool,
}

/// Where a chosen sound came from.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    Program { list: usize, index: usize },
    File { path: PathBuf, user: bool },
}

/// The sound a card was last given by the menu or its arrows.
struct Current {
    name: String,
    source: Source,
    /// The parameter values the project held once the sound had settled; the
    /// sound is changed when they are not what the project holds now.
    baseline: Option<BTreeMap<u32, f64>>,
    /// The device's sound revision when a file was loaded: another one means an
    /// undo or redo went across the load.
    rev: u32,
}

/// One entry of the menu.
#[derive(Debug, Clone, PartialEq)]
enum Choice {
    Program {
        list: usize,
        index: usize,
        name: String,
        /// The list's name, when the plugin offers more than one.
        group: Option<String>,
    },
    File {
        path: PathBuf,
        user: bool,
        name: String,
        group: Option<String>,
    },
}

impl Choice {
    fn name(&self) -> &str {
        match self {
            Self::Program { name, .. } | Self::File { name, .. } => name,
        }
    }

    fn group(&self) -> Option<&str> {
        match self {
            Self::Program { group, .. } | Self::File { group, .. } => group.as_deref(),
        }
    }

    fn heading(&self) -> String {
        let (base, group) = match self {
            Self::Program { group, .. } => ("Factory", group),
            Self::File {
                user: true, group, ..
            } => ("Your presets", group),
            Self::File { group, .. } => ("System presets", group),
        };
        match group {
            Some(group) => format!("{base} \u{b7} {group}"),
            None => base.to_owned(),
        }
    }

    fn is_current(&self, current: &Current) -> bool {
        match (self, &current.source) {
            (Self::Program { list, index, .. }, Source::Program { list: l, index: i }) => {
                list == l && index == i
            }
            (Self::File { path, .. }, Source::File { path: p, .. }) => path == p,
            _ => false,
        }
    }
}

/// A save waiting for the plugin to be read.
struct Saving {
    device: Id,
    name: String,
    /// The file to write over (Save), or none for an unused name (Save as).
    over: Option<PathBuf>,
    since: Instant,
}

#[derive(Default)]
struct MenuState {
    open_for: Option<Id>,
    search: String,
    /// The name being typed for Save as….
    naming: Option<String>,
    focus: Focus,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Focus {
    #[default]
    None,
    Search,
    Name,
}

/// The cards' preset state.
#[derive(Default)]
pub(crate) struct Presets {
    /// Where preset files are kept.
    pub(crate) places: Places,
    files: Files,
    /// The files found for each device's plugin.
    listed: HashMap<Id, Vec<Entry>>,
    /// Devices whose folders have been asked about.
    asked: HashSet<Id>,
    current: HashMap<Id, Current>,
    /// A file being read for a device: its name, path and whether it is the
    /// user's.
    reading: HashMap<Id, (String, PathBuf, bool)>,
    menu: MenuState,
    saving: Option<Saving>,
    /// The folders shown to the person, newest last (tests read it; with fixed
    /// places no window opens).
    pub(crate) revealed: Vec<PathBuf>,
}

impl Presets {
    pub(crate) fn new(places: Places) -> Self {
        Self {
            places,
            ..Self::default()
        }
    }

    /// Asks again for the files of the plugin of `device`.
    fn relist(&mut self, ctx: &egui::Context, device: Id, vendor: &str, plugin: &str) {
        self.asked.insert(device);
        let folders = self.places.folders(vendor, plugin);
        if folders.is_empty() {
            self.listed.entry(device).or_default();
        } else {
            self.files.list(ctx, device, folders);
        }
    }

    /// Gives `device` a chosen sound and a list of files, as if they had been
    /// found (screenshots).
    pub(crate) fn pretend(
        &mut self,
        device: Id,
        name: &str,
        changed: Option<BTreeMap<u32, f64>>,
        files: &[(Option<&str>, &str, bool)],
    ) {
        self.current.insert(
            device,
            Current {
                name: name.to_owned(),
                source: Source::Program { list: 0, index: 0 },
                baseline: changed,
                rev: 0,
            },
        );
        self.asked.insert(device);
        self.listed.insert(
            device,
            files
                .iter()
                .map(|(group, name, user)| Entry {
                    name: (*name).to_owned(),
                    group: group.map(str::to_owned),
                    path: PathBuf::from(format!("/presets/{name}.vstpreset")),
                    user: *user,
                })
                .collect(),
        );
    }

    /// Records that a program was chosen for `device`.
    pub(crate) fn chose_program(
        &mut self,
        device: Id,
        name: &str,
        list: usize,
        index: usize,
        rev: u32,
    ) {
        self.current.insert(
            device,
            Current {
                name: name.to_owned(),
                source: Source::Program { list, index },
                baseline: None,
                rev,
            },
        );
    }

    /// A preset the plugin took is the project's now.
    pub(crate) fn loaded(&mut self, device: Id, name: &str, rev: u32) {
        let Some((reading, path, user)) = self.reading.remove(&device) else {
            // Loaded by a seam or a command: a name, no file.
            self.current.remove(&device);
            return;
        };
        let name = if reading == name { name } else { &reading };
        self.current.insert(
            device,
            Current {
                name: name.to_owned(),
                source: Source::File { path, user },
                baseline: None,
                rev,
            },
        );
    }

    /// A preset was refused: nothing was chosen.
    pub(crate) fn refused(&mut self, device: Id) {
        self.reading.remove(&device);
    }
}

impl App {
    /// What the field of `device`'s card shows; none for a card without one (a
    /// built-in device, or a plugin that is not running).
    pub(crate) fn field_look(&self, device: Id) -> Option<FieldLook> {
        let (_, d) = find_device(self.document.project(), device)?;
        if !d.is_vst3() || !matches!(self.hosting.state(device), Some(State::Ready)) {
            return None;
        }
        let current = self.chain.presets.current.get(&device);
        Some(FieldLook {
            name: current.map(|c| c.name.clone()),
            changed: current
                .and_then(|c| c.baseline.as_ref())
                .is_some_and(|baseline| *baseline != d.params),
        })
    }

    /// Asks for the preset files of a plugin that has not been asked about.
    pub(super) fn ensure_listed(&mut self, ctx: &egui::Context, device: Id) {
        if self.chain.presets.asked.contains(&device) {
            return;
        }
        if let Some((vendor, plugin)) = self.plugin_vendor_name(device) {
            self.chain.presets.relist(ctx, device, &vendor, &plugin);
        }
    }

    fn plugin_vendor_name(&self, device: Id) -> Option<(String, String)> {
        let (_, d) = find_device(self.document.project(), device)?;
        match &d.plugin {
            PluginRef::Vst3 { name, vendor, .. } => Some((vendor.clone(), name.clone())),
            PluginRef::Builtin { .. } => None,
        }
    }

    /// The sounds that can be chosen for `device`, in the menu's order: the
    /// factory programs, then the user's files, then the system's.
    fn choices(&self, device: Id) -> Vec<Choice> {
        let mut out = Vec::new();
        let lists = self.hosting.programs(device);
        let selectable = lists.iter().filter(|l| l.parameter.is_some()).count();
        for (list, chosen) in lists.iter().enumerate() {
            if chosen.parameter.is_none() {
                continue;
            }
            for (index, name) in chosen.programs.iter().enumerate() {
                out.push(Choice::Program {
                    list,
                    index,
                    name: name.clone(),
                    group: (selectable > 1).then(|| chosen.name.clone()),
                });
            }
        }
        if let Some(files) = self.chain.presets.listed.get(&device) {
            out.extend(files.iter().map(|e| Choice::File {
                path: e.path.clone(),
                user: e.user,
                name: e.name.clone(),
                group: e.group.clone(),
            }));
        }
        out
    }

    /// Chooses a sound for `device`.
    fn choose_preset(&mut self, ctx: &egui::Context, device: Id, choice: Choice) {
        match choice {
            Choice::Program {
                list, index, name, ..
            } => {
                let rev =
                    find_device(self.document.project(), device).map_or(0, |(_, d)| d.state_rev);
                self.load_program(device, list, index);
                self.chain
                    .presets
                    .chose_program(device, &name, list, index, rev);
            }
            Choice::File {
                path, user, name, ..
            } => {
                self.chain
                    .presets
                    .reading
                    .insert(device, (name, path.clone(), user));
                self.chain.presets.files.read(ctx, device, path);
            }
        }
    }

    /// The previous or next sound after the one `device` has (the card's
    /// arrows), wrapping round. With none chosen, the first or the last.
    pub(super) fn step_preset(&mut self, ctx: &egui::Context, device: Id, forward: bool) {
        let choices = self.choices(device);
        if choices.is_empty() {
            self.say("This plugin lists no presets. Open its window to choose one.".to_owned());
            return;
        }
        let at = self
            .chain
            .presets
            .current
            .get(&device)
            .and_then(|current| choices.iter().position(|c| c.is_current(current)));
        let last = choices.len() - 1;
        let to = match (at, forward) {
            (Some(i), true) => (i + 1) % choices.len(),
            (Some(0), false) => last,
            (Some(i), false) => i - 1,
            (None, true) => 0,
            (None, false) => last,
        };
        self.choose_preset(ctx, device, choices[to].clone());
    }

    /// Once a frame: files found, read and written; a save waiting for its
    /// plugin; the names of the sounds kept honest.
    pub(crate) fn poll_presets(&mut self, ctx: &egui::Context) {
        for done in self.chain.presets.files.poll() {
            match done {
                Done::Listed { device, entries } => {
                    self.chain.presets.listed.insert(device, entries);
                }
                Done::Read {
                    device,
                    name,
                    result,
                } => match result {
                    Ok(preset) => self.load_preset(device, &name, preset),
                    Err(why) => {
                        self.chain.presets.refused(device);
                        self.say(format!("\u{201c}{name}\u{201d} cannot be used: {why}."));
                    }
                },
                Done::Folder { folder, exists, .. } => self.folder_found(folder, exists),
                Done::Wrote {
                    device,
                    name,
                    result,
                } => match result {
                    Ok(path) => self.saved(ctx, device, path),
                    Err(why) => self.say(format!("Could not save \u{201c}{name}\u{201d}: {why}.")),
                },
            }
        }
        self.drive_save(ctx);
        self.keep_names_honest();
        if let Some(wait) = self.hosting.next_retry() {
            ctx.request_repaint_after(wait);
        }
    }

    /// A file was written: it is the card's sound now, and listed.
    fn saved(&mut self, ctx: &egui::Context, device: Id, path: PathBuf) {
        let name = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let rev = find_device(self.document.project(), device).map_or(0, |(_, d)| d.state_rev);
        self.say(format!("Saved \u{201c}{name}\u{201d}."));
        self.chain.presets.current.insert(
            device,
            Current {
                name,
                source: Source::File { path, user: true },
                baseline: None,
                rev,
            },
        );
        if let Some((vendor, plugin)) = self.plugin_vendor_name(device) {
            self.chain.presets.relist(ctx, device, &vendor, &plugin);
        }
    }

    /// Drops the name of a sound an undo took away, and notes the values a
    /// sound settled at so a change after it shows.
    fn keep_names_honest(&mut self) {
        let project = self.document.project();
        let hosting = &self.hosting;
        self.chain.presets.current.retain(|&device, current| {
            let Some((_, d)) = find_device(project, device) else {
                return false;
            };
            if matches!(current.source, Source::File { .. }) && d.state_rev != current.rev {
                return false;
            }
            // A program's answering edits are part of it for a moment; a
            // file is the whole sound at once.
            let settled = matches!(current.source, Source::File { .. })
                || hosting.program_loading(device).is_none();
            if current.baseline.is_none() && settled {
                current.baseline = Some(d.params.clone());
            }
            true
        });
    }

    /// Starts saving the sound of `device`: its plugin is read, then the file
    /// is written. `over` writes over a file (Save); otherwise the file gets an
    /// unused name (Save as).
    fn begin_preset_save(&mut self, device: Id, name: String, over: Option<PathBuf>) {
        if self.chain.presets.places.user.is_none() {
            self.say("There is no presets folder on this computer.".to_owned());
            return;
        }
        if !self.hosting.capture_one(device) {
            self.say("That plugin is not running, so its sound cannot be saved.".to_owned());
            return;
        }
        self.chain.presets.saving = Some(Saving {
            device,
            name,
            over,
            since: Instant::now(),
        });
    }

    fn drive_save(&mut self, ctx: &egui::Context) {
        let Some(saving) = &self.chain.presets.saving else {
            return;
        };
        let device = saving.device;
        if let Some(state) = self.hosting.fresh_state(device) {
            let Saving { name, over, .. } =
                self.chain.presets.saving.take().expect("checked above");
            self.write_preset(ctx, device, name, over, state);
            return;
        }
        if self.hosting.capture_failed(device) || saving.since.elapsed() > SAVE_WAIT {
            let name = saving.name.clone();
            self.chain.presets.saving = None;
            self.say(format!(
                "Could not read the plugin's sound to save \u{201c}{name}\u{201d}."
            ));
            return;
        }
        // Changed while it was read: ask again.
        self.hosting.capture_one(device);
        ctx.request_repaint_after(crate::motion::RECHECK);
    }

    fn write_preset(
        &mut self,
        ctx: &egui::Context,
        device: Id,
        name: String,
        over: Option<PathBuf>,
        state: lumen_hosting::Captured,
    ) {
        let Some((_, held)) = find_device(self.document.project(), device) else {
            return;
        };
        let PluginRef::Vst3 {
            class_id,
            name: plugin,
            vendor,
            ..
        } = &held.plugin
        else {
            return;
        };
        let (Some(class), Some(folder)) = (
            parse_class_id(class_id),
            self.chain.presets.places.user_folder(vendor, plugin),
        ) else {
            self.say("There is no presets folder on this computer.".to_owned());
            return;
        };
        if state.component.is_empty() && state.controller.is_none() {
            self.say("This plugin has no sound of its own to save.".to_owned());
            return;
        }
        let bytes = Preset {
            class_id: class,
            component: state.component,
            controller: state.controller,
            info: Some(info_xml(plugin)),
        }
        .to_bytes();
        self.chain
            .presets
            .files
            .write(ctx, device, folder, over, name, bytes);
    }

    /// Save, for the chosen device (the command).
    pub(crate) fn save_chosen_preset(&mut self, ctx: &egui::Context) {
        if let Some(device) = self.chain.selected {
            self.save_for(ctx, device);
        }
    }

    /// Opens the menu of the chosen device's field (the command); with
    /// `naming` it opens on the name for Save as….
    pub(crate) fn open_preset_menu(&mut self, ctx: &egui::Context, device: Id, naming: bool) {
        let offered = naming.then(|| self.current_name(device));
        self.chain.presets.menu = MenuState {
            open_for: Some(device),
            focus: if naming { Focus::Name } else { Focus::Search },
            naming: offered,
            ..MenuState::default()
        };
        if let Some((vendor, plugin)) = self.plugin_vendor_name(device) {
            self.chain.presets.relist(ctx, device, &vendor, &plugin);
        }
        widgets::Menu::open_for(ctx, field_id(device));
    }

    /// Whether the chosen device has a preset field (a running plugin).
    pub(crate) fn chosen_has_presets(&self) -> bool {
        self.chain
            .selected
            .is_some_and(|d| self.field_look(d).is_some())
    }

    /// The menu under `device`'s field, when it is open.
    pub(super) fn preset_menu(&mut self, field: &egui::Response, device: Id) {
        // Staying: typing a name is no reason for the menu to go when the
        // pointer leaves it; Esc or a click elsewhere closes it.
        let shown = widgets::Menu::below(field)
            .staying()
            .show(|ui| self.preset_menu_body(ui, device));
        if shown.is_none() && self.chain.presets.menu.open_for == Some(device) {
            self.chain.presets.menu = MenuState::default();
        }
    }

    fn preset_menu_body(&mut self, ui: &mut Ui, device: Id) {
        let ctx = ui.ctx().clone();
        ui.set_width(WIDTH);
        if self.chain.presets.menu.open_for != Some(device) {
            // Opened by a click on the field.
            self.chain.presets.menu = MenuState {
                open_for: Some(device),
                focus: Focus::Search,
                ..MenuState::default()
            };
            if let Some((vendor, plugin)) = self.plugin_vendor_name(device) {
                self.chain.presets.relist(&ctx, device, &vendor, &plugin);
            }
        }
        let mut menu = std::mem::take(&mut self.chain.presets.menu);
        let mut close = false;
        enum Act {
            Save,
            SaveAs,
            Folder,
        }
        let mut act: Option<Act> = None;

        if let Some(name) = menu.naming.as_mut() {
            widgets::menu_caption(ui, "Save as");
            let field = ui.add(
                egui::TextEdit::singleline(name)
                    .id(egui::Id::new(("preset name", device.0)))
                    .hint_text("Name")
                    .char_limit(NAME_LIMIT)
                    .desired_width(f32::INFINITY),
            );
            field.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Preset name")
            });
            if menu.focus == Focus::Name {
                field.request_focus();
                menu.focus = Focus::None;
                // The name offered is all selected: typing replaces it.
                if let Some(mut edit) = egui::TextEdit::load_state(&ctx, field.id) {
                    let end = egui::text::CCursor::new(name.chars().count());
                    edit.cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            end,
                        )));
                    edit.store(&ctx, field.id);
                }
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut save = enter;
            ui.horizontal(|ui| {
                save |= crate::widgets::raised::primary_button(ui, "Save preset").clicked();
                if crate::widgets::raised::button(ui, "Cancel").clicked() {
                    menu.naming = None;
                }
            });
            if save {
                let typed = menu.naming.take().unwrap_or_default();
                if typed.trim().is_empty() {
                    self.say("Give the preset a name first.".to_owned());
                } else {
                    let name: String = typed.trim().chars().take(NAME_LIMIT).collect();
                    self.begin_preset_save(device, name, None);
                    close = true;
                }
            }
        } else {
            let search = ui.add(
                egui::TextEdit::singleline(&mut menu.search)
                    .id(egui::Id::new(("preset search", device.0)))
                    .hint_text("Search presets")
                    .desired_width(f32::INFINITY),
            );
            search.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search presets")
            });
            if menu.focus == Focus::Search {
                search.request_focus();
                menu.focus = Focus::None;
            }
            ui.add_space(theme::space::XS);
            let choices = self.choices(device);
            let shown = filter(&choices, &menu.search);
            if let Some(picked) = self.preset_list(ui, device, &choices, &shown) {
                self.choose_preset(&ctx, device, picked);
                close = true;
            }
            ui.separator();
            if widgets::menu_toggle(ui, "Save", None, false).clicked() {
                act = Some(Act::Save);
            }
            if widgets::menu_toggle(ui, "Save as\u{2026}", None, false).clicked() {
                act = Some(Act::SaveAs);
            }
            if widgets::menu_toggle(ui, "Show presets folder", None, false).clicked() {
                act = Some(Act::Folder);
            }
        }
        match act {
            Some(Act::Save) => match self.save_over(device) {
                Some((name, path)) => {
                    close = true;
                    self.begin_preset_save(device, name, Some(path));
                }
                // No file of the user's own to write over: ask for a name.
                None => {
                    menu.naming = Some(self.current_name(device));
                    menu.focus = Focus::Name;
                }
            },
            Some(Act::SaveAs) => {
                menu.naming = Some(self.current_name(device));
                menu.focus = Focus::Name;
            }
            Some(Act::Folder) => {
                self.show_presets_folder(ui.ctx(), device);
                close = true;
            }
            None => {}
        }
        self.chain.presets.menu = menu;
        self.finish_menu(ui, close);
    }

    fn finish_menu(&mut self, ui: &mut Ui, close: bool) {
        if close {
            self.chain.presets.menu = MenuState::default();
            ui.close();
        }
    }

    /// The name offered for Save as…: the sound's own, cut to the limit.
    fn current_name(&self, device: Id) -> String {
        self.chain
            .presets
            .current
            .get(&device)
            .map(|c| c.name.chars().take(NAME_LIMIT).collect())
            .unwrap_or_default()
    }

    /// The user's own file the sound of `device` came from, with its name:
    /// what Save writes over.
    fn save_over(&self, device: Id) -> Option<(String, PathBuf)> {
        let current = self.chain.presets.current.get(&device)?;
        match &current.source {
            Source::File { path, user: true } => Some((current.name.clone(), path.clone())),
            _ => None,
        }
    }

    /// Save for `device` (the command): writes over the user's own file the
    /// sound came from, or opens the menu on a name.
    fn save_for(&mut self, ctx: &egui::Context, device: Id) {
        match self.save_over(device) {
            Some((name, path)) => self.begin_preset_save(device, name, Some(path)),
            None => self.open_preset_menu(ctx, device, true),
        }
    }

    /// The list of sounds: headings, then rows, as many as are in view. The
    /// one picked, if any.
    fn preset_list(
        &self,
        ui: &mut Ui,
        device: Id,
        all: &[Choice],
        shown: &[usize],
    ) -> Option<Choice> {
        if shown.is_empty() {
            let words = if all.is_empty() {
                "This plugin lists no presets, and none are saved for it."
            } else {
                "No preset matches. Clear the search to see them all."
            };
            ui.add_space(theme::space::XS);
            ui.label(egui::RichText::new(words).small().weak());
            return None;
        }
        // Rows and the headings between them, as a flat list so only the
        // rows in view are drawn.
        enum Line<'a> {
            Heading(String),
            Item(&'a Choice),
        }
        let mut lines: Vec<Line<'_>> = Vec::new();
        let mut last: Option<String> = None;
        for &i in shown {
            let heading = all[i].heading();
            if last.as_deref() != Some(heading.as_str()) {
                lines.push(Line::Heading(heading.clone()));
                last = Some(heading);
            }
            lines.push(Line::Item(&all[i]));
        }
        let current = self.chain.presets.current.get(&device);
        let font = FontId::proportional(theme::size::BODY);
        let row_height = ui.spacing().interact_size.y + theme::space::XS;
        let mut picked = None;
        egui::ScrollArea::vertical()
            .id_salt(("preset list", device.0))
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show_rows(ui, row_height, lines.len(), |ui, range| {
                for line in &lines[range] {
                    match line {
                        Line::Heading(text) => {
                            // As tall as a row, so the list's height is exact.
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), row_height),
                                egui::Sense::hover(),
                            );
                            ui.painter().text(
                                egui::pos2(rect.left() + theme::space::S, rect.bottom() - 2.0),
                                egui::Align2::LEFT_BOTTOM,
                                text,
                                FontId::proportional(theme::size::CAPTION),
                                theme::p().text_hint,
                            );
                        }
                        Line::Item(choice) => {
                            let on = current.is_some_and(|c| choice.is_current(c));
                            let label = widgets::fit(
                                ui.painter(),
                                choice.name(),
                                font.clone(),
                                WIDTH - 2.0 * theme::space::M,
                            );
                            let row = widgets::choice_row(ui, &label, on);
                            if row.clicked() {
                                picked = Some((*choice).clone());
                            }
                        }
                    }
                }
            });
        picked
    }

    /// Shows the folder where the plugin's presets are saved.
    fn show_presets_folder(&mut self, ctx: &egui::Context, device: Id) {
        let Some((vendor, plugin)) = self.plugin_vendor_name(device) else {
            return;
        };
        let Some(folder) = self.chain.presets.places.user_folder(&vendor, &plugin) else {
            self.say("There is no presets folder on this computer.".to_owned());
            return;
        };
        // Whether it is there is found out on the file thread.
        self.chain.presets.files.folder(ctx, folder);
    }

    /// The file thread has looked for a plugin's presets folder: show it, or
    /// say nothing is saved yet.
    fn folder_found(&mut self, folder: PathBuf, exists: bool) {
        if !exists {
            self.say(
                "Nothing is saved for this plugin yet. Save as\u{2026} makes the folder."
                    .to_owned(),
            );
            return;
        }
        if !self.chain.presets.places.fixed {
            crate::crash::reveal(&folder);
        }
        self.chain.presets.revealed.push(folder);
    }
}

/// The indexes of the choices that match every word of `search`, in order.
fn filter(choices: &[Choice], search: &str) -> Vec<usize> {
    let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
    choices
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            let text = format!("{} {}", c.name(), c.group().unwrap_or("")).to_lowercase();
            words.iter().all(|w| text.contains(w))
        })
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(index: usize, name: &str) -> Choice {
        Choice::Program {
            list: 0,
            index,
            name: name.to_owned(),
            group: None,
        }
    }

    fn file(name: &str, user: bool, group: Option<&str>) -> Choice {
        Choice::File {
            path: PathBuf::from(format!("/p/{name}.vstpreset")),
            user,
            name: name.to_owned(),
            group: group.map(str::to_owned),
        }
    }

    #[test]
    fn a_search_needs_every_word_in_the_name_or_the_group() {
        let all = [
            program(0, "Warm Pad"),
            program(1, "Hard Lead"),
            file("Warm Bass", true, Some("Bass")),
        ];
        assert_eq!(filter(&all, ""), [0, 1, 2]);
        assert_eq!(filter(&all, "warm"), [0, 2]);
        assert_eq!(filter(&all, "warm bass"), [2]);
        assert_eq!(filter(&all, "BASS warm"), [2]);
        assert!(filter(&all, "nothing").is_empty());
    }

    #[test]
    fn headings_say_where_a_sound_came_from() {
        assert_eq!(program(0, "x").heading(), "Factory");
        assert_eq!(file("x", true, None).heading(), "Your presets");
        assert_eq!(
            file("x", false, Some("Bass")).heading(),
            "System presets \u{b7} Bass"
        );
    }

    #[test]
    fn the_current_sound_is_found_by_its_place_not_its_name() {
        let current = Current {
            name: "Warm Pad".to_owned(),
            source: Source::Program { list: 0, index: 1 },
            baseline: None,
            rev: 0,
        };
        assert!(!program(0, "Warm Pad").is_current(&current));
        assert!(program(1, "Other name").is_current(&current));
        assert!(!file("Warm Pad", true, None).is_current(&current));
    }
}
