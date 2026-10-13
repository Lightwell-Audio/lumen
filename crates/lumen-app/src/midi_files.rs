//! Reading and writing MIDI files off the UI thread (M5 slice 7, ADR-0062).
//! The bytes are turned into notes by `lumen_project::smf`; this module
//! only moves them to and from the disk and back to the frame loop, which
//! polls each frame.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::smf::{self, MidiFile};

use crate::document::DropInto;

/// The extensions read as Standard MIDI Files.
pub const EXTENSIONS: &[&str] = &["mid", "midi", "smf"];

/// Whether `path` names a MIDI file, by its extension.
pub fn is_midi_file(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| EXTENSIONS.iter().any(|e| x.eq_ignore_ascii_case(e)))
}

/// A file read in the background, and where it was dropped.
pub struct Read {
    pub path: PathBuf,
    /// The file's name without its extension, for clips and tracks.
    pub name: String,
    pub at: Ticks,
    pub into: DropInto,
    pub file: Result<MidiFile, String>,
}

/// Files being read, in the order they were dropped.
#[derive(Default)]
pub struct Reader {
    batches: Vec<Receiver<Read>>,
}

impl Reader {
    /// Starts reading `paths` on a thread of their own.
    pub fn read(&mut self, paths: Vec<PathBuf>, at: Ticks, into: DropInto, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("lumen-midi-read".into())
            .spawn(move || {
                for path in paths {
                    let file = read_file(&path);
                    let name = path
                        .file_stem()
                        .map_or_else(|| "MIDI".into(), |s| s.to_string_lossy().into_owned());
                    let read = Read {
                        path,
                        name,
                        at,
                        into,
                        file,
                    };
                    if tx.send(read).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("spawn MIDI file reader");
        self.batches.push(rx);
    }

    /// The files read since the last call, in order.
    pub fn poll(&mut self) -> Vec<Read> {
        let mut done = Vec::new();
        self.batches.retain(|rx| {
            loop {
                match rx.try_recv() {
                    Ok(read) => done.push(read),
                    Err(mpsc::TryRecvError::Empty) => break true,
                    Err(mpsc::TryRecvError::Disconnected) => break false,
                }
            }
        });
        done
    }

    /// Whether files are still being read.
    pub fn busy(&self) -> bool {
        !self.batches.is_empty()
    }
}

/// A MIDI file's notes, or why they could not be read.
pub fn read_file(path: &Path) -> Result<MidiFile, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    smf::read(&bytes).map_err(|e| e.to_string())
}

/// Writes `bytes` to `path` in the background, atomically; the answer is
/// the path written, or why not.
pub fn write(
    path: PathBuf,
    bytes: Vec<u8>,
    ctx: &egui::Context,
) -> Receiver<Result<PathBuf, String>> {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name("lumen-midi-write".into())
        .spawn(move || {
            let result = lumen_project::write_atomic(&path, &bytes)
                .map(|()| path)
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        })
        .expect("spawn MIDI file writer");
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_files_are_recognised_by_extension() {
        assert!(is_midi_file(Path::new("groove.mid")));
        assert!(is_midi_file(Path::new("C:/Song.MIDI")));
        assert!(!is_midi_file(Path::new("kick.wav")));
        assert!(!is_midi_file(Path::new("mid")));
    }

    #[test]
    fn files_are_read_and_written_in_the_background() {
        let dir = std::env::temp_dir().join(format!("lumen-midi-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = egui::Context::default();
        let path = dir.join("one.mid");
        let file = MidiFile {
            tracks: vec![smf::FileTrack {
                notes: vec![lumen_project::model::Note::new(
                    Ticks::ZERO,
                    Ticks::quarters(1),
                    60,
                    100,
                )],
                ..Default::default()
            }],
            ..MidiFile::default()
        };
        let written = write(path.clone(), smf::write(&file, smf::Layout::Single), &ctx);
        assert_eq!(written.recv().unwrap(), Ok(path.clone()));

        let mut reader = Reader::default();
        reader.read(
            vec![path, dir.join("gone.mid")],
            Ticks::ZERO,
            DropInto::END,
            &ctx,
        );
        let mut reads = Vec::new();
        while reader.busy() || reads.len() < 2 {
            reads.extend(reader.poll());
            std::thread::yield_now();
        }
        assert_eq!(reads[0].name, "one");
        assert_eq!(
            reads[0].file.as_ref().unwrap().tracks[0].notes,
            file.tracks[0].notes
        );
        assert!(reads[1].file.is_err(), "a missing file says why");
        let _ = std::fs::remove_dir_all(dir);
    }
}
