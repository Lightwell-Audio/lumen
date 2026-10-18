//! Loading programs and presets into plugin devices (M6 slice 5). A factory
//! program is chosen through the plugin's own program parameter: the choice and
//! what the plugin sets in answer are one undo step. A preset file's state is
//! tried on the running plugin first and put into the project only when the
//! plugin took it, so a damaged or foreign state changes nothing; the load is
//! one undo step ("Load <name>") that undo gives back exactly.

use lumen_hosting::Preset;
use lumen_project::model::{Id, PluginRef};

use crate::App;
use crate::document::find_device;
use crate::hosting::PresetNews;

/// The merge key of one program choice, shared by the choice and the edits the
/// plugin makes in answer.
pub(crate) fn load_key(device: Id) -> String {
    format!("load:{}", device.0)
}

impl App {
    /// Chooses program `program` of list `list` of `device`'s plugin. One undo
    /// step: the program parameter, and the values the plugin sets because of
    /// it. Does nothing for a plugin that offers no way to select a program.
    pub(crate) fn load_program(&mut self, device: Id, list: usize, program: usize) {
        let Some(chosen) = self.hosting.programs(device).get(list) else {
            return;
        };
        let (Some(parameter), Some(name)) = (chosen.parameter, chosen.programs.get(program)) else {
            return;
        };
        let name = name.clone();
        let last = chosen.programs.len().saturating_sub(1);
        // The standard maps a program onto its parameter in equal steps.
        let value = if last == 0 {
            0.0
        } else {
            program as f64 / last as f64
        };
        self.hosting.begin_program(device, name.clone());
        self.document.seal();
        let _ = self.document.set_device_params(
            device,
            &[(parameter, value)],
            &format!("Load {name}"),
            Some(&load_key(device)),
        );
    }

    /// Gives `device`'s plugin the state of a preset file. A preset made for
    /// another plugin, or for a plugin that is not running, is refused with
    /// a note and nothing changes.
    pub(crate) fn load_preset(&mut self, device: Id, name: &str, preset: Preset) {
        let Some((_, held)) = find_device(self.document.project(), device) else {
            return;
        };
        let PluginRef::Vst3 { class_id, .. } = &held.plugin else {
            return;
        };
        if lumen_hosting::parse_class_id(class_id) != Some(preset.class_id) {
            self.say(format!(
                "\u{201c}{name}\u{201d} was made for another plugin."
            ));
            return;
        }
        if !self
            .hosting
            .try_preset(device, name.to_owned(), preset.component, preset.controller)
        {
            self.say("That plugin is not ready to load a preset.".to_owned());
        }
    }

    /// Puts the presets plugins took into the project, and tells the person of
    /// the ones they refused. Called every frame.
    pub(crate) fn apply_presets(&mut self) {
        for news in self.hosting.take_presets() {
            match news {
                PresetNews::Ready {
                    device,
                    name,
                    component,
                    controller,
                } => {
                    match self
                        .document
                        .load_device_state(device, &name, component, controller)
                    {
                        Ok(()) => {
                            if let Some((_, held)) = find_device(self.document.project(), device) {
                                // The plugin already has this sound.
                                self.hosting.synced(device, held.state_rev);
                            }
                        }
                        Err(refusal) => self.say(refusal.to_string()),
                    }
                }
                PresetNews::Refused { name, why, .. } => {
                    self.say(format!(
                        "The plugin could not use \u{201c}{name}\u{201d}: {why}"
                    ));
                }
            }
        }
    }
}
