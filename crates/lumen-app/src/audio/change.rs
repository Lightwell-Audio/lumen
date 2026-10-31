//! A device change on a worker thread (ADR-0113): the device lookup and,
//! between driver models, `AudioSystem::switch` with its handover. A driver
//! can take seconds to let go of the audio, or never answer at all, so none
//! of it runs on the interface thread. The worker owns the `AudioSystem`
//! while it switches, and its answers are read each frame
//! (`Audio::poll`). Nothing waits for it: quitting leaves it behind.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use lumen_audio::{AudioSystem, SwitchError};

use lumen_core::product::PRODUCT_NAME;

use super::{Audio, NOT_ANSWERING, Previous, Resolved, Status, driver_name, find_saved};
use crate::settings::AudioSettings;

/// What the worker is asked to open.
pub(super) enum Target {
    /// Settings chosen in Preferences: looked up first.
    Settings(AudioSettings),
    /// A device already found: the setup before a failed switch.
    Found(Resolved),
}

/// The step the worker is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// Listing the driver's devices to find the one chosen.
    Looking,
    /// Waiting for the old driver's thread to give the audio back (it has
    /// its own deadline, `lumen_audio::HANDOVER_TIMEOUT`).
    HandingOver,
    /// The audio is back; waiting for the old driver to close.
    Closing,
}

/// What the worker sends back.
pub(super) enum Answer {
    Step(Step),
    /// The device chosen is not there; the system comes back untouched.
    Missing(Option<AudioSystem>),
    /// Found on the driver model in use (or with no system yet): the
    /// interface thread opens it, which does not wait.
    Found(Resolved),
    /// Switched: the system runs the new driver and was asked to open it.
    Switched(Resolved, AudioSystem),
    /// The switch failed. The worker keeps the system: after
    /// `NoHandover`, dropping it waits for the old driver.
    Failed(SwitchError),
    /// The worker ended without answering (it panicked), and took the
    /// system it held with it.
    Gone,
}

/// A change in progress, as the interface thread sees it.
pub(super) struct Change {
    answers: Receiver<Answer>,
    /// Answers read and not applied yet ([`Change::switched_unread`]).
    unread: VecDeque<Answer>,
    pub step: Step,
    /// When the current step began.
    pub since: Instant,
    /// The settings asked for; `None` when going back to the setup before.
    pub settings: Option<AudioSettings>,
    /// The setup to go back to if the new one fails.
    pub previous: Option<Previous>,
    /// The device being started, and the driver being left and the one
    /// started, as messages name them.
    pub device: String,
    pub from: String,
    pub to: String,
}

/// Who a change is for, as its messages name them.
pub(super) struct Names {
    pub device: String,
    pub from: String,
    pub to: String,
}

impl Change {
    /// Starts the worker on `target`. With `system`, it switches that
    /// system to the target's driver model.
    pub fn start(
        target: Target,
        system: Option<AudioSystem>,
        settings: Option<AudioSettings>,
        previous: Option<Previous>,
        names: Names,
    ) -> Self {
        let (tx, answers) = mpsc::channel();
        std::thread::Builder::new()
            .name("lumen-audio-change".into())
            .spawn(move || run(target, system, &tx))
            .expect("spawn the audio change thread");
        Self {
            answers,
            unread: VecDeque::new(),
            step: Step::Looking,
            since: Instant::now(),
            settings,
            previous,
            device: names.device,
            from: names.from,
            to: names.to,
        }
    }

    /// The worker's next answer, without waiting.
    pub fn answer(&mut self) -> Option<Answer> {
        if let Some(answer) = self.unread.pop_front() {
            return Some(answer);
        }
        match self.answers.try_recv() {
            Ok(answer) => Some(answer),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Answer::Gone),
        }
    }

    /// Whether the worker has switched and that answer waits, unapplied.
    /// Reads what the worker sent and keeps it to be applied as usual. For
    /// tests that quit with the answer unread.
    pub fn switched_unread(&mut self) -> bool {
        while let Ok(answer) = self.answers.try_recv() {
            self.unread.push_back(answer);
        }
        self.unread
            .iter()
            .any(|answer| matches!(answer, Answer::Switched(..)))
    }
}

/// The worker. A send that fails means the interface has gone (Lumen is
/// quitting): what it carried is dropped here, where waiting is harmless.
fn run(target: Target, system: Option<AudioSystem>, tx: &Sender<Answer>) {
    let found = match target {
        Target::Settings(settings) => match find_saved(&settings) {
            Some(found) => found,
            None => {
                let _ = tx.send(Answer::Missing(system));
                return;
            }
        },
        Target::Found(found) => found,
    };
    let Some(mut system) = system else {
        let _ = tx.send(Answer::Found(found));
        return;
    };
    let _ = tx.send(Answer::Step(Step::HandingOver));
    let switched = system.switch(found.make, found.config(), || {
        let _ = tx.send(Answer::Step(Step::Closing));
    });
    match switched {
        Ok(()) => {
            let _ = tx.send(Answer::Switched(found, system));
        }
        Err(error) => {
            let _ = tx.send(Answer::Failed(error));
            // After `NoHandover` this waits for the old driver's thread,
            // perhaps for ever: on this thread, nobody waits with it.
            drop(system);
        }
    }
}

impl Audio {
    /// Uses `settings` (from the audio settings window). Saved once the
    /// device starts; on failure the reason shows and nothing is saved.
    /// The lookup, and a switch of driver model, run on a worker
    /// (ADR-0113); the answer comes through [`Audio::poll`]. Refused, and
    /// false, while another change runs ([`Audio::can_change`]).
    pub fn apply(&mut self, settings: AudioSettings) -> bool {
        if !self.can_change() {
            return false;
        }
        let previous = self.config.clone().map(|config| Previous {
            driver: self.driver.clone(),
            device: self.device.clone(),
            config,
            cue_channels: self.cue_channels.clone(),
            extra_channels: self.extra_channels.clone(),
            info: self.info.clone(),
            status: self.status.clone(),
        });
        let names = Names {
            device: settings.device_name.clone(),
            from: driver_name(&self.driver, &self.device),
            to: driver_name(&settings.driver, &settings.device_name),
        };
        // Only a switch of driver model takes the system to the worker.
        let system = if settings.driver == self.driver {
            None
        } else {
            self.system.take()
        };
        self.change = Some(Change::start(
            Target::Settings(settings.clone()),
            system,
            Some(settings),
            previous,
            names,
        ));
        true
    }

    /// Whether the device can be changed now: no change is running or
    /// waiting for the device to answer, and the engine is here to run.
    /// Only one change runs at a time; the Audio page's device controls are
    /// disabled meanwhile.
    pub fn can_change(&self) -> bool {
        self.change.is_none()
            && !self.awaiting()
            && (self.system.is_some() || self.parked.is_some())
    }

    /// A change is under way: the interface checks on it often.
    pub fn waiting(&self) -> bool {
        self.change.is_some() || self.awaiting()
    }

    /// The device was asked to open and has not answered yet.
    pub(super) fn awaiting(&self) -> bool {
        self.asked.is_some() && matches!(self.status, Status::Starting)
    }

    /// Says so once a driver has taken [`NOT_ANSWERING`] to answer: one
    /// whose devices are being listed, the one left while it closes, or the
    /// one asked to open. The handover has its own deadline and message.
    pub fn not_answering(&self) -> Option<String> {
        let (since, who) = match &self.change {
            Some(change) => match change.step {
                Step::Looking => (change.since, &change.to),
                Step::HandingOver => return None,
                Step::Closing => (change.since, &change.from),
            },
            None if self.awaiting() => {
                let (since, who) = self.asked.as_ref()?;
                (*since, who)
            }
            None => return None,
        };
        (since.elapsed() >= NOT_ANSWERING)
            .then(|| format!("{who} is not answering. Save your work and restart {PRODUCT_NAME}."))
    }

    /// Whether a switch has finished and its answer waits, unread (tests).
    pub(crate) fn switched_unread(&mut self) -> bool {
        self.change.as_mut().is_some_and(Change::switched_unread)
    }

    /// Reads the change's worker: its steps, then its answer.
    pub(super) fn poll_change(&mut self) -> bool {
        let mut changed = false;
        while let Some(answer) = self.change.as_mut().and_then(Change::answer) {
            changed = true;
            if let Answer::Step(step) = answer {
                if let Some(change) = &mut self.change {
                    change.step = step;
                    change.since = Instant::now();
                }
                // The old driver is closing: audio is off until the new one
                // starts.
                self.status = Status::Starting;
                continue;
            }
            let Some(change) = self.change.take() else {
                break;
            };
            self.finish(change, answer);
        }
        changed
    }

    /// Applies the worker's answer to `change`.
    fn finish(&mut self, change: Change, answer: Answer) {
        let adopt = |audio: &mut Self| {
            // Going back to the setup before keeps the notice saying why.
            if change.settings.is_some() {
                audio.dismiss_notice();
                audio.previous = change.previous;
                audio.pending = change.settings;
            }
        };
        match answer {
            Answer::Step(_) => {}
            Answer::Missing(system) => {
                if system.is_some() {
                    self.system = system;
                }
                self.notice = Some(format!("{} is not available", change.device));
            }
            Answer::Found(found) => {
                let from = change.from.clone();
                adopt(self);
                self.open(found, from);
            }
            Answer::Switched(found, system) => {
                self.system = Some(system);
                adopt(self);
                self.opened(found);
            }
            Answer::Failed(SwitchError::NoHandover) => {
                self.lost_for_this_run(format!(
                    "{} did not let go of the audio. Save your work and restart {PRODUCT_NAME}.",
                    change.from
                ));
            }
            Answer::Failed(error @ SwitchError::Stopped) => {
                self.lost_for_this_run(format!("Could not switch the audio driver: {error}"));
            }
            Answer::Gone if self.system.is_some() || self.parked.is_some() => {
                self.notice = Some(format!("{} is not available", change.device));
            }
            Answer::Gone => {
                self.lost_for_this_run(format!(
                    "The audio stopped while changing to {}. Save your work and restart \
                     {PRODUCT_NAME}.",
                    change.to
                ));
            }
        }
    }

    /// The engine's callback is gone with a driver that kept it: audio
    /// stays off for this run, saying why. The song, editing and autosave
    /// carry on.
    fn lost_for_this_run(&mut self, why: String) {
        self.pending = None;
        self.previous = None;
        self.asked = None;
        self.info = None;
        self.status = Status::Off(why);
    }
}
