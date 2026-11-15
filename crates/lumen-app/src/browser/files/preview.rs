use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use lumen_engine::preview::{BLOCK_FRAMES, PreviewFeed, PreviewHandle};
use lumen_media::MediaError;
use lumen_media::stream::convert::Converter;

const READ_FRAMES: usize = 4_096;
const WAIT: Duration = Duration::from_micros(4_000);

struct Play {
    generation: u32,
    path: PathBuf,
    from: f64,
    cancel: Arc<AtomicBool>,
}

impl Play {
    fn abandoned(&self, feed: &PreviewFeed) -> bool {
        feed.asked() != self.generation || self.cancel.load(Ordering::Relaxed)
    }
}

fn feed(mut feed: PreviewFeed, asks: Receiver<Play>, errors: Sender<(u32, String)>) {
    while let Ok(mut play) = asks.recv() {
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
        pending[0].push(0.0);
        pending[1].push(0.0);
    }
    send(feed, &mut pending, stereo, true)
}

#[derive(Debug, Clone)]
pub(crate) struct Playing {
    pub path: PathBuf,
    pub generation: u32,
    pub from: f64,
    pub asked_at: Instant,
}

pub(crate) struct Preview {
    handle: PreviewHandle,
    asks: Option<Sender<Play>>,
    errors: Option<Receiver<(u32, String)>>,
    pub playing: Option<Playing>,
    cancel: Arc<AtomicBool>,
    pub failed: Option<(PathBuf, String)>,
    pub started_in: Option<Duration>,
    pub position: Option<f64>,
}

impl Preview {
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

    pub(crate) fn stop(&mut self) {
        if self.playing.take().is_some() {
            self.handle.stop();
            self.let_go();
        }
    }

    fn let_go(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub(crate) fn set_output(&self, db: f32, first_channel: usize) {
        self.handle.set_level(10.0_f32.powf(db / 20.0));
        self.handle.set_first_channel(first_channel);
    }

    pub(crate) fn poll(&mut self) -> Option<f64> {
        self.position = self.follow();
        self.position
    }

    fn follow(&mut self) -> Option<f64> {
        while let Some((generation, error)) = self.errors.as_ref().and_then(|e| e.try_recv().ok()) {
            if let Some(playing) = &self.playing
                && playing.generation == generation
            {
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
            self.playing = None;
            self.let_go();
            return None;
        }
        let rate = self.handle.rate().max(1);
        Some(playing.from + heard.frames as f64 / f64::from(rate))
    }

    pub(crate) fn is_playing(&self, path: &Path) -> bool {
        self.playing.as_ref().is_some_and(|p| p.path == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_media::export::{Format, Spec, encode};
    use lumen_media::wav::{Sample, Spec as WavSpec, Writer};

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
