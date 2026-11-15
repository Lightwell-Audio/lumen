use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::{Id, Project};
use lumen_project::smf::{self, Layout, MidiFile};

use crate::document::DropInto;
use crate::midi::export as midi_export;

pub const EXTENSIONS: &[&str] = &["mid", "midi", "smf"];

pub fn is_midi_file(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| EXTENSIONS.iter().any(|e| x.eq_ignore_ascii_case(e)))
}

pub struct Read {
    pub path: PathBuf,
    pub name: String,
    pub at: Ticks,
    pub into: DropInto,
    pub file: Result<MidiFile, String>,
}

#[derive(Default)]
pub struct Reader {
    batches: Vec<Receiver<Read>>,
    failure: Option<String>,
}

impl Reader {
    pub fn read(&mut self, paths: Vec<PathBuf>, at: Ticks, into: DropInto, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = crate::spawn::spawn_named("lumen-midi-read", move || {
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
        });
        match spawned {
            Ok(_) => self.batches.push(rx),
            Err(error) => {
                let why = crate::spawn::start_failed("reading the MIDI file", &error);
                crate::trace::log(|| format!("midi: {why}"));
                self.failure = Some(why);
            }
        }
    }

    pub fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }

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

    pub fn busy(&self) -> bool {
        !self.batches.is_empty()
    }
}

pub fn read_file(path: &Path) -> Result<MidiFile, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    smf::read(&bytes).map_err(|e| e.to_string())
}

pub enum Export {
    Clips(Vec<Id>),
    Project,
}

pub struct Exported {
    pub bytes: Vec<u8>,
    pub title: &'static str,
    pub name: String,
}

pub fn export(
    project: Project,
    name: String,
    what: Export,
    ctx: &egui::Context,
) -> Receiver<Result<Exported, String>> {
    let (tx, rx) = mpsc::channel();
    let report = tx.clone();
    let ctx = ctx.clone();
    if let Err(error) = crate::spawn::spawn_named("lumen-midi-export", move || {
        let _ = tx.send(make(&project, name, what));
        ctx.request_repaint();
    }) {
        let _ = report.send(Err(crate::spawn::start_failed(
            "making the MIDI file",
            &error,
        )));
    }
    rx
}

fn make(project: &Project, name: String, what: Export) -> Result<Exported, String> {
    let (file, layout, title) = match &what {
        Export::Clips(ids) => (
            midi_export::clips_file(project, ids),
            Layout::Single,
            "Export MIDI clip",
        ),
        Export::Project => (
            midi_export::project_file(project),
            Layout::Parallel,
            "Export MIDI file",
        ),
    };
    let file = file.ok_or("there are no MIDI notes to export")?;
    let name = match (&what, file.tracks.first()) {
        (Export::Clips(_), Some(part)) => part.name.clone().unwrap_or_else(|| "MIDI clip".into()),
        _ => name,
    };
    let bytes = smf::write(&file, layout).map_err(|e| e.to_string())?;
    Ok(Exported { bytes, title, name })
}

pub fn write(
    path: PathBuf,
    bytes: Vec<u8>,
    ctx: &egui::Context,
) -> Receiver<Result<PathBuf, String>> {
    let (tx, rx) = mpsc::channel();
    let report = tx.clone();
    let ctx = ctx.clone();
    if let Err(error) = crate::spawn::spawn_named("lumen-midi-write", move || {
        let result = lumen_project::write_atomic(&path, &bytes)
            .map(|()| path)
            .map_err(|e| e.to_string());
        let _ = tx.send(result);
        ctx.request_repaint();
    }) {
        let _ = report.send(Err(crate::spawn::start_failed(
            "writing the MIDI file",
            &error,
        )));
    }
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
        let written = write(
            path.clone(),
            smf::write(&file, smf::Layout::Single).unwrap(),
            &ctx,
        );
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
