//! The browser's file preview, UI side (M9 R7c, the owner's P A and O A;
//! ADR-0116). A decode thread of its own reads the file in small blocks
//! (`lumen_media::read_blocks_from`), converts it to the device rate and
//! fills the engine's preview ring (`lumen_engine::preview`), tagging every
//! block with its generation; the [`Preview`] here starts, seeks and stops,
//! and follows what is heard.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use lumen_engine::preview::{BLOCK_FRAMES, PreviewFeed, PreviewHandle};
use lumen_media::MediaError;
use lumen_media::stream::convert::Converter;

/// Frames read from a file at a time: small, so a preview starts soon.
const READ_FRAMES: usize = 4_096;
/// How long the feed waits for a free block before looking again: a
/// worker's poll, not a design timing (no `motion` token applies).
const WAIT: Duration = Duration::from_micros(4_000);

/// One file to play, from `from` seconds, for `generation`.
struct Play {
    generation: u32,
    path: PathBuf,
    from: f64,
    /// Set when the preview is replaced, stopped or over: the decoder
    /// checks it at every packet, even while it decodes its way to a far
    /// position in a compressed file.
    cancel: Arc<AtomicBool>,
}

impl Play {
    fn abandoned(&self, feed: &PreviewFeed) -> bool {
        feed.asked() != self.generation || self.cancel.load(Ordering::Relaxed)
    }
}

/// The decode thread's end: plays what it is asked, abandoning a file as
/// soon as a newer generation is asked for.
fn feed(mut feed: PreviewFeed, asks: Receiver<Play>, errors: Sender<(u32, String)>) {
    while let Ok(mut play) = asks.recv() {
        // Only the newest ask matters.
        while let Ok(newer) = asks.try_recv() {
            play = newer;
        }
        if play.abandoned(&feed) {
            continue;
        }
        if let Err(error) = stream(&mut feed, &play)
            && !matches!(error, MediaError::Cancelled)
        {
            let _ = errors.send((play.generation, error.to_string()));
        }
    }
}

/// Feeds `play` until the file ends or another generation is asked.
fn stream(feed: &mut PreviewFeed, play: &Play) -> Result<(), MediaError> {
    let rate = feed.rate();
    if rate == 0 {
        return Ok(());
    }
    let generation = play.generation;
    let header = lumen_media::stream::layout(&play.path)
        .ok()
        .flatten()
        .map(|l| (l.sample_rate, l.played()));
    let (file_rate, channels) = match header {
        Some(found) => found,
        None => {
            let probed = lumen_media::probe(&play.path)?;
            (
                probed.sample_rate.unwrap_or(rate),
                probed.channels.unwrap_or(2).clamp(1, 2),
            )
        }
    };
    let start = (play.from.max(0.0) * f64::from(file_rate)) as u64;
    let mut converter: Option<Converter> = None;
    let mut pending: [Vec<f32>; 2] = [Vec::new(), Vec::new()];
    let mut stereo = channels > 1;
    let send = |feed: &mut PreviewFeed,
                pending: &mut [Vec<f32>; 2],
                stereo: bool,
                last: bool|
     -> Result<(), MediaError> {
        while pending[0].len() >= BLOCK_FRAMES || (last && !pending[0].is_empty()) {
            let mut block = loop {
                if play.abandoned(feed) {
                    return Err(MediaError::Cancelled);
                }
                if let Some(block) = feed.take() {
                    break block;
                }
                std::thread::sleep(WAIT);
            };
            let n = pending[0].len().min(BLOCK_FRAMES);
            let end = last && n == pending[0].len();
            let right = stereo.then(|| &pending[1][..n]);
            block.fill(generation, &pending[0][..n], right, end);
            feed.send(block);
            for p in pending.iter_mut() {
                p.drain(..n.min(p.len()));
            }
        }
        Ok(())
    };
    lumen_media::read_blocks_from(
        &play.path,
        start,
        READ_FRAMES,
        &play.cancel,
        &mut |from, planes| {
            if play.abandoned(feed) {
                return Err(MediaError::Cancelled);
            }
            stereo = planes.len() > 1;
            let converter = match &mut converter {
                Some(c) => c,
                None => converter.insert(Converter::new(from, rate, planes.len())?),
            };
            converter.push(planes, &mut |out| {
                for (p, part) in pending.iter_mut().zip(out) {
                    p.extend_from_slice(part);
                }
                Ok(())
            })?;
            send(feed, &mut pending, stereo, false)
        },
    )?;
    if let Some(converter) = converter {
        converter.finish(&mut |out| {
            for (p, part) in pending.iter_mut().zip(out) {
                p.extend_from_slice(part);
            }
            Ok(())
        })?;
    }
    if pending[0].is_empty() {
        // Ended on a whole block: mark the end with an empty one.
        pending[0].push(0.0);
        pending[1].push(0.0);
    }
    send(feed, &mut pending, stereo, true)
}

/// What is playing, as the browser knows it.
#[derive(Debug, Clone)]
pub(crate) struct Playing {
    pub path: PathBuf,
    pub generation: u32,
    /// Where it started, in seconds into the file.
    pub from: f64,
    pub asked_at: Instant,
}

/// The UI's preview.
pub(crate) struct Preview {
    handle: PreviewHandle,
    asks: Option<Sender<Play>>,
    errors: Option<Receiver<(u32, String)>>,
    pub playing: Option<Playing>,
    /// The playing one's cancel flag, for the decode thread.
    cancel: Arc<AtomicBool>,
    /// Why the last preview did not play, for the strip.
    pub failed: Option<(PathBuf, String)>,
    /// From the click to the first frame heard, the last time (measured,
    /// for the slice's report).
    pub started_in: Option<Duration>,
    /// Where the preview is, in seconds into the file (the last poll).
    pub position: Option<f64>,
}

impl Preview {
    /// Starts the decode thread on `feed` (`None`: there is no device end,
    /// and previews are silent).
    pub(crate) fn new(handle: PreviewHandle, feed: Option<PreviewFeed>) -> Self {
        let (asks, errors) = match feed {
            Some(feed) => {
                let (ask_tx, ask_rx) = channel();
                let (error_tx, error_rx) = channel();
                let spawned = std::thread::Builder::new()
                    .name("lumen-preview".into())
                    .spawn(move || self::feed(feed, ask_rx, error_tx));
                if spawned.is_ok() {
                    (Some(ask_tx), Some(error_rx))
                } else {
                    (None, None)
                }
            }
            None => (None, None),
        };
        Self {
            handle,
            asks,
            errors,
            playing: None,
            cancel: Arc::default(),
            failed: None,
            started_in: None,
            position: None,
        }
    }

    /// Plays `path` from `from` seconds, ending whatever played.
    pub(crate) fn play(&mut self, path: &Path, from: f64) {
        let generation = self.handle.start();
        self.let_go();
        self.cancel = Arc::default();
        self.failed = None;
        self.started_in = None;
        if let Some(asks) = &self.asks {
            let _ = asks.send(Play {
                generation,
                path: path.to_owned(),
                from,
                cancel: Arc::clone(&self.cancel),
            });
        }
        self.playing = Some(Playing {
            path: path.to_owned(),
            generation,
            from,
            asked_at: Instant::now(),
        });
    }

    /// Fades it out.
    pub(crate) fn stop(&mut self) {
        if self.playing.take().is_some() {
            self.handle.stop();
            self.let_go();
        }
    }

    /// Tells the decode thread the preview it works on is over.
    fn let_go(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// The level, in dB, and the pair it is heard on.
    pub(crate) fn set_output(&self, db: f32, first_channel: usize) {
        self.handle.set_level(10.0_f32.powf(db / 20.0));
        self.handle.set_first_channel(first_channel);
    }

    /// Follows what is heard: ends the preview when its file ended, and
    /// notes the first frame and failures. Returns where it is, in seconds
    /// into the file, while one plays.
    pub(crate) fn poll(&mut self) -> Option<f64> {
        self.position = self.follow();
        self.position
    }

    fn follow(&mut self) -> Option<f64> {
        while let Some((generation, error)) = self.errors.as_ref().and_then(|e| e.try_recv().ok()) {
            if let Some(playing) = &self.playing
                && playing.generation == generation
            {
                // What was decoded before the failure may still be queued:
                // it fades out with the rest.
                self.failed = Some((playing.path.clone(), error));
                self.playing = None;
                self.handle.stop();
                self.let_go();
            }
        }
        let playing = self.playing.as_ref()?;
        let heard = self.handle.heard();
        if heard.generation != playing.generation {
            return Some(playing.from);
        }
        if heard.frames > 0 && self.started_in.is_none() {
            self.started_in = Some(playing.asked_at.elapsed());
        }
        if heard.ended {
            // Played to its end, or a change of the device rate ended it.
            self.playing = None;
            self.let_go();
            return None;
        }
        let rate = self.handle.rate().max(1);
        Some(playing.from + heard.frames as f64 / f64::from(rate))
    }

    /// Whether `path` is the one playing.
    pub(crate) fn is_playing(&self, path: &Path) -> bool {
        self.playing.as_ref().is_some_and(|p| p.path == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_media::export::{Format, Spec, encode};
    use lumen_media::wav::{Sample, Spec as WavSpec, Writer};

    /// A FLAC file of `frames` frames, in a folder of its own.
    fn flac(label: &str, frames: usize) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lumen-preview-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("source.wav");
        let tone: Vec<f32> = (0..frames).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        let spec = WavSpec {
            sample_rate: 48_000,
            channels: 2,
            sample: Sample::F32,
        };
        let mut writer = Writer::create(&wav, spec).unwrap();
        writer.write(&[&tone, &tone]).unwrap();
        writer.finish().unwrap();
        let out = dir.join("tone.flac");
        let spec = Spec {
            sample_rate: 48_000,
            format: Format::FlacI16,
            normalize_peak: false,
            tpdf_dither: false,
        };
        encode(&wav, &out, spec, &AtomicBool::new(false), &mut |_| {}).unwrap();
        out
    }

    /// A feed whose player has run at 48 kHz, so it knows the rate.
    fn feed_at_48k() -> (lumen_engine::preview::PreviewHandle, PreviewFeed) {
        let (handle, feed, mut player) = lumen_engine::preview::preview();
        let mut out = vec![0.0; 2 * 64];
        player.process(48_000, &mut out, 2, 64);
        (handle, feed)
    }

    #[test]
    fn an_abandoned_preview_stops_while_it_decodes_its_way_to_a_far_position() {
        let path = flac("far", 96_000);
        let (handle, mut feed) = feed_at_48k();
        let generation = handle.start();
        // Replaced while the decoder skips towards its position: the
        // generation is still the one asked, and a position past the end
        // never reaches the sink, so only the decoder's own check of the
        // flag can stop it (without it, the whole file is decoded and an
        // end is sent).
        let play = Play {
            generation,
            path: path.clone(),
            from: 5.0,
            cancel: Arc::new(AtomicBool::new(true)),
        };
        assert_eq!(stream(&mut feed, &play), Err(MediaError::Cancelled));
        let mut free = 0;
        while feed.take().is_some() {
            free += 1;
        }
        assert_eq!(free, lumen_engine::preview::RING_BLOCKS, "nothing was sent");
        // The same seek, not abandoned, plays the end of the file.
        let (handle, mut feed) = feed_at_48k();
        let play = Play {
            generation: handle.start(),
            path,
            from: 1.9,
            cancel: Arc::default(),
        };
        assert_eq!(stream(&mut feed, &play), Ok(()));
        assert!(feed.take().is_some());
    }

    #[test]
    fn a_preview_that_fails_after_it_sent_audio_stops_what_it_sent() {
        let (handle, _feed) = feed_at_48k();
        let mut preview = Preview::new(handle.clone(), None);
        let (errors, failures) = channel();
        preview.errors = Some(failures);
        preview.play(Path::new("broken.flac"), 0.0);
        let generation = preview.playing.as_ref().expect("playing").generation;
        // Blocks of this generation may already be queued when the
        // decoder meets the error.
        errors.send((generation, "damaged".into())).unwrap();
        assert_eq!(preview.poll(), None);
        assert!(preview.playing.is_none());
        assert_eq!(
            preview.failed.as_ref().map(|(_, why)| why.as_str()),
            Some("damaged")
        );
        assert_ne!(handle.asked(), generation, "the engine was told to stop");
    }
}
