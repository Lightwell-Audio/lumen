//! Normalize (M7b slice E1): sets the selected audio clips' gain so the
//! loudest sample of the stretch each plays reaches full scale. The scan
//! runs on a worker (`normalize::Scan`) with progress in the toast and Esc
//! to stop; the gains are applied as one undo step when it finishes.

use std::time::Instant;

use eframe::egui;
use lumen_project::edit::normalize::{gain_for_peak, is_current, source_of};
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
            let (Some(audio), Some(source)) = (audio, source_of(project, clip)) else {
                left_out += 1;
                continue;
            };
            jobs.push(Job {
                clip: clip.id,
                audio,
                source,
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
                let mut changed = 0;
                for found in &found {
                    // An edit while the scan ran (a trim, a slip, a reverse,
                    // a tempo change) means the peak is of other audio.
                    if !is_current(self.document.project(), found.clip, &found.source) {
                        changed += 1;
                        continue;
                    }
                    match gain_for_peak(found.peak) {
                        Some(db) => gains.push((found.clip, db)),
                        None => silent += 1,
                    }
                }
                let limited = self.document.normalize_clips(&gains);
                self.say(normalized_words(Words {
                    done: gains.len(),
                    silent,
                    limited,
                    left_out,
                    changed,
                }));
            }
        }
    }
}

/// What a scan came to, for the note after it.
struct Words {
    /// Clips given a gain.
    done: usize,
    /// Clips whose stretch is silent.
    silent: usize,
    /// Clips asked for more gain than a clip can have.
    limited: usize,
    /// Clips left out because their audio was still loading.
    left_out: usize,
    /// Clips edited while the scan ran, so their peak was not applied.
    changed: usize,
}

/// What to tell the user after a scan.
fn normalized_words(
    Words {
        done,
        silent,
        limited,
        left_out,
        changed,
    }: Words,
) -> String {
    if done == 0 {
        return if changed > 0 {
            "Normalize not applied: the clips were edited while it ran. Run it again".to_owned()
        } else {
            "Nothing to normalize: the clips are silent".to_owned()
        };
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
    if changed > 0 {
        words.push_str(" \u{b7} clips edited while it ran left as they were");
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(
        done: usize,
        silent: usize,
        limited: usize,
        left_out: usize,
        changed: usize,
    ) -> String {
        normalized_words(Words {
            done,
            silent,
            limited,
            left_out,
            changed,
        })
    }

    #[test]
    fn the_note_names_what_was_left_as_it_was() {
        assert_eq!(words(1, 0, 0, 0, 0), "Normalized");
        assert_eq!(words(3, 0, 0, 0, 0), "Normalized 3 clips");
        assert!(words(2, 1, 0, 0, 0).contains("silent"));
        assert!(words(2, 0, 1, 0, 0).contains("largest clip gain"));
        assert!(words(2, 0, 0, 1, 0).contains("still loading"));
        assert!(words(0, 2, 0, 0, 0).contains("silent"));
        assert!(words(2, 0, 0, 0, 1).contains("edited while it ran"));
        let none = words(0, 0, 0, 0, 2);
        assert!(none.contains("not applied") && none.contains("Run it again"));
    }
}
