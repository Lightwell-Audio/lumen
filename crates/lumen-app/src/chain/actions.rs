//! What the browser's choice and the three device commands do to the
//! project: the placement rule (`place`) applied through the `Document`'s
//! edits, one undo step each, and the interface following (the new card
//! chosen, the tab shown). A refusal says why in a toast.

use std::time::Instant;

use lumen_plugin_catalog::catalog::{Device as Listed, Kind, Source};
use lumen_project::model::{DeviceKind, Id, PluginRef, SYNTH};

use super::place::{Place, place};
use crate::App;
use crate::app::detail::DetailTab;

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
            .find(|id| project.tracks.iter().any(|t| t.id == *id))
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
        let track = self
            .document
            .project()
            .tracks
            .iter()
            .find(|t| t.devices.iter().any(|d| d.id == card))?;
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
        let neighbour = self
            .document
            .project()
            .tracks
            .iter()
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
        let bypass = self
            .document
            .project()
            .tracks
            .iter()
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
