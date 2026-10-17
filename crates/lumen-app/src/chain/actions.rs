//! What the browser's choice and the three device commands do to the
//! project: the placement rule (`place`) applied through the `Document`'s
//! edits, one undo step each, and the interface following (the new card
//! chosen, the tab shown). A refusal says why in a toast.

use std::time::Instant;

use lumen_plugin_catalog::catalog::{Device as Listed, Kind, Source};
use lumen_project::model::{DeviceKind, Id, PluginRef, SYNTH};

use super::place::{Place, place};
use lumen_project::routing::all_tracks;

use crate::App;
use crate::app::detail::DetailTab;
use crate::document::{Refusal, Slot};

/// The project's name for a device the browser lists; `None` for one that
/// cannot be put on a track.
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
    /// The track the Devices tab shows: the last one selected.
    pub(crate) fn chain_track(&self) -> Option<Id> {
        let project = self.document.project();
        self.arrangement
            .selected
            .iter()
            .rev()
            .copied()
            .find(|id| all_tracks(project).any(|t| t.id == *id))
    }

    /// Double-click or Enter in the browser: puts the device on the selected
    /// track, or makes a track for it (D3), and shows the chain.
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

    /// Applies `place` for `plugin`.
    pub(crate) fn add_device_at(&mut self, plugin: PluginRef, place: Place) {
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
            }
            Err(refusal) => self.say(refusal.to_string()),
        }
    }

    /// The chosen card's track and id, if it is still there.
    fn chosen_device(&self) -> Option<(Id, Id)> {
        let card = self.chain.selected?;
        let track =
            all_tracks(self.document.project()).find(|t| t.devices.iter().any(|d| d.id == card))?;
        Some((track.id, card))
    }

    /// Whether a device is chosen (for the commands' availability).
    pub(crate) fn device_chosen(&self) -> bool {
        self.chosen_device().is_some()
    }

    /// Takes the chosen device off its track, and chooses its neighbour.
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
            Ok(()) => self.chain.selected = neighbour,
            Err(refusal) => self.say(refusal.to_string()),
        }
    }

    /// Bypasses the chosen device, or turns it on again.
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

    /// Bypasses or enables `device` on `track`.
    pub(crate) fn bypass_device(&mut self, track: Id, device: Id, bypass: bool) {
        if let Err(refusal) = self.document.set_device_bypass(track, device, bypass) {
            self.say(refusal.to_string());
        }
    }

    /// Moves the chosen device one place along its chain (`-1` earlier, `1`
    /// later). At either end nothing happens.
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
        // A gap counts the chain as it stands: past the neighbour is its
        // far side.
        let gap = if step > 0 { to + 1 } else { to };
        let result = self
            .document
            .move_device(track, device, track, Slot::At(gap));
        self.say_refusal(result);
    }

    /// Chooses the card before (`-1`) or after (`1`) the chosen one; with
    /// none chosen, the first or last.
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

    /// Keeps the chosen device's settings for a Paste.
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

    /// Whether a device was copied.
    pub(crate) fn device_copied(&self) -> bool {
        self.chain.clipboard.is_some()
    }

    /// Puts the copied device on the shown track, after the chosen card.
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
        self.chose_new(track, result);
    }

    /// Copies the chosen device right after itself.
    pub(crate) fn duplicate_chain_device(&mut self) {
        let Some((track, device)) = self.chosen_device() else {
            return;
        };
        let result = self
            .document
            .copy_device(track, device, track, Slot::After(device));
        self.chose_new(track, result);
    }

    /// Moves (`copy` false) or copies the chosen device to the end of another
    /// track's chain, and shows that track.
    pub(crate) fn send_chain_device(&mut self, to: Id, copy: bool) {
        let Some((track, device)) = self.chosen_device() else {
            return;
        };
        self.send_device(track, device, to, Slot::End, copy);
    }

    /// Moves or copies `device` from `track` to `slot` on `to`; the interface
    /// follows it there.
    pub(crate) fn send_device(&mut self, track: Id, device: Id, to: Id, slot: Slot, copy: bool) {
        if copy {
            let result = self.document.copy_device(track, device, to, slot);
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

    /// Folds the chosen card, or opens it again.
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

    /// The ids of a track's devices, in order.
    pub(crate) fn devices_of(&self, track: Id) -> Vec<Id> {
        all_tracks(self.document.project())
            .find(|t| t.id == track)
            .map(|t| t.devices.iter().map(|d| d.id).collect())
            .unwrap_or_default()
    }

    /// The tracks that can take the chosen device, as (id, name): any, the
    /// master too, and only MIDI tracks for an instrument.
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

    /// Says why an edit was refused.
    fn say_refusal(&mut self, result: Result<(), Refusal>) {
        if let Err(refusal) = result {
            self.say(refusal.to_string());
        }
    }

    /// After an edit that made a device on `track`: shows it and chooses it.
    fn chose_new(&mut self, track: Id, result: Result<Id, Refusal>) {
        match result {
            Ok(device) => {
                self.arrangement.selected = vec![track];
                self.chain.selected = Some(device);
            }
            Err(refusal) => self.say(refusal.to_string()),
        }
    }

    /// A short note in the toast.
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
