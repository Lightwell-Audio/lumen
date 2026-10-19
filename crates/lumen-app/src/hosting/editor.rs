//! The plugin's own window, as the app tracks it (M6 slice 4f): opening, hiding
//! with the track, closing, the places remembered per plugin, and the computer
//! keyboard's keys pressed inside it.

use super::*;

impl Hosting {
    /// What the device's plugin window is doing.
    pub fn editor(&self, device: Id) -> EditorState {
        self.tracked
            .get(&device.0)
            .map(|t| t.editor.clone())
            .unwrap_or_default()
    }

    /// Opens the device's plugin window (at `place` if the plugin's was
    /// remembered). The news comes back as [`Hosting::editor`] changing.
    pub fn open_editor(&mut self, device: Id, place: Option<EditorBounds>) {
        let Some(t) = self.tracked.get_mut(&device.0) else {
            return;
        };
        if !matches!(t.state, State::Ready) || !matches!(t.editor, EditorState::Closed) {
            return;
        }
        t.editor = EditorState::Opening;
        if let Some(manager) = self.manager.as_ref() {
            manager.open_editor(device.0, place);
        }
    }

    /// Shows the plugin windows of `devices` and hides every other that is
    /// open (the user is on a track that has them or not), so a window is
    /// only on screen with its own track. Only a change is sent: a window
    /// still being made is dealt with once it is up. Returns the devices
    /// whose windows were hidden by this call.
    pub fn show_editors_of(&mut self, devices: &[Id]) -> Vec<Id> {
        let mut hidden_now = Vec::new();
        for (&id, t) in &mut self.tracked {
            if !matches!(t.editor, EditorState::Open | EditorState::NotResponding) {
                continue;
            }
            let hide = !devices.contains(&Id(id));
            if t.editor_hidden == hide {
                continue;
            }
            t.editor_hidden = hide;
            if let Some(manager) = self.manager.as_ref() {
                manager.show_editor(id, !hide);
            }
            if hide {
                hidden_now.push(Id(id));
            }
        }
        hidden_now
    }

    /// Whether the device's plugin window is hidden while open.
    pub fn editor_hidden(&self, device: Id) -> bool {
        self.tracked.get(&device.0).is_some_and(|t| t.editor_hidden)
    }

    /// Closes the device's plugin window.
    pub fn close_editor(&mut self, device: Id) {
        if let Some(manager) = self.manager.as_ref() {
            manager.close_editor(device.0);
        }
    }

    /// Ends the host of a plugin whose window stopped answering. The device
    /// then reads as failed, with Reload.
    pub fn end_plugin(&mut self, device: Id) {
        if let Some(manager) = self.manager.as_ref() {
            manager.end(device.0);
        }
    }

    /// Where plugin windows were last seen since the last call, by plugin
    /// class, for the app to remember.
    pub fn take_editor_places(&mut self) -> Vec<(String, EditorBounds)> {
        std::mem::take(&mut self.places)
    }

    /// Has every plugin's host take `keys` (virtual-key codes) from its
    /// window when pressed alone, and give the rest to the plugin; empty
    /// gives them all back. Cheap when nothing changes.
    pub fn capture_keys(&mut self, keys: Vec<u8>) {
        if let Some(manager) = self.manager.as_mut() {
            manager.capture_keys(keys);
        }
    }

    /// The keys pressed (`true`) and let go of in plugins' windows since the
    /// last call, oldest first.
    pub fn take_keys(&mut self) -> Vec<(u8, bool)> {
        std::mem::take(&mut self.keys)
    }
}
