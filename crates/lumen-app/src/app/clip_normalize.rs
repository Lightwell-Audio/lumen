//! Normalize (M7b slice E1): sets the selected audio clips' gain so the
//! loudest sample of the stretch each plays reaches full scale. The scan
//! runs on a worker (`normalize::Scan`) with progress in the toast and Esc
//! to stop; the gains are applied as one undo step when it finishes.

use std::time::Instant;

use eframe::egui;
use lumen_project::edit::normalize::{gain_for_peak, window};
use lumen_project::edit::tempo_of;
use lumen_project::model::ClipContent;

use crate::App;
use crate::media::{State, reversed_key};
use crate::normalize::{Job, Poll, Scan};

/// A scan under way.
pub(crate) struct Running {
    scan: Scan,
    /// Selected clips left out because their audio was not ready.
    left_out: usize,
}

impl App {
    /// Starts the scan for the selected audio clips (Normalize).
    pub(crate) fn normalize_selected_clips(&mut self, ctx: &egui::Context) {
        if self.normalizing.is_some() {
            self.say("Already normalizing".to_owned());
            return;
        }
        let project = self.document.project();
        let tempo = tempo_of(project);
        let mut jobs = Vec::new();
        let mut left_out = 0;
        for clip in project.tracks.iter().flat_map(|t| &t.clips) {
            let ClipContent::Audio { media, .. } = &clip.content else {
                continue;
            };
            if !self.arrangement.clips.contains(&clip.id) || clip.locked {
                continue;
            }
            // A reversed clip plays its file's reversed twin.
            let key = if clip.reversed {
                reversed_key(media.0)
            } else {
                media.0
            };
            let audio = self
                .lanes
                .library
                .get(key)
                .and_then(|item| match &item.state {
                    State::Ready(audio) => Some(std::sync::Arc::clone(audio)),
                    _ => None,
                });
            let rate = project
                .media
                .iter()
                .find(|m| m.id == *media)
                .map(|m| f64::from(m.sample_rate.max(1)));
            let (Some(audio), Some(rate)) = (audio, rate) else {
                left_out += 1;
                continue;
            };
            let Some(window) = window(clip, rate, &tempo) else {
                continue;
            };
            jobs.push(Job {
                clip: clip.id,
                audio,
                window,
            });
        }
        if jobs.is_empty() {
            self.say(if left_out > 0 {
                "Wait for the audio to finish loading, then normalize".to_owned()
            } else {
                "Select an audio clip first".to_owned()
            });
            return;
        }
        self.normalizing = Some(Running {
            scan: Scan::start(jobs, ctx),
            left_out,
        });
    }

    /// Follows the scan: shows its progress, stops it on Esc and applies the
    /// gains when it is done. Call every frame.
    pub(crate) fn poll_normalize(&mut self, ctx: &egui::Context) {
        let Some(running) = &self.normalizing else {
            return;
        };
        if !self.typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            running.scan.stop();
            self.normalizing = None;
            self.say("Normalize stopped".to_owned());
            return;
        }
        match running.scan.poll() {
            Poll::Running(done) => {
                let shown = (done * 100.0).floor() as u32;
                // Keeps the note up while the scan goes.
                self.toast = Some((
                    format!("Normalizing {shown}% \u{b7} Esc to stop"),
                    Instant::now(),
                ));
            }
            Poll::Stopped => {
                self.normalizing = None;
                self.say("Normalize stopped".to_owned());
            }
            Poll::Done(found) => {
                let left_out = running.left_out;
                self.normalizing = None;
                let mut gains = Vec::new();
                let mut silent = 0;
                for found in &found {
                    match gain_for_peak(found.peak) {
                        Some(db) => gains.push((found.clip, db)),
                        None => silent += 1,
                    }
                }
                let limited = self.document.normalize_clips(&gains);
                self.say(normalized_words(gains.len(), silent, limited, left_out));
            }
        }
    }
}

/// What to tell the user after a scan.
fn normalized_words(done: usize, silent: usize, limited: usize, left_out: usize) -> String {
    if done == 0 {
        return "Nothing to normalize: the clips are silent".to_owned();
    }
    let mut words = if done == 1 {
        "Normalized".to_owned()
    } else {
        format!("Normalized {done} clips")
    };
    if limited > 0 {
        words.push_str(" \u{b7} the largest clip gain is not enough for full scale");
    }
    if silent > 0 {
        words.push_str(" \u{b7} silent clips left as they were");
    }
    if left_out > 0 {
        words.push_str(" \u{b7} clips still loading left out");
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_names_what_was_left_as_it_was() {
        assert_eq!(normalized_words(1, 0, 0, 0), "Normalized");
        assert_eq!(normalized_words(3, 0, 0, 0), "Normalized 3 clips");
        assert!(normalized_words(2, 1, 0, 0).contains("silent"));
        assert!(normalized_words(2, 0, 1, 0).contains("largest clip gain"));
        assert!(normalized_words(2, 0, 0, 1).contains("still loading"));
        assert!(normalized_words(0, 2, 0, 0).contains("silent"));
    }
}
