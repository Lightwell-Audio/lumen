use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use lumen_audio::{AudioSystem, SwitchError};

use lumen_core::product::PRODUCT_NAME;

use super::{Audio, NOT_ANSWERING, Previous, Resolved, Status, driver_name, find_saved};
use crate::settings::AudioSettings;

pub(super) enum Target {
    Settings(AudioSettings),
    Found(Resolved),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Looking,
    HandingOver,
    Closing,
}

pub(super) enum Answer {
    Step(Step),
    Missing(Option<AudioSystem>),
    Found(Resolved),
    Switched(Resolved, AudioSystem),
    Failed(SwitchError),
    Gone,
}

pub(super) struct Change {
    answers: Receiver<Answer>,
    unread: VecDeque<Answer>,
    pub step: Step,
    pub since: Instant,
    pub settings: Option<AudioSettings>,
    pub previous: Option<Previous>,
    pub device: String,
    pub from: String,
    pub to: String,
}

pub(super) struct Names {
    pub device: String,
    pub from: String,
    pub to: String,
}

impl Change {
    pub fn start(
        target: Target,
        system: Option<AudioSystem>,
        settings: Option<AudioSettings>,
        previous: Option<Previous>,
        names: Names,
    ) -> Self {
        let (tx, answers) = mpsc::channel();
        let spawned =
            crate::spawn::spawn_named("lumen-audio-change", move || run(target, system, &tx));
        if let Err(error) = spawned {
            crate::trace::log(|| {
                format!(
                    "audio: {}",
                    crate::spawn::start_failed("the audio change", &error)
                )
            });
        }
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

    pub fn switched_unread(&mut self) -> bool {
        while let Ok(answer) = self.answers.try_recv() {
            self.unread.push_back(answer);
        }
        self.unread
            .iter()
            .any(|answer| matches!(answer, Answer::Switched(..)))
    }
}

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
            drop(system);
        }
    }
}

impl Audio {
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

    pub fn can_change(&self) -> bool {
        self.change.is_none()
            && !self.awaiting()
            && (self.system.is_some() || self.parked.is_some())
    }

    pub fn waiting(&self) -> bool {
        self.change.is_some() || self.awaiting()
    }

    pub(super) fn awaiting(&self) -> bool {
        self.asked.is_some() && matches!(self.status, Status::Starting)
    }

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

    pub(crate) fn switched_unread(&mut self) -> bool {
        self.change.as_mut().is_some_and(Change::switched_unread)
    }

    pub(super) fn poll_change(&mut self) -> bool {
        let mut changed = false;
        while let Some(answer) = self.change.as_mut().and_then(Change::answer) {
            changed = true;
            if let Answer::Step(step) = answer {
                if let Some(change) = &mut self.change {
                    change.step = step;
                    change.since = Instant::now();
                }
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

    fn finish(&mut self, change: Change, answer: Answer) {
        let adopt = |audio: &mut Self| {
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
            Answer::Failed(error @ SwitchError::NotStarted) => {
                self.lost_for_this_run(format!(
                    "Could not switch the audio driver: {error}. Save your work and restart \
                     {PRODUCT_NAME}."
                ));
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

    fn lost_for_this_run(&mut self, why: String) {
        self.pending = None;
        self.previous = None;
        self.asked = None;
        self.info = None;
        self.status = Status::Off(why);
    }
}
