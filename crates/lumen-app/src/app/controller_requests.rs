//! What the controls' menus ask of MIDI learn, answered after the frame (M7
//! slice A5), and the note that stays up while a control waits for a
//! controller.

use eframe::egui;
use lumen_project::automation::Target;
use lumen_project::model::Id;

use crate::App;
use crate::arrangement::auto_state::MapRequest;

impl App {
    /// Takes the things the controls' menus asked for during the frame.
    pub(crate) fn answer_map_requests(&mut self) {
        for request in std::mem::take(&mut self.arrangement.auto.requests) {
            match request {
                MapRequest::Learn(track, target) => self.start_learning(track, target),
                MapRequest::StopLearning => {
                    self.stop_learning();
                }
                MapRequest::Clear(track, target) => self.clear_controller(track, target),
                MapRequest::Window => self.controllers.window = true,
            }
        }
        self.arrangement.auto.learning = self.controllers.learning;
    }

    /// Takes away the controller that moves `track`'s `target`, and says so.
    pub(crate) fn clear_controller(&mut self, track: Id, target: Target) {
        if self.document.clear_mapping(track, target) {
            let what = self.control_name(track, target);
            self.say(format!("{what} no longer has a controller"));
        }
    }

    /// While a control waits for a controller, the note stays up and Esc
    /// ends the wait. A control that has gone is not waited for.
    pub(crate) fn keep_learning_note(&mut self, ctx: &egui::Context) {
        let Some((track, target)) = self.controllers.learning else {
            return;
        };
        let gone = lumen_project::routing::all_tracks(self.document.project())
            .find(|t| t.id == track)
            .is_none_or(|t| !t.has_target(target));
        if gone {
            self.controllers.learning = None;
            return;
        }
        if !self.typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.controllers.learning = None;
            self.say("Stopped waiting for a controller".to_owned());
            return;
        }
        let what = self.control_name(track, target);
        self.say(format!(
            "Move a controller to map {what} \u{b7} Esc to stop"
        ));
    }
}
