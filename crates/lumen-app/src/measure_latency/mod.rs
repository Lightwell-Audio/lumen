use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use lumen_audio::StreamInfo;
use lumen_engine::loopback::{self, Failure, Job, LoopbackHandle, Measured};

use crate::settings::MeasuredWith;

mod card;

pub(crate) use card::{Asked, Card, Shown};

const PATIENCE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cable {
    pub output: usize,
    pub pair: bool,
    pub input: usize,
    pub output_name: String,
    pub input_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Failed(Failure),
    Stopped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub cable: Cable,
    pub result: Result<Measured, Problem>,
    pub rate: u32,
    pub reported: u32,
    pub with: MeasuredWith,
}

impl Outcome {
    pub fn offset_ms(&self) -> Option<f32> {
        let measured = self.result.as_ref().ok()?;
        let ms = (f64::from(measured.frames) - f64::from(self.reported)) * 1000.0
            / f64::from(self.rate.max(1));
        Some(((ms * 10.0).round() / 10.0).clamp(-500.0, 500.0) as f32)
    }
}

enum State {
    Idle,
    Listening {
        since: Instant,
        start: Start,
    },
    Analysing {
        start: Start,
        found: Receiver<Result<Measured, Failure>>,
    },
    Done(Outcome),
}

struct Start {
    id: u64,
    cable: Cable,
    rate: u32,
    reported: u32,
    with: MeasuredWith,
}

pub struct Measurement {
    handle: LoopbackHandle,
    state: State,
    last: u64,
}

impl Measurement {
    pub fn new(handle: LoopbackHandle) -> Self {
        Self {
            handle,
            state: State::Idle,
            last: 0,
        }
    }

    pub fn start(&mut self, info: &StreamInfo, cable: Cable, with: MeasuredWith) {
        if self.busy() {
            return;
        }
        self.last += 1;
        let start = Start {
            id: self.last,
            reported: info.input_latency + info.output_latency,
            rate: info.sample_rate,
            with,
            cable,
        };
        let job = Job::new(
            start.id,
            start.rate,
            start.cable.output,
            start.cable.pair,
            start.cable.input,
        );
        self.state = match self.handle.start(job) {
            Ok(()) => State::Listening {
                since: Instant::now(),
                start,
            },
            Err(_) => State::Done(start.finish(Err(Problem::Stopped))),
        };
    }

    pub fn poll(&mut self, running: bool) -> bool {
        let ended = std::iter::from_fn(|| self.handle.ended())
            .filter(|job| job.id() == self.last)
            .last();
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Listening { since, start } => {
                if let Some(job) = ended {
                    let (tx, found) = mpsc::channel();
                    let spawned = std::thread::Builder::new()
                        .name("measure latency".into())
                        .spawn(move || {
                            let _ = tx.send(loopback::analyse(&job));
                        });
                    self.state = match spawned {
                        Ok(_) => State::Analysing { start, found },
                        Err(_) => State::Done(start.finish(Err(Problem::Stopped))),
                    };
                    true
                } else if !running || since.elapsed() > PATIENCE {
                    self.state = State::Done(start.finish(Err(Problem::Stopped)));
                    true
                } else {
                    self.state = State::Listening { since, start };
                    false
                }
            }
            State::Analysing { start, found } => match found.try_recv() {
                Ok(result) => {
                    self.state = State::Done(start.finish(result.map_err(Problem::Failed)));
                    true
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.state = State::Analysing { start, found };
                    false
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.state = State::Done(start.finish(Err(Problem::Stopped)));
                    true
                }
            },
            other => {
                self.state = other;
                false
            }
        }
    }

    pub fn busy(&self) -> bool {
        matches!(
            self.state,
            State::Listening { .. } | State::Analysing { .. }
        )
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        match &self.state {
            State::Done(outcome) => Some(outcome),
            _ => None,
        }
    }
}

impl Start {
    fn finish(self, result: Result<Measured, Problem>) -> Outcome {
        Outcome {
            cable: self.cable,
            result,
            rate: self.rate,
            reported: self.reported,
            with: self.with,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(frames: u32, reported: u32) -> Outcome {
        Outcome {
            cable: Cable {
                output: 0,
                pair: true,
                input: 0,
                output_name: "Out 1/2".into(),
                input_name: "In 1".into(),
            },
            result: Ok(Measured {
                frames,
                clipped: false,
                inverted: false,
                returns: Vec::new(),
            }),
            rate: 48_000,
            reported,
            with: MeasuredWith {
                driver: "asio".into(),
                device_id: "x".into(),
                device_name: "X".into(),
                sample_rate: 48_000,
                buffer_frames: 128,
            },
        }
    }

    #[test]
    fn the_offset_is_what_the_driver_left_out_to_a_tenth() {
        assert_eq!(outcome(427, 352).offset_ms(), Some(1.6));
        assert_eq!(outcome(352, 352).offset_ms(), Some(0.0));
        assert_eq!(outcome(300, 352).offset_ms(), Some(-1.1));
        assert_eq!(outcome(100_000, 0).offset_ms(), Some(500.0));
    }

    #[test]
    fn a_failure_offers_no_offset() {
        let mut failed = outcome(427, 352);
        failed.result = Err(Problem::Failed(Failure::Silent));
        assert_eq!(failed.offset_ms(), None);
    }
}
