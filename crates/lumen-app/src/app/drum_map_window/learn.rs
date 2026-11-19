use eframe::egui;
use lumen_project::model::Id;

use super::{Notice, Open};
use crate::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Column {
    Input,
    Output,
}

impl Column {
    pub(crate) fn tooltip(self) -> &'static str {
        match self {
            Column::Input => "Learn the key you play (sets the input note)",
            Column::Output => "Learn the instrument's note (sets the output note)",
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Column::Input => "input",
            Column::Output => "output",
        }
    }

    fn waiting(self) -> &'static str {
        match self {
            Column::Input => {
                "Waiting for a note: play it on a MIDI input, and it becomes the note this sound answers to"
            }
            Column::Output => {
                "Listening to the instrument: play keys, and the instrument sounds on each one as it is, without the map. The first note you play becomes the note this sound sends. Esc stops"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Learn {
    pub input: u8,
    pub column: Column,
}

pub(super) fn pressed(current: Option<Learn>, pressed: Learn) -> Option<Learn> {
    if current == Some(pressed) {
        None
    } else {
        Some(pressed)
    }
}

pub(super) fn notice_for(learn: Option<Learn>) -> Notice {
    Notice::good(match learn {
        Some(learn) => learn.column.waiting(),
        None => "No longer listening for a note",
    })
}

impl super::State {
    pub(crate) fn learning(&self) -> Option<(Id, Learn)> {
        let open = self.open.as_ref()?;
        Some((open.map?, open.learn?))
    }

    pub(crate) fn plays_the_instrument_as_it_is(&self) -> bool {
        self.learning()
            .is_some_and(|(_, learn)| learn.column == Column::Output)
    }
}

impl App {
    pub(crate) fn drum_map_heard(&mut self, key: u8) {
        let Some((map, learn)) = self.drum_map.learning() else {
            return;
        };
        match learn.column {
            Column::Input => self.drum_map_heard_input(map, learn.input, key),
            Column::Output => self.drum_map_heard_output(map, learn.input, key),
        }
    }

    fn drum_map_heard_input(&mut self, map: Id, input: u8, key: u8) {
        if let Some(open) = &mut self.drum_map.open {
            open.learn = None;
        }
        if input == key {
            self.say(format!(
                "That sound already answers to note {key}; play another note"
            ));
            return;
        }
        if self.document.set_sound_input(map, input, key) {
            self.say(format!(
                "The sound now answers to note {key}, one undo step"
            ));
        } else {
            self.say(format!(
                "Another sound already answers to note {key}, so nothing changed"
            ));
        }
    }

    fn drum_map_heard_output(&mut self, map: Id, input: u8, key: u8) {
        let Some(sound) = self
            .document
            .drum_map(map)
            .and_then(|m| m.sound(input))
            .map(|s| (s.name.clone(), s.output))
        else {
            return;
        };
        let (name, was) = sound;
        if was == key {
            self.say(format!(
                "{name} already sends note {key} to the instrument; play another note"
            ));
            return;
        }
        if let Some(open) = &mut self.drum_map.open {
            open.learn = None;
        }
        if self.document.learn_sound_output(map, input, key) {
            self.say(format!(
                "{name} now sends note {key} to the instrument instead of {was}, one undo step"
            ));
        }
    }

    pub(super) fn drum_map_escape(&self, open: &mut Open, ctx: &egui::Context) -> bool {
        let learning = open.learn.is_some();
        if learning && !self.typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open.learn = None;
            open.notice = Some(notice_for(None));
        }
        learning
    }

    pub(crate) fn sync_live_notes_as_played(&self) {
        let Some(tap) = &self.controllers.tap else {
            return;
        };
        let want = self.drum_map.plays_the_instrument_as_it_is();
        if tap.raw() != want {
            tap.set_raw(want);
        }
    }
}
