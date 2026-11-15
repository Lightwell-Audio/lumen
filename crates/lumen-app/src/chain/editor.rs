use std::time::Instant;

use lumen_hosting::{Edit, EditKind, EditorBounds};
use lumen_project::model::Id;
use lumen_project::routing::all_tracks;

use super::strip::{self, SLOTS};
use crate::App;
use crate::hosting::EditorState;

impl App {
    pub(super) fn editor_look(&self, device: &lumen_project::model::Device) -> strip::Editor {
        match self.hosting.editor(device.id) {
            EditorState::Closed => strip::Editor::Closed,
            EditorState::Opening => strip::Editor::Opening,
            EditorState::Open => strip::Editor::Open,
            EditorState::NotResponding => strip::Editor::NotResponding,
            EditorState::Refused(why) => strip::Editor::Unavailable(why),
        }
    }

    pub(crate) fn toggle_editor(&mut self, device: Id) {
        match self.hosting.editor(device) {
            EditorState::Closed => self.open_editor(device),
            EditorState::Open => {
                self.hosting.close_editor(device);
                if self.chain.learning == Some(device) {
                    self.chain.learning = None;
                }
            }
            _ => {}
        }
    }

    fn open_editor(&mut self, device: Id) {
        let place = self
            .hosting
            .class(device)
            .and_then(|class| self.settings.plugins.editors.get(class))
            .copied()
            .map(EditorBounds::from);
        self.hosting.open_editor(device, place);
    }

    pub(crate) fn toggle_learn(&mut self, device: Id) {
        if self.chain.learning == Some(device) {
            self.chain.learning = None;
            return;
        }
        if self.strip_ids(device).len() >= SLOTS {
            self.say("The strip is full. Take one off first.".to_owned());
            return;
        }
        self.chain.learning = Some(device);
        if matches!(self.hosting.editor(device), EditorState::Closed) {
            self.open_editor(device);
        }
    }

    pub(crate) fn follow_chosen_track(&mut self) {
        let project = self.document.project();
        let shown: Vec<Id> = self
            .chain_track()
            .and_then(|track| all_tracks(project).find(|t| t.id == track))
            .map(|t| t.devices.iter().map(|d| d.id).collect())
            .unwrap_or_default();
        for hidden in self.hosting.show_editors_of(&shown) {
            if self.chain.learning == Some(hidden) {
                self.chain.learning = None;
            }
        }
    }

    pub(crate) fn end_editor_plugin(&mut self, device: Id) {
        self.hosting.end_plugin(device);
        if self.chain.learning == Some(device) {
            self.chain.learning = None;
        }
    }

    pub(crate) fn follow_editors(
        &mut self,
        ctx: &eframe::egui::Context,
        touched: &[(Id, Vec<u32>)],
    ) {
        let mut changed = false;
        for (class, place) in self.hosting.take_editor_places() {
            changed |= self.settings.plugins.remember_editor(&class, place.into());
        }
        if changed {
            self.settings_changed = Some(Instant::now());
        }
        let Some(device) = self.chain.learning else {
            return;
        };
        let open = matches!(
            self.hosting.editor(device),
            EditorState::Opening | EditorState::Open | EditorState::NotResponding
        );
        if !open || (!self.typing && ctx.input(|i| i.key_pressed(eframe::egui::Key::Escape))) {
            self.chain.learning = None;
            return;
        }
        for (touched_device, ids) in touched {
            if *touched_device != device {
                continue;
            }
            for &id in ids {
                if !self.learn(device, id) {
                    self.chain.learning = None;
                    return;
                }
            }
        }
    }

    fn learn(&mut self, device: Id, parameter: u32) -> bool {
        let mut ids = self.strip_ids(device);
        if ids.contains(&parameter) {
            return true;
        }
        let offered = self
            .hosting
            .parameters(device)
            .iter()
            .any(|p| p.id == parameter && strip::offered(p));
        if !offered {
            return true;
        }
        if ids.len() >= SLOTS {
            self.say("The strip is full. Learn is off.".to_owned());
            return false;
        }
        ids.push(parameter);
        let full = ids.len() >= SLOTS;
        self.chain.shown.insert(device, ids);
        if full {
            self.say("The strip is full. Learn is off.".to_owned());
        }
        !full
    }
}

pub(crate) fn touched(edits: &[(Edit, bool)]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for (edit, _) in edits {
        if matches!(edit.kind, EditKind::Begin | EditKind::Perform) && !out.contains(&edit.id) {
            out.push(edit.id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(id: u32, kind: EditKind) -> (Edit, bool) {
        (
            Edit {
                id,
                value: 0.5,
                kind,
            },
            true,
        )
    }

    #[test]
    fn only_what_a_person_touched_is_learned_once_each() {
        let edits = [
            edit(3, EditKind::Begin),
            edit(3, EditKind::Perform),
            edit(3, EditKind::End),
            edit(9, EditKind::Output),
            edit(5, EditKind::Perform),
            edit(3, EditKind::Perform),
        ];
        assert_eq!(touched(&edits), [3, 5]);
        assert!(touched(&[]).is_empty());
    }
}
