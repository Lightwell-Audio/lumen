use super::*;

impl Hosting {
    pub fn editor(&self, device: Id) -> EditorState {
        self.tracked
            .get(&device.0)
            .map(|t| t.editor.clone())
            .unwrap_or_default()
    }

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

    pub fn editor_hidden(&self, device: Id) -> bool {
        self.tracked.get(&device.0).is_some_and(|t| t.editor_hidden)
    }

    pub fn close_editor(&mut self, device: Id) {
        if let Some(manager) = self.manager.as_ref() {
            manager.close_editor(device.0);
        }
    }

    pub fn end_plugin(&mut self, device: Id) {
        if let Some(manager) = self.manager.as_ref() {
            manager.end(device.0);
        }
    }

    pub fn take_editor_places(&mut self) -> Vec<(String, EditorBounds)> {
        std::mem::take(&mut self.places)
    }

    pub fn capture_keys(&mut self, keys: Vec<u8>) {
        if let Some(manager) = self.manager.as_mut() {
            manager.capture_keys(keys);
        }
    }

    pub fn take_keys(&mut self) -> Vec<(u8, bool)> {
        std::mem::take(&mut self.keys)
    }
}
