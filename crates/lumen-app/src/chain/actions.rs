use std::time::Instant;

use lumen_plugin_catalog::catalog::{Device as Listed, Kind, Source};
use lumen_project::model::{DeviceKind, Id, PluginRef, SYNTH};

use super::place::{Place, place};
use lumen_project::routing::all_tracks;

use crate::App;
use crate::app::detail::DetailTab;
use crate::document::{Refusal, Slot};
use crate::hosting::State;

pub(crate) fn placeable(listed: &Listed) -> bool {
    plugin_ref(listed).is_some()
}

pub(super) fn plugin_ref(listed: &Listed) -> Option<PluginRef> {
    match listed.source {
        Source::Builtin if listed.id == "builtin:synth" => {
            Some(PluginRef::Builtin { name: SYNTH.into() })
        }
        Source::Builtin => None,
        Source::Vst3 => Some(PluginRef::Vst3 {
            class_id: listed.id.strip_prefix("vst3:")?.to_owned(),
            name: listed.name.clone(),
            vendor: listed.vendor.clone(),
            kind: match listed.kind {
                Kind::Instrument => DeviceKind::Instrument,
                Kind::Effect => DeviceKind::Effect,
            },
        }),
    }
}

impl App {
    pub(crate) fn chain_track(&self) -> Option<Id> {
        self.subject()
    }

    pub(crate) fn add_listed_device(&mut self, id: &str) {
        let Some(plugin) = self.plugins.catalog().find(id).and_then(plugin_ref) else {
            self.say("That device can't be put on a track".to_owned());
            return;
        };
        let wanted = plugin.kind();
        let place = place(
            self.document.project(),
            &self.arrangement.selected,
            self.chain.selected,
            wanted,
        );
        self.add_device_at(plugin, place);
    }

    pub(crate) fn add_device_label(&self, kind: DeviceKind) -> String {
        let project = self.document.project();
        match place(
            project,
            &self.arrangement.selected,
            self.chain.selected,
            kind,
        ) {
            Place::Track { track, .. } => match all_tracks(project).find(|t| t.id == track) {
                Some(track) => format!("Add to {}", track.name),
                None => "Add on a new track".to_owned(),
            },
            Place::NewTrack { .. } => "Add on a new track".to_owned(),
        }
    }

    pub(crate) fn add_dropped_device(&mut self, id: &str, track: Id) {
        let Some(plugin) = self.plugins.catalog().find(id).and_then(plugin_ref) else {
            self.say("That device can't be put on a track".to_owned());
            return;
        };
        let place = place(self.document.project(), &[track], None, plugin.kind());
        self.add_device_showing_window(plugin, place);
    }

    pub(crate) fn add_device_showing_window(&mut self, plugin: PluginRef, place: Place) {
        let hosted = matches!(plugin, PluginRef::Vst3 { .. });
        if let Some(device) = self.add_device_at(plugin, place)
            && hosted
        {
            self.chain.open_when_ready = Some(device);
        }
    }

    pub(crate) fn add_utility(&mut self) {
        self.add_builtin(lumen_devices::utility::NAME);
    }

    pub(crate) fn add_eq(&mut self) {
        self.add_builtin(lumen_devices::eq::NAME);
    }

    pub(crate) fn add_builtin(&mut self, name: &str) {
        let plugin = PluginRef::Builtin {
            name: name.to_owned(),
        };
        let values = crate::builtin::defaults(name);
        let place = place(
            self.document.project(),
            &self.arrangement.selected,
            self.chain.selected,
            DeviceKind::Effect,
        );
        let result = match place {
            Place::Track { track, slot } => self
                .document
                .add_device_with(track, plugin, &values, slot)
                .map(|device| (track, device)),
            Place::NewTrack { after } => self
                .document
                .add_device_on_new_track_with(plugin, &values, after),
        };
        match result {
            Ok((track, device)) => {
                self.arrangement.selected = vec![track];
                self.chain.selected = Some(device);
                self.show_detail = true;
                self.detail_tab = DetailTab::Devices;
                self.chain.grow = true;
            }
            Err(refusal) => self.say(refusal.to_string()),
        }
    }

    pub(crate) fn open_dropped_editor(&mut self) {
        let Some(device) = self.chain.open_when_ready else {
            return;
        };
        let there =
            all_tracks(self.document.project()).any(|t| t.devices.iter().any(|d| d.id == device));
        match self.hosting.state(device) {
            Some(State::Ready) if there => {
                self.chain.open_when_ready = None;
                self.toggle_editor(device);
            }
            Some(State::Loading) | None if there => {}
            _ => self.chain.open_when_ready = None,
        }
    }

    pub(crate) fn add_device_at(&mut self, plugin: PluginRef, place: Place) -> Option<Id> {
        let result = match place {
            Place::Track { track, slot } => self
                .document
                .add_device(track, plugin, slot)
                .map(|device| (track, device)),
            Place::NewTrack { after } => self.document.add_device_on_new_track(plugin, after),
        };
        match result {
            Ok((track, device)) => {
                self.arrangement.selected = vec![track];
                self.chain.selected = Some(device);
                self.show_detail = true;
                self.detail_tab = DetailTab::Devices;
                self.chain.grow = true;
                Some(device)
            }
            Err(refusal) => {
                self.say(refusal.to_string());
                None
            }
        }
    }

    fn chosen_device(&self) -> Option<(Id, Id)> {
        let card = self.chain.selected?;
        let track =
            all_tracks(self.document.project()).find(|t| t.devices.iter().any(|d| d.id == card))?;
        Some((track.id, card))
    }

    pub(crate) fn device_chosen(&self) -> bool {
        self.chosen_device().is_some()
    }

    pub(crate) fn remove_chain_device(&mut self) {
        let Some((track, device)) = self.chosen_device() else {
            return;
        };
        let neighbour = all_tracks(self.document.project())
            .find(|t| t.id == track)
            .and_then(|t| {
                let at = t.devices.iter().position(|d| d.id == device)?;
                t.devices
                    .get(at + 1)
                    .or_else(|| at.checked_sub(1).and_then(|i| t.devices.get(i)))
                    .map(|d| d.id)
            });
        match self.document.remove_device(track, device) {
            Ok(()) => {
                self.chain.selected = neighbour;
                self.chain.deferred.retain(|(known, _), _| *known != device);
            }
            Err(refusal) => self.say(refusal.to_string()),
        }
    }

    pub(crate) fn toggle_chain_bypass(&mut self) {
        let Some((track, device)) = self.chosen_device() else {
            return;
        };
        let bypass = all_tracks(self.document.project())
            .find(|t| t.id == track)
            .and_then(|t| t.devices.iter().find(|d| d.id == device))
            .is_some_and(|d| d.bypass);
        self.bypass_device(track, device, !bypass);
    }

    pub(crate) fn bypass_device(&mut self, track: Id, device: Id, bypass: bool) {
        if let Err(refusal) = self.document.set_device_bypass(track, device, bypass) {
            self.say(refusal.to_string());
        }
    }

    pub(crate) fn move_chain_device(&mut self, step: isize) {
        let Some((track, device)) = self.chosen_device() else {
            return;
        };
        let devices = self.devices_of(track);
        let Some(at) = devices.iter().position(|d| *d == device) else {
            return;
        };
        let Some(to) = at.checked_add_signed(step).filter(|to| *to < devices.len()) else {
            return;
        };
        let gap = if step > 0 { to + 1 } else { to };
        let result = self
            .document
            .move_device(track, device, track, Slot::At(gap));
        self.say_refusal(result);
    }

    pub(crate) fn step_chain_choice(&mut self, step: isize) {
        let Some(track) = self.chain_track() else {
            return;
        };
        let devices = self.devices_of(track);
        let at = self
            .chain
            .selected
            .and_then(|c| devices.iter().position(|d| *d == c));
        let to = match at {
            Some(at) => at.checked_add_signed(step).filter(|to| *to < devices.len()),
            None if step > 0 => Some(0),
            None => devices.len().checked_sub(1),
        };
        if let Some(to) = to {
            self.chain.selected = Some(devices[to]);
        }
    }

    pub(crate) fn copy_chain_device(&mut self) {
        let Some((track, device)) = self.chosen_device() else {
            return;
        };
        let kept = all_tracks(self.document.project())
            .find(|t| t.id == track)
            .and_then(|t| t.devices.iter().find(|d| d.id == device))
            .cloned();
        if let Some(kept) = kept {
            let name = crate::document::device_name(&kept);
            self.chain.clipboard = Some(kept);
            self.say(format!("Copied {name}"));
        }
    }

    pub(crate) fn device_copied(&self) -> bool {
        self.chain.clipboard.is_some()
    }

    pub(crate) fn paste_chain_device(&mut self) {
        let (Some(device), Some(track)) = (self.chain.clipboard.clone(), self.chain_track()) else {
            return;
        };
        let slot = self
            .chain
            .selected
            .filter(|c| self.devices_of(track).contains(c))
            .map_or(Slot::End, Slot::After);
        let result = self.document.paste_device(&device, track, slot);
        self.inherit_strip(device.id, &result);
        self.chose_new(track, result);
    }

    pub(crate) fn duplicate_chain_device(&mut self) {
        let Some((track, device)) = self.chosen_device() else {
            return;
        };
        let result = self
            .document
            .copy_device(track, device, track, Slot::After(device));
        self.inherit_strip(device, &result);
        self.chose_new(track, result);
    }

    pub(crate) fn send_chain_device(&mut self, to: Id, copy: bool) {
        let Some((track, device)) = self.chosen_device() else {
            return;
        };
        self.send_device(track, device, to, Slot::End, copy);
    }

    pub(crate) fn send_device(&mut self, track: Id, device: Id, to: Id, slot: Slot, copy: bool) {
        if copy {
            let result = self.document.copy_device(track, device, to, slot);
            self.inherit_strip(device, &result);
            self.chose_new(to, result);
        } else {
            let result = self.document.move_device(track, device, to, slot);
            if result.is_ok() && to != track {
                self.arrangement.selected = vec![to];
                self.chain.selected = Some(device);
            }
            self.say_refusal(result);
        }
    }

    fn inherit_strip(&mut self, from: Id, copy: &Result<Id, Refusal>) {
        if let Ok(new) = copy
            && let Some(shown) = self.chain.shown.get(&from).cloned()
        {
            self.chain.shown.insert(*new, shown);
        }
    }

    pub(crate) fn toggle_chain_fold(&mut self) {
        if let Some(card) = self.chain.selected.filter(|_| self.device_chosen()) {
            self.toggle_fold_of(card);
        }
    }

    pub(crate) fn toggle_fold_of(&mut self, card: Id) {
        if !self.chain.folded.remove(&card) {
            self.chain.folded.insert(card);
        }
    }

    pub(crate) fn devices_of(&self, track: Id) -> Vec<Id> {
        all_tracks(self.document.project())
            .find(|t| t.id == track)
            .map(|t| t.devices.iter().map(|d| d.id).collect())
            .unwrap_or_default()
    }

    pub(crate) fn send_targets(&self) -> Vec<(Id, String)> {
        let Some((track, device)) = self.chosen_device() else {
            return Vec::new();
        };
        let project = self.document.project();
        let instrument = all_tracks(project)
            .find(|t| t.id == track)
            .and_then(|t| t.devices.iter().find(|d| d.id == device))
            .is_some_and(|d| d.is_instrument());
        all_tracks(project)
            .filter(|t| !instrument || t.kind == lumen_project::model::TrackKind::Midi)
            .map(|t| (t.id, t.name.clone()))
            .collect()
    }

    fn say_refusal(&mut self, result: Result<(), Refusal>) {
        if let Err(refusal) = result {
            self.say(refusal.to_string());
        }
    }

    fn chose_new(&mut self, track: Id, result: Result<Id, Refusal>) {
        match result {
            Ok(device) => {
                self.arrangement.selected = vec![track];
                self.chain.selected = Some(device);
            }
            Err(refusal) => self.say(refusal.to_string()),
        }
    }

    pub(crate) fn say(&mut self, words: String) {
        self.toast = Some((words, Instant::now()));
    }
}

#[cfg(test)]
mod tests {
    use lumen_plugin_catalog::catalog::builtin_devices;

    use super::*;

    #[test]
    fn the_built_in_synth_is_named_and_a_plugin_keeps_its_class_and_kind() {
        let synth = builtin_devices().remove(0);
        assert_eq!(
            plugin_ref(&synth),
            Some(PluginRef::Builtin { name: SYNTH.into() })
        );
        let listed = Listed {
            id: "vst3:ABCDEF0123456789ABCDEF0123456789".into(),
            name: "Tape Delay".into(),
            vendor: "Vendor D".into(),
            kind: Kind::Effect,
            source: Source::Vst3,
            categories: Vec::new(),
            version: String::new(),
            module: None,
        };
        assert_eq!(
            plugin_ref(&listed),
            Some(PluginRef::Vst3 {
                class_id: "ABCDEF0123456789ABCDEF0123456789".into(),
                name: "Tape Delay".into(),
                vendor: "Vendor D".into(),
                kind: DeviceKind::Effect,
            })
        );
    }
}
