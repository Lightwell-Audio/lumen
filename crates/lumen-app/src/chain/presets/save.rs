//! Saving a card's sound: reading the plugin's state, then writing the preset
//! file, and the folder the files are kept in.

use super::state::{Saving, Source};
use super::*;

impl App {
    /// Starts saving the sound of `device`: its plugin is read (or a built-in's
    /// values are taken as they are), then the file is written. `over` writes
    /// over a file (Save); otherwise the file gets an unused name (Save as).
    pub(super) fn begin_preset_save(
        &mut self,
        ctx: &egui::Context,
        device: Id,
        name: String,
        over: Option<PathBuf>,
    ) {
        let Some(folder) = self.preset_folder(device) else {
            self.say("There is no presets folder on this computer.".to_owned());
            return;
        };
        let Some((_, held)) = find_device(self.document.project(), device) else {
            return;
        };
        if crate::builtin::is_builtin(held) {
            // A built-in's sound is what its values say: nothing to read.
            let bytes =
                crate::preset_files::builtin_bytes(crate::builtin::name_of(held), &held.params);
            self.chain.presets.files.write(
                ctx,
                device,
                crate::preset_files::Written {
                    folder,
                    over,
                    name,
                    bytes,
                    extension: crate::preset_files::BUILTIN_EXTENSION,
                },
            );
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

    pub(super) fn drive_save(&mut self, ctx: &egui::Context) {
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
        self.chain.presets.files.write(
            ctx,
            device,
            crate::preset_files::Written {
                folder,
                over,
                name,
                bytes,
                extension: crate::preset_files::EXTENSION,
            },
        );
    }

    /// Save, for the chosen device (the command).
    pub(crate) fn save_chosen_preset(&mut self, ctx: &egui::Context) {
        if let Some(device) = self.chain.selected {
            self.save_for(ctx, device);
        }
    }

    /// The user's own file the sound of `device` came from, with its name:
    /// what Save writes over.
    pub(super) fn save_over(&self, device: Id) -> Option<(String, PathBuf)> {
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
            Some((name, path)) => self.begin_preset_save(ctx, device, name, Some(path)),
            None => self.open_preset_menu(ctx, device, true),
        }
    }

    /// Shows the folder where the device's presets are saved.
    pub(super) fn show_presets_folder(&mut self, ctx: &egui::Context, device: Id) {
        let Some(folder) = self.preset_folder(device) else {
            self.say("There is no presets folder on this computer.".to_owned());
            return;
        };
        // Whether it is there is found out on the file thread.
        self.chain.presets.files.folder(ctx, folder);
    }

    /// The file thread has looked for a plugin's presets folder: show it, or
    /// say nothing is saved yet.
    pub(super) fn folder_found(&mut self, folder: PathBuf, exists: bool) {
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
