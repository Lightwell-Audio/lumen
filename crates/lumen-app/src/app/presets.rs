use lumen_hosting::Preset;
use lumen_project::model::{Id, PluginRef};

use crate::App;
use crate::document::find_device;
use crate::hosting::PresetNews;

pub(crate) fn load_key(device: Id) -> String {
    format!("load:{}", device.0)
}

impl App {
    pub(crate) fn load_program(&mut self, device: Id, list: usize, program: usize) {
        let Some(chosen) = self.hosting.programs(device).get(list) else {
            return;
        };
        let (Some(parameter), Some(name)) = (chosen.parameter, chosen.programs.get(program)) else {
            return;
        };
        let name = name.clone();
        let last = chosen.programs.len().saturating_sub(1);
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

    pub(crate) fn load_builtin_preset(
        &mut self,
        device: Id,
        name: &str,
        for_device: &str,
        values: std::collections::BTreeMap<u32, f64>,
    ) {
        let Some((_, held)) = find_device(self.document.project(), device) else {
            return;
        };
        if !crate::builtin::is_builtin(held) || crate::builtin::name_of(held) != for_device {
            self.say(format!(
                "\u{201c}{name}\u{201d} was made for another device."
            ));
            self.chain.presets.refused(device);
            return;
        }
        let mut all: Vec<(u32, f64)> = crate::builtin::defaults(for_device)
            .into_iter()
            .map(|(id, value)| (id, f64::from(value)))
            .collect();
        for (id, value) in values {
            match all.iter_mut().find(|(known, _)| *known == id) {
                Some(slot) => slot.1 = value,
                None => all.push((id, value)),
            }
        }
        match self
            .document
            .set_device_params(device, &all, &format!("Load {name}"), None)
        {
            Ok(()) => {
                let rev =
                    find_device(self.document.project(), device).map_or(0, |(_, d)| d.state_rev);
                self.chain.presets.loaded(device, name, rev);
            }
            Err(refusal) => {
                self.chain.presets.refused(device);
                self.say(refusal.to_string());
            }
        }
    }

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
                                self.hosting.synced(device, held.state_rev);
                                self.chain.presets.loaded(device, &name, held.state_rev);
                            }
                        }
                        Err(refusal) => {
                            self.chain.presets.refused(device);
                            self.say(refusal.to_string());
                        }
                    }
                }
                PresetNews::Refused { device, name, why } => {
                    self.chain.presets.refused(device);
                    self.say(format!(
                        "The plugin could not use \u{201c}{name}\u{201d}: {why}"
                    ));
                }
            }
        }
    }
}
