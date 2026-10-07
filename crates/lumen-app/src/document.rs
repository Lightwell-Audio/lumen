//! The open project (M3 slice 3): the model with its undo history, where it
//! is saved, and whether it has changed since.
//!
//! Every edit goes through [`Document::apply`], so it is one undo step
//! (ADR-0024). Transport and view state are not edits: they are stamped into
//! the project when it is saved (slice 3c).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::History;
use lumen_project::model::{Clip, ClipContent, Color, Id, Media, Project, Track, TrackKind};

/// Track colours, cycled for new tracks (`theme::CLIP_COLORS`, as `#RRGGBB`).
fn track_color(index: usize) -> Color {
    let c = crate::theme::CLIP_COLORS[index % crate::theme::CLIP_COLORS.len()];
    Color(u32::from(c.r()) << 16 | u32::from(c.g()) << 8 | u32::from(c.b()))
}

/// An audio file that finished loading, ready to become a track.
#[derive(Debug, Clone)]
pub struct Imported {
    pub media: Id,
    pub path: PathBuf,
    pub name: String,
    /// The file's own format.
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub size_bytes: u64,
    /// Where the clip starts.
    pub at: Ticks,
}

pub struct Document {
    history: History,
    /// The `.lumen` file, once saved or opened.
    pub path: Option<PathBuf>,
    /// The history state that matches the file on disk.
    saved: u64,
    next_id: u64,
    /// Where each media file actually is on this machine.
    pub media_files: HashMap<Id, PathBuf>,
}

impl Document {
    pub fn new(project: Project) -> Self {
        let next_id = highest_id(&project) + 1;
        let history = History::new(project);
        Self {
            saved: history.state(),
            history,
            path: None,
            next_id,
            media_files: HashMap::new(),
        }
    }

    pub fn project(&self) -> &Project {
        self.history.project()
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Identifies the current project state (changes with every edit, undo
    /// and redo).
    pub fn state(&self) -> u64 {
        self.history.state()
    }

    /// Unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.history.state() != self.saved
    }

    /// The project's name for the title bar: its file name, or "Untitled".
    pub fn name(&self) -> String {
        self.path
            .as_deref()
            .and_then(|p| p.file_stem())
            .map_or_else(
                || "Untitled".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            )
    }

    /// The window title: name, a dot while there are unsaved changes, and
    /// the product.
    pub fn title(&self) -> String {
        let dot = if self.is_dirty() { " •" } else { "" };
        format!(
            "{}{dot} — {}",
            self.name(),
            lumen_core::product::display_name()
        )
    }

    /// Records that `state` is what is on disk now (the state when a save
    /// started: edits made during the save stay unsaved).
    pub fn mark_saved_at(&mut self, state: u64) {
        self.saved = state;
    }

    /// A document recovered from an autosave: it belongs to `path` (if the
    /// project had been saved) and has unsaved changes.
    pub fn recovered(
        project: Project,
        path: Option<PathBuf>,
        media_files: HashMap<Id, PathBuf>,
    ) -> Self {
        let mut doc = Self::new(project);
        doc.path = path;
        doc.media_files = media_files;
        // No history state is ever this: the recovered changes are unsaved.
        doc.saved = u64::MAX;
        doc
    }

    /// A document for a project read from `path`.
    pub fn opened(project: Project, path: PathBuf, media_files: HashMap<Id, PathBuf>) -> Self {
        let mut doc = Self::new(project);
        doc.path = Some(path);
        doc.media_files = media_files;
        doc
    }

    /// A new id, unique in this project for its lifetime (never reused,
    /// even after undo).
    pub fn fresh_id(&mut self) -> Id {
        let id = Id(self.next_id);
        self.next_id += 1;
        id
    }

    /// One undo step; see [`History::apply`].
    pub fn apply(
        &mut self,
        label: &str,
        merge: Option<&str>,
        edit: impl FnOnce(&mut Project),
    ) -> bool {
        self.history.apply(label, merge, edit)
    }

    /// Ends the gesture in progress (call when no pointer button is down).
    pub fn seal(&mut self) {
        self.history.seal();
    }

    pub fn undo(&mut self) -> Option<String> {
        self.history.undo()
    }

    pub fn redo(&mut self) -> Option<String> {
        self.history.redo()
    }

    pub fn jump_to(&mut self, done: usize) {
        self.history.jump_to(done);
    }

    /// Adds loaded files as new audio tracks, each with one clip, as one
    /// undo step.
    pub fn import(&mut self, files: Vec<Imported>) {
        if files.is_empty() {
            return;
        }
        let label = match files.as_slice() {
            [one] => format!("Import {}", one.name),
            many => format!("Import {} files", many.len()),
        };
        let mut additions = Vec::new();
        for file in files {
            let (track, clip) = (self.fresh_id(), self.fresh_id());
            self.media_files.insert(file.media, file.path.clone());
            additions.push((track, clip, file));
        }
        self.history.apply(&label, None, |project| {
            for (track, clip, file) in additions {
                let start_seconds = seconds_at(project, file.at);
                let seconds = file.frames as f64 / f64::from(file.sample_rate.max(1));
                let end = ticks_at_seconds(project, start_seconds + seconds);
                project.media.push(Media {
                    id: file.media,
                    path: file.path.to_string_lossy().into_owned(),
                    original_path: None,
                    frames: file.frames,
                    channels: file.channels,
                    sample_rate: file.sample_rate,
                    size_bytes: file.size_bytes,
                });
                let color = track_color(project.tracks.len());
                project.tracks.push(Arc::new(Track {
                    id: track,
                    name: file.name.clone(),
                    kind: TrackKind::Audio,
                    color,
                    parent: None,
                    gain_db: 0.0,
                    pan: 0.0,
                    mute: false,
                    solo: false,
                    devices: vec![],
                    clips: vec![Arc::new(Clip {
                        id: clip,
                        name: file.name,
                        start: file.at,
                        length: (end - file.at).max(Ticks(1)),
                        muted: false,
                        content: ClipContent::Audio {
                            media: file.media,
                            offset: 0,
                            gain_db: 0.0,
                        },
                    })],
                }));
            }
        });
    }
}

fn tempo_map(project: &Project) -> Option<lumen_core::time::TempoMap> {
    lumen_core::time::TempoMap::new(project.tempo.clone()).ok()
}

fn seconds_at(project: &Project, at: Ticks) -> f64 {
    tempo_map(project).map_or(0.0, |m| m.seconds_at(at))
}

fn ticks_at_seconds(project: &Project, seconds: f64) -> Ticks {
    tempo_map(project).map_or(Ticks::ZERO, |m| m.ticks_at_seconds(seconds))
}

/// The highest id used anywhere in `project`.
fn highest_id(project: &Project) -> u64 {
    let tracks = project.tracks.iter().flat_map(|t| {
        std::iter::once(t.id.0)
            .chain(t.devices.iter().map(|d| d.id.0))
            .chain(t.clips.iter().map(|c| c.id.0))
    });
    tracks
        .chain(project.media.iter().map(|m| m.id.0))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(media: Id, name: &str, at: Ticks) -> Imported {
        Imported {
            media,
            path: PathBuf::from(format!("C:/audio/{name}.wav")),
            name: name.into(),
            sample_rate: 48_000,
            channels: 2,
            frames: 96_000,
            size_bytes: 384_044,
            at,
        }
    }

    #[test]
    fn an_import_is_one_valid_undo_step() {
        let mut doc = Document::new(crate::empty_project());
        assert!(!doc.is_dirty());
        let (a, b) = (doc.fresh_id(), doc.fresh_id());
        doc.import(vec![
            file(a, "Kick", Ticks::ZERO),
            file(b, "Snare", Ticks::quarters(4)),
        ]);
        let project = doc.project();
        assert_eq!(project.tracks.len(), 2);
        assert_eq!(project.media.len(), 2);
        // Two seconds at 120 BPM is four quarter notes.
        assert_eq!(project.tracks[0].clips[0].length, Ticks::quarters(4));
        assert_eq!(project.tracks[1].clips[0].start, Ticks::quarters(4));
        assert_eq!(lumen_project::validate(project), Ok(()));
        assert!(doc.is_dirty());
        assert_eq!(doc.history().undo_label(), Some("Import 2 files"));
        doc.undo();
        assert!(doc.project().tracks.is_empty());
        assert!(!doc.is_dirty(), "undoing to the saved state is clean again");
        assert!(doc.title().starts_with("Untitled — "));
        doc.redo();
        assert!(doc.title().starts_with("Untitled •"));
        doc.mark_saved_at(doc.state());
        assert!(!doc.is_dirty());
        doc.undo();
        assert!(doc.is_dirty(), "undoing past a save is a change");
    }

    #[test]
    fn ids_are_never_reused() {
        let mut doc = Document::new(lumen_project::testing::small_project(2));
        let highest = highest_id(doc.project());
        let id = doc.fresh_id();
        assert!(id.0 > highest);
        doc.import(vec![file(id, "Vox", Ticks::ZERO)]);
        doc.undo();
        assert!(doc.fresh_id().0 > id.0);
    }
}
