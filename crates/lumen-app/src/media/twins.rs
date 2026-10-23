//! Reversed twins of loaded files (M7b slice E1). A reversed clip's place in
//! its file is measured in the reversed file (`Clip.reversed`), so the clip
//! player, the waveform and the sample view all read a copy of the audio with
//! its frames in the other order. A twin is made on a worker thread when the
//! project first has a reversed clip of a file, is named by
//! [`reversed_key`] wherever a media id is wanted, and goes when no clip
//! needs it. Until it is ready the reversed clip is silent and has no
//! waveform, as a clip is while its file loads.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use lumen_engine::ClipAudio;
use lumen_media::Peaks;

use super::{Item, Library, State};

/// Set in a media id to name the reversed twin of that file.
const REVERSED: u64 = 1 << 62;

/// The key of the reversed twin of media `id`, for [`Library::get`] and the
/// engine's and the waveform's lookups.
pub fn reversed_key(id: u64) -> u64 {
    id | REVERSED
}

/// The media id a key stands for, and whether it names a reversed twin.
pub fn media_of(key: u64) -> (u64, bool) {
    (key & !REVERSED, key & REVERSED != 0)
}

/// A reversed copy of one file's audio, made or being made.
pub(super) struct Twin {
    pub item: Item,
    /// The audio it was made from: a file converted again (another device
    /// rate) is a new `Arc`, and the twin is made again.
    from: Arc<ClipAudio>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Twin {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// A finished twin on its way back to the library.
pub(super) struct Mirrored {
    key: u64,
    from: Arc<ClipAudio>,
    audio: ClipAudio,
    peaks: Option<Peaks>,
}

/// `audio` with every channel's frames in the other order.
pub fn mirror(audio: &ClipAudio) -> ClipAudio {
    ClipAudio {
        sample_rate: audio.sample_rate,
        channels: audio
            .channels
            .iter()
            .map(|c| c.iter().rev().copied().collect())
            .collect(),
    }
}

impl Library {
    /// Makes the reversed twins of the files in `wanted` (media ids) that
    /// are ready and have none, and drops twins nobody wants or whose file
    /// was converted again. Returns whether a twin was dropped, so the
    /// engine's clips need building again.
    pub fn keep_reversed(&mut self, wanted: &[u64], ctx: &egui::Context) -> bool {
        let before = self.twins.len();
        let items = &self.items;
        self.twins.retain(|twin| {
            let (id, _) = media_of(twin.item.id);
            wanted.contains(&id)
                && items.iter().any(|i| {
                    i.id == id && matches!(&i.state, State::Ready(a) if Arc::ptr_eq(a, &twin.from))
                })
        });
        let dropped = self.twins.len() != before;
        for &id in wanted {
            let key = reversed_key(id);
            if self.twins.iter().any(|t| t.item.id == key) {
                continue;
            }
            let Some(item) = self.items.iter().find(|i| i.id == id) else {
                continue;
            };
            let State::Ready(from) = &item.state else {
                continue;
            };
            let (from, cancel) = (Arc::clone(from), Arc::new(AtomicBool::new(false)));
            self.twins.push(Twin {
                item: Item {
                    id: key,
                    path: PathBuf::from(&item.name),
                    name: item.name.clone(),
                    state: State::Loading,
                    info: item.info.clone(),
                    peaks: None,
                    job: None,
                },
                from: Arc::clone(&from),
                cancel: Arc::clone(&cancel),
            });
            let (done, ctx) = (self.twin_tx.clone(), ctx.clone());
            std::thread::Builder::new()
                .name("lumen-media-reverse".into())
                .spawn(move || {
                    let audio = mirror(&from);
                    let peaks = Peaks::build(&audio.channels, &cancel);
                    let _ = done.send(Mirrored {
                        key,
                        from,
                        audio,
                        peaks,
                    });
                    ctx.request_repaint();
                })
                .expect("spawn media reverser");
        }
        dropped
    }

    /// Takes the twins that finished. Returns whether any did.
    pub(super) fn take_twins(&mut self) -> bool {
        let mut changed = false;
        while let Ok(done) = self.twin_rx.try_recv() {
            let Some(twin) = self
                .twins
                .iter_mut()
                .find(|t| t.item.id == done.key && Arc::ptr_eq(&t.from, &done.from))
            else {
                continue;
            };
            twin.item.peaks = done.peaks.map(Arc::new);
            twin.item.state = State::Ready(Arc::new(done.audio));
            changed = true;
        }
        changed
    }

    /// Whether a twin is still being made.
    #[cfg(test)]
    pub fn is_reversing(&self) -> bool {
        self.twins
            .iter()
            .any(|t| matches!(t.item.state, State::Loading))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(frames: usize) -> Arc<ClipAudio> {
        let left: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32).collect();
        let right: Vec<f32> = left.iter().map(|v| -v).collect();
        Arc::new(ClipAudio {
            sample_rate: 48_000,
            channels: vec![left, right],
        })
    }

    fn peaks_of(audio: &ClipAudio) -> Arc<Peaks> {
        Arc::new(Peaks::build(&audio.channels, &AtomicBool::new(false)).unwrap())
    }

    fn wait_for_twins(library: &mut Library) {
        let started = std::time::Instant::now();
        while library.is_reversing() {
            library.take_twins();
            assert!(started.elapsed().as_secs() < 20, "twin did not finish");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn a_key_names_the_twin_and_the_file_it_came_from() {
        assert_eq!(media_of(reversed_key(7)), (7, true));
        assert_eq!(media_of(7), (7, false));
        assert_ne!(reversed_key(7), 7);
    }

    #[test]
    fn mirroring_puts_every_channels_frames_in_the_other_order() {
        let audio = audio(1_000);
        let twin = mirror(&audio);
        assert_eq!(twin.frames(), 1_000);
        for channel in 0..2 {
            for j in 0..1_000 {
                assert_eq!(twin.channels[channel][j], audio.channels[channel][999 - j]);
            }
        }
    }

    #[test]
    fn a_twin_is_made_once_kept_while_wanted_and_dropped_after() {
        let ctx = egui::Context::default();
        let mut library = Library::default();
        let source = audio(40_000);
        library.insert_ready(5, "a", Arc::clone(&source), peaks_of(&source));
        assert!(library.get(reversed_key(5)).is_none());

        assert!(!library.keep_reversed(&[5], &ctx));
        assert!(library.is_reversing());
        wait_for_twins(&mut library);
        let twin = library.get(reversed_key(5)).expect("the twin is held");
        let State::Ready(made) = &twin.state else {
            panic!("not ready")
        };
        assert_eq!(made.channels[0][0], source.channels[0][39_999]);
        assert!(twin.peaks.is_some());

        // Asking again makes no second twin.
        assert!(!library.keep_reversed(&[5], &ctx));
        assert!(!library.is_reversing());
        // Nobody wants it any more.
        assert!(library.keep_reversed(&[], &ctx));
        assert!(library.get(reversed_key(5)).is_none());
    }

    #[test]
    fn a_file_converted_again_gets_a_new_twin() {
        let ctx = egui::Context::default();
        let mut library = Library::default();
        let first = audio(10_000);
        library.insert_ready(5, "a", Arc::clone(&first), peaks_of(&first));
        library.keep_reversed(&[5], &ctx);
        wait_for_twins(&mut library);
        let second = audio(12_000);
        library.insert_ready(5, "a", Arc::clone(&second), peaks_of(&second));
        // The old twin goes; the new one starts.
        assert!(library.keep_reversed(&[5], &ctx));
        assert!(library.is_reversing());
        wait_for_twins(&mut library);
        let State::Ready(made) = &library.get(reversed_key(5)).unwrap().state else {
            panic!("not ready")
        };
        assert_eq!(made.frames(), 12_000);
    }
}
