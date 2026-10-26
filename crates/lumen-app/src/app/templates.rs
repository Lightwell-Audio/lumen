//! Templates in the app (M9 slice R4, ADR-0105): Save as template, New from
//! template and the templates folder. The folder's rules and its listing are
//! `crate::templates`; writing the template is the copy worker
//! (`app::copies`), which takes every audio file along and leaves the open
//! project as it is.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, Align2, Key, RichText, WidgetInfo, WidgetType, pos2};

use crate::app::copies::Purpose;
use crate::app::projects::{self, Next};
use crate::commands::Command;
use crate::templates::{self, LONGEST_NAME, Listing};
use crate::widgets::{self, Dialog, raised};
use crate::{App, files, theme};

/// How much audio a template would hold, read on a worker.
enum Size {
    Reading(Receiver<(u64, usize)>),
    Known { bytes: u64, files: usize },
}

impl Size {
    /// Starts reading the sizes of `files`.
    fn read(files: Vec<PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("lumen-template-size".into())
            .spawn(move || {
                let sizes = files
                    .iter()
                    .filter_map(|file| std::fs::metadata(file).ok())
                    .filter(|meta| meta.is_file());
                let total = sizes.fold((0, 0), |(bytes, n), meta| (bytes + meta.len(), n + 1));
                let _ = tx.send(total);
            })
            .expect("spawn template size reader");
        Self::Reading(rx)
    }

    fn poll(&mut self) {
        if let Self::Reading(rx) = self
            && let Ok((bytes, files)) = rx.try_recv()
        {
            *self = Self::Known { bytes, files };
        }
    }
}

/// The window that names a template.
struct Naming {
    name: String,
    /// The field takes focus, with its text selected, on the first frame.
    fresh: bool,
    /// The field takes focus again (Enter leaves it).
    refocus: bool,
    /// The name is a template already and Enter was pressed once.
    confirming: bool,
    size: Size,
}

/// What the app keeps about templates.
#[derive(Default)]
pub(crate) struct Templates {
    pub(crate) listing: Listing,
    naming: Option<Naming>,
    /// The project being opened is a template (New from template).
    pub(crate) opening: bool,
    /// Whether "Open templates folder" shows a window of the file manager;
    /// tests only note the folder (`revealed`).
    pub(crate) reveals: bool,
    pub(crate) revealed: Vec<PathBuf>,
}

impl Templates {
    pub(crate) fn new(reveals: bool) -> Self {
        Self {
            reveals,
            ..Self::default()
        }
    }

    /// Whether the naming window is showing.
    pub(crate) fn naming_open(&self) -> bool {
        self.naming.is_some()
    }
}

impl App {
    /// Where templates are kept: `Documents/Lumen/Templates`, beside the
    /// projects (the user folder). A run without one (a test, an isolated
    /// run) keeps them beside the autosaves, in local data, else beside the
    /// settings.
    pub(crate) fn templates_dir(&self) -> PathBuf {
        if let Some(user) = &self.user_dir {
            return user.join(projects::TEMPLATES);
        }
        self.autosave
            .as_ref()
            .and_then(|a| a.dir())
            .and_then(Path::parent)
            .or_else(|| self.settings_path.as_deref().and_then(Path::parent))
            .map_or_else(
                || std::env::temp_dir().join("lumen-templates"),
                |dir| dir.join("Templates"),
            )
    }

    /// Whether a new project can start from a template now, or why not: a
    /// copy under way belongs to the open project.
    pub(crate) fn new_unavailable(&self) -> Result<(), &'static str> {
        if self.collecting.is_some() {
            Err("A copy is under way")
        } else {
            Ok(())
        }
    }

    /// The listing's answers, and its first question. Call every frame.
    pub(crate) fn poll_templates(&mut self, ctx: &egui::Context) {
        if !self.templates.listing.asked_once() {
            self.templates.listing.ask(ctx, self.templates_dir(), false);
        }
        self.templates.listing.poll();
    }

    /// Save as template: asks for a name in a small window.
    pub(crate) fn open_save_template(&mut self, ctx: &egui::Context) {
        if let Err(why) = self.copy_unavailable() {
            self.say(format!("{why}. Try again in a moment"));
            return;
        }
        let files: Vec<PathBuf> = self.document.media_files.values().cloned().collect();
        self.templates.naming = Some(Naming {
            name: self.document.name(),
            fresh: true,
            refocus: false,
            confirming: false,
            size: Size::read(files),
        });
        // The names that exist are read again before the person types one.
        self.templates.listing.ask(ctx, self.templates_dir(), true);
    }

    /// The naming window. Call every frame.
    pub(crate) fn save_template_window(&mut self, ctx: &egui::Context) {
        let Some(mut naming) = self.templates.naming.take() else {
            return;
        };
        let dir = self.templates_dir();
        self.templates.listing.ask(ctx, dir.clone(), false);
        naming.size.poll();
        let listing = &self.templates.listing;
        let (mut go, mut cancel) = (false, false);
        let close = Dialog::new("save template", "Save as template", 340.0)
            .at(
                pos2(ctx.content_rect().center().x, 96.0),
                Align2::CENTER_TOP,
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "New projects can start from a template, with its tracks, devices and audio.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(theme::space::S);
                let field = ui.add(
                    egui::TextEdit::singleline(&mut naming.name)
                        .id(egui::Id::new("template name"))
                        .hint_text("Template name")
                        .char_limit(LONGEST_NAME)
                        .desired_width(f32::INFINITY),
                );
                field.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::TextEdit, true, "Template name")
                });
                if std::mem::take(&mut naming.fresh) {
                    field.request_focus();
                    // The name offered is all selected: typing replaces it.
                    if let Some(mut edit) = egui::TextEdit::load_state(ui.ctx(), field.id) {
                        let end = egui::text::CCursor::new(naming.name.chars().count());
                        edit.cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                end,
                            )));
                        edit.store(ui.ctx(), field.id);
                    }
                }
                if std::mem::take(&mut naming.refocus) {
                    field.request_focus();
                }
                if field.changed() {
                    naming.confirming = false;
                }
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));

                let typed = naming.name.trim().to_owned();
                let kept = templates::safe(&typed);
                let exists = !typed.is_empty() && listing.has(&typed);
                ui.add_space(theme::space::XS);
                if typed.is_empty() {
                    ui.label(RichText::new("Give the template a name.").small().weak());
                } else if kept != typed {
                    ui.label(
                        RichText::new(format!("It is kept as {kept}."))
                            .small()
                            .weak(),
                    );
                }
                if exists {
                    let words = if naming.confirming {
                        format!("Press Enter again to replace {kept}.")
                    } else {
                        format!("A template called {kept} exists. Saving replaces it.")
                    };
                    ui.label(RichText::new(words).small().color(theme::p().warning));
                }
                match &naming.size {
                    Size::Reading(_) => {}
                    Size::Known { bytes, files } if *files > 0 => {
                        ui.label(
                            RichText::new(format!(
                                "Takes {} of audio ({files} file{})",
                                files::describe_size(*bytes),
                                if *files == 1 { "" } else { "s" }
                            ))
                            .small()
                            .weak(),
                        );
                    }
                    Size::Known { .. } => {}
                }
                ui.add_space(theme::space::S);
                ui.horizontal(|ui| {
                    let label = if exists { "Replace" } else { "Save template" };
                    ui.add_enabled_ui(!typed.is_empty(), |ui| {
                        if raised::primary_button(ui, label).clicked() {
                            go = true;
                        }
                    });
                    if raised::button(ui, "Cancel").clicked() {
                        cancel = true;
                    }
                });
                if entered && !typed.is_empty() {
                    if exists && !naming.confirming {
                        naming.confirming = true;
                        naming.refocus = true;
                    } else {
                        go = true;
                    }
                }
            });
        if close || cancel {
            return;
        }
        if go {
            if let Err(why) = self.copy_unavailable() {
                self.say(format!("{why}. Try again in a moment"));
            } else {
                let file = templates::file_for(&dir, &naming.name);
                self.start_save_for(Purpose::Template, file, false);
                return;
            }
        }
        self.templates.naming = Some(naming);
    }

    /// New from template: another project replaces the open one, so unsaved
    /// changes are offered a save first.
    pub(crate) fn new_from_listed_template(&mut self, ctx: &egui::Context, index: usize) {
        let Some(template) = self.templates.listing.found().get(index) else {
            return;
        };
        let file = template.file.clone();
        self.request(ctx, Next::Template(file));
    }

    /// "New from template" in the app menu: the templates found, and the way
    /// to the folder they are kept in.
    pub(crate) fn template_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        widgets::submenu(ui, "New from template", |ui| {
            self.templates
                .listing
                .ask(&ctx, self.templates_dir(), false);
            let found = self.templates.listing.found().to_vec();
            let enabled = self.new_unavailable().is_ok();
            if found.is_empty() {
                ui.label(RichText::new("No templates yet").small().weak());
            }
            for template in found {
                if widgets::menu_item(ui, &template.name, None, enabled)
                    .on_hover_text(template.file.display().to_string())
                    .clicked()
                {
                    self.request(&ctx, Next::Template(template.file));
                    ui.close();
                }
            }
            ui.separator();
            self.menu_command(ui, Command::OpenTemplatesFolder, "Open templates folder");
        });
    }

    /// Open templates folder: shown in the system's file manager, made first
    /// when no template has been saved yet.
    pub(crate) fn open_templates_folder(&mut self) {
        let dir = self.templates_dir();
        let reveals = self.templates.reveals;
        let shown = dir.clone();
        std::thread::Builder::new()
            .name("lumen-templates-folder".into())
            .spawn(move || {
                let _ = std::fs::create_dir_all(&shown);
                if reveals {
                    crate::crash::reveal(&shown);
                }
            })
            .expect("spawn templates folder opener");
        self.templates.revealed.push(dir);
    }

    /// What New from template says when the project is open.
    pub(crate) fn said_new_from(&mut self, file: &Path) {
        let called = file
            .file_stem()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.say(format!(
            "New project from the template {called}. Its audio is copied in when you save"
        ));
    }
}
