//! The audio clip editor (M9.5 S1, `docs/milestones/M9.5.md`): the Clip
//! tab of an opened audio clip. The clip's whole file is drawn on the
//! timeline, the part it plays bright and the rest dimmed, under a ruler of
//! bars and file time, with the playhead; zoom and scroll are the note
//! editor's gestures, on a view of its own (ADR-0048).
//!
//! - `source`: where the file sits on the timeline (pure, tested);
//! - `paint`: the wave, column by column from the peaks, and samples at
//!   depth; the ruler.
//!
//! What the editor does with the app (`impl App`) is `app::audio_editor`.

pub(crate) mod paint;
pub(crate) mod source;

use eframe::egui::Rect;
use lumen_project::model::Id;

use crate::arrangement::samples::StripReader;
use crate::note_editor::EditorZoom;
use crate::timeview::TimeView;

/// A view the editor is asked to take on its next frame (a command or a
/// header button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fit {
    /// The part of the file the clip plays.
    Clip,
    /// The whole file.
    File,
}

/// The editor's state between frames (not saved).
#[derive(Default)]
pub(crate) struct AudioEditor {
    /// The editor's own horizontal view: its zoom and scroll.
    pub time: TimeView,
    /// The clip the view was last framed for: fitted to the clip when it
    /// first draws.
    pub framed_for: Option<Id>,
    /// Zoom keys waiting for the next frame.
    pub zooms: Vec<EditorZoom>,
    /// A fit waiting for the next frame.
    pub fit: Option<Fit>,
    /// Written while drawing: the pointer is over the editor.
    pub hovered: bool,
    /// What `hovered` was when the frame began.
    pub hovered_before: bool,
    /// Reads samples of files played from disk for the deepest zoom.
    pub reader: StripReader,
    /// What was drawn last (tests).
    pub last: Option<Drawn>,
    /// The opened clip's file on the timeline, and what it was worked out
    /// from: kept until one of those changes.
    pub file: Option<FileCache>,
    /// Samples around the view at the deepest zoom.
    pub samples: Option<SampleCache>,
}

/// [`source::WholeFile`] and what it was worked out from.
pub(crate) struct FileCache {
    pub clip: std::sync::Arc<lumen_project::model::Clip>,
    pub tempo: Vec<lumen_core::time::TempoPoint>,
    pub rate: u32,
    pub frames: u64,
    pub file: source::WholeFile,
}

/// Samples of one file (by its media key) from device-rate frame `first`.
pub(crate) struct SampleCache {
    pub key: u64,
    pub first: i64,
    pub frames: Vec<[i16; 2]>,
}

/// Where the editor drew, last frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Drawn {
    pub clip: Id,
    /// The wave area.
    pub body: Rect,
    pub time: TimeView,
    /// The beats the clip plays, and the beats the file covers.
    pub window: (f64, f64),
    pub file: (f64, f64),
    /// The wave was drawn sample by sample.
    pub samples: bool,
}
