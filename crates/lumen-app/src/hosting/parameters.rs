//! A plugin's parameters as the bridge keeps them (M6 slice 4, ADR-0076):
//! the lists, values and text, typed values, the plugin's own edits, and what
//! differs between the project and the plugin.

use super::*;

impl Hosting {
    /// The plugin's parameters as listed when it loaded (empty until it has).
    pub fn parameters(&self, device: Id) -> &[ParamInfo] {
        self.tracked
            .get(&device.0)
            .map_or(&[], |t| t.params.list.as_slice())
    }

    /// The groups the plugin's parameters sit in.
    pub fn units(&self, device: Id) -> &[UnitInfo] {
        self.tracked
            .get(&device.0)
            .map_or(&[], |t| t.params.units.as_slice())
    }

    /// A parameter's value as far as Lumen knows (normalized).
    pub fn value(&self, device: Id, id: u32) -> Option<f64> {
        self.tracked.get(&device.0)?.params.values.get(&id).copied()
    }

    /// Asks the plugin for its own text for `value` of a parameter; the answer
    /// is [`Hosting::text`] a moment later.
    pub fn ask_text(&self, device: Id, id: u32, value: f64) {
        if let Some(manager) = self.manager.as_ref() {
            manager.param_text(device.0, id, value);
        }
    }

    /// Sends a parameter value straight to the device's plugin, as its own
    /// window would deliver it, without an edit of the project. For tests.
    pub fn poke_param(&self, device: Id, id: u32, value: f64) {
        if let Some(manager) = self.manager.as_ref() {
            manager.set_param(device.0, id, value);
        }
    }

    /// Asks the plugin to read `text` the user typed for a parameter; the
    /// answer is in [`Hosting::take_typed`] a moment later.
    pub fn ask_value(&self, device: Id, id: u32, text: String) {
        if let Some(manager) = self.manager.as_ref() {
            manager.param_value(device.0, id, text);
        }
    }

    /// What plugins made of the text typed for them since the last call, in
    /// the order they answered.
    pub fn take_typed(&mut self) -> Vec<Typed> {
        let mut out = Vec::new();
        let mut devices: Vec<u64> = self
            .tracked
            .iter()
            .filter(|(_, t)| !t.params.typed.is_empty())
            .map(|(&id, _)| id)
            .collect();
        devices.sort_unstable();
        for id in devices {
            if let Some(t) = self.tracked.get_mut(&id) {
                for (parameter, text, value) in std::mem::take(&mut t.params.typed) {
                    out.push(Typed {
                        device: Id(id),
                        id: parameter,
                        text,
                        value,
                    });
                }
            }
        }
        out
    }

    /// The plugin's text for the value it was last asked about, and that value.
    pub fn text(&self, device: Id, id: u32) -> Option<&(f64, String)> {
        self.tracked.get(&device.0)?.params.texts.get(&id)
    }

    /// The plugin's own word for each step of a stepped parameter, as far as
    /// it has said them.
    pub fn choices(&self, device: Id, id: u32) -> &[String] {
        self.tracked
            .get(&device.0)
            .and_then(|t| t.params.choices.get(&id))
            .map_or(&[], Vec::as_slice)
    }

    /// Asks the plugin for the word of every step of the stepped parameters
    /// `ids` (once each), for the chooser's list.
    pub fn ask_choices(&mut self, device: Id, ids: &[u32]) {
        let (Some(manager), Some(t)) = (self.manager.as_ref(), self.tracked.get_mut(&device.0))
        else {
            return;
        };
        for &id in ids {
            let Some(steps) = t
                .params
                .list
                .iter()
                .find(|i| i.id == id)
                .map(|i| i.steps)
                .filter(|s| (2..=MOST_CHOICES).contains(s))
            else {
                continue;
            };
            if t.params.choices_asked.insert(id) {
                let values = (0..=steps)
                    .map(|step| f64::from(step) / f64::from(steps))
                    .collect();
                manager.param_texts(device.0, id, values);
            }
        }
    }

    /// How many values Lumen has sent the device's plugin to match the project.
    pub fn values_sent(&self, device: Id) -> u32 {
        self.tracked.get(&device.0).map_or(0, |t| t.params.sent)
    }

    /// The edits plugins made inside themselves since the last call, oldest
    /// first, for the document to record. Their values are already known here,
    /// so the document's matching change sends nothing back.
    pub fn take_edits(&mut self) -> Vec<DeviceEdits> {
        let mut out: Vec<DeviceEdits> = self
            .tracked
            .iter_mut()
            .filter(|(_, t)| !t.params.edits.is_empty())
            .map(|(&id, t)| DeviceEdits {
                device: Id(id),
                edits: std::mem::take(&mut t.params.edits),
            })
            .collect();
        out.sort_by_key(|e| e.device);
        out
    }

    /// Whether a plugin has a gesture open (a control held in its window): the
    /// document keeps the undo step open until it ends.
    pub fn gesture_open(&self) -> bool {
        self.tracked.values().any(|t| t.params.gesture_is_open())
    }
}
