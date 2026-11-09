//! Listening to imported files for a loop's tempo (M9.5 S5a, W A: a file
//! follows the song when it "lasts a whole number of bars from 1 to 32" and
//! detection is confident of its tempo). A file that states no tempo plays at
//! its own speed the moment it is imported; the file is read on a worker, and
//! when it is a loop the clip follows the song as one more undo step, so
//! nothing waits and the import stays one step. A clip edited meanwhile is
//! left alone (`Document::follow_detected`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use lumen_media::detect::{Analysis, CONFIDENT};

use eframe::egui;

use crate::App;
use crate::app::clip_file::ClipFile;
use crate::audio_editor::detect::listen;
use crate::document::Awaiting;

type Answer = Result<Option<Analysis>, String>;

/// A file being listened to.
struct Job {
    wait: Awaiting,
    done: Receiver<Answer>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The files being listened to, and the clips that wait for a file that is
/// not ready yet.
#[derive(Default)]
pub(crate) struct Jobs {
    waiting: Vec<Awaiting>,
    running: Vec<Job>,
}

impl Jobs {
    /// How many files are waiting or being listened to.
    pub(crate) fn busy(&self) -> usize {
        self.waiting.len() + self.running.len()
    }
}

/// The tempo `found` is sure of for a loop, when it is one.
pub(crate) fn loop_tempo(found: &Analysis) -> Option<f64> {
    let tempo = found.tempo?;
    (tempo.confidence >= CONFIDENT && tempo.is_loop()).then_some(tempo.bpm)
}

impl App {
    /// Starts listening to the files imported since the last frame, takes the
    /// answers, and makes the clips of the loops follow the song. Once a
    /// frame; cheap when nothing is being listened to.
    pub(crate) fn drive_follow_detection(&mut self, ctx: &egui::Context) {
        let found = self.document.take_awaiting_follow();
        self.follow_jobs.waiting.extend(found);
        if self.follow_jobs.waiting.is_empty() && self.follow_jobs.running.is_empty() {
            return;
        }
        let rate = self.lanes.rate();
        let mut still = Vec::new();
        for wait in std::mem::take(&mut self.follow_jobs.waiting) {
            match self.clip_file_state(wait.media) {
                // Under way: ask again next frame.
                ClipFile::Reading => {
                    still.push(wait);
                    continue;
                }
                ClipFile::Ready => {}
                // A file that cannot be used plays as it was imported.
                ClipFile::Failed(_) | ClipFile::Missing => continue,
            }
            let Some(item) = self.lanes.library.get(wait.media.0) else {
                continue;
            };
            let (path, cache) = (item.path.clone(), self.cache_dir());
            let (tx, done) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            let (stop, ctx) = (Arc::clone(&cancel), ctx.clone());
            let spawned = std::thread::Builder::new()
                .name("lumen-app::follow-detect".into())
                .spawn(move || {
                    let _ = tx.send(listen(&path, rate, cache, &stop));
                    ctx.request_repaint();
                });
            if spawned.is_ok() {
                self.follow_jobs.running.push(Job { wait, done, cancel });
            }
        }
        self.follow_jobs.waiting = still;
        if !self.follow_jobs.waiting.is_empty() {
            ctx.request_repaint_after(crate::motion::RECHECK);
        }

        let mut answered = Vec::new();
        self.follow_jobs
            .running
            .retain(|job| match job.done.try_recv() {
                Ok(answer) => {
                    answered.push((job.wait.clone(), answer));
                    false
                }
                Err(TryRecvError::Empty) => true,
                Err(TryRecvError::Disconnected) => false,
            });
        for (wait, answer) in answered {
            // A file that cannot be read plays as it was imported; the
            // editor says why when its chip is opened.
            let Ok(Some(found)) = answer else {
                continue;
            };
            let Some(bpm) = loop_tempo(&found) else {
                continue;
            };
            if self.document.follow_detected(&wait, bpm) {
                self.say(format!(
                    "Warped to follow the song's tempo: it sounds like a loop at {bpm:.1} BPM. Undo to play it at its own speed"
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use lumen_media::detect::Tempo;

    use super::*;

    fn found(tempo: Option<Tempo>) -> Analysis {
        Analysis {
            seconds: 9.6,
            transients: Vec::new(),
            tempo,
        }
    }

    #[test]
    fn only_a_confident_tempo_of_whole_bars_is_a_loops() {
        let tempo = |confidence, beats| {
            Some(Tempo {
                bpm: 100.0,
                confidence,
                beats,
            })
        };
        assert_eq!(loop_tempo(&found(tempo(0.9, Some(16)))), Some(100.0));
        assert_eq!(
            loop_tempo(&found(tempo(0.9, Some(14)))),
            None,
            "not whole bars"
        );
        assert_eq!(loop_tempo(&found(tempo(0.9, None))), None, "no beat count");
        assert_eq!(loop_tempo(&found(tempo(0.3, Some(16)))), None, "not sure");
        assert_eq!(loop_tempo(&found(None)), None);
    }
}
