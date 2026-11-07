//! The audio clip editor (M9.5 S1, `docs/milestones/M9.5.md`): the Clip
//! tab of an opened audio clip. The clip's whole file is drawn on the
//! timeline, the part it plays bright and the rest dimmed, under a ruler of
//! bars and file time, with the playhead; zoom and scroll are the note
//! editor's gestures, on a view of its own (ADR-0048).
//!
//! - `source`: where the file sits on the timeline (pure, tested);
//! - `handles`: where the grips, fades and gain line are, and what the
//!   pointer is on (pure, tested);
//! - `header`: which of the header's layouts fits (pure, tested);
//! - `paint`: the wave, column by column from the peaks, and samples at
//!   depth; the ruler.
//!
//! What the editor does with the app (`impl App`) is `app::audio_editor`.

pub(crate) mod handles;
pub(crate) mod header;
pub(crate) mod paint;
pub(crate) mod source;

use eframe::egui::Rect;
use lumen_project::model::Id;

use crate::arrangement::samples::StripReader;
use crate::arrangement::select::Gesture;
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
    /// The view before the last fit (X).
    Back,
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
    /// The view before the last fit, for Back.
    pub back: Option<TimeView>,
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
    /// A drag of one of the clip's handles or of its content.
    pub gesture: Option<Gesture>,
    /// A handle pressed with a right-click, whose menu is open.
    pub menu_part: Option<handles::Part>,
    /// A handle to bring into view on the next frame (it took the keyboard,
    /// or a key moved it).
    pub reveal: Option<handles::Part>,
    /// The handle with the keyboard, last frame.
    pub focused_before: Option<handles::Part>,
    /// The typed value of a handle (Enter on it).
    pub popover: Option<Popover>,
    /// The header's layout, last frame.
    pub layout: header::Layout,
    /// The properties column gave its room to the wave.
    pub folded: bool,
}

/// A handle's popover: the value being typed.
pub(crate) struct Popover {
    pub part: handles::Part,
    pub text: String,
    /// What it started as: unchanged, it makes no edit.
    pub initial: String,
    /// Why the text was not taken, until it changes.
    pub error: Option<String>,
    /// The field has been given the keyboard.
    pub focused: bool,
}

/// [`source::WholeFile`] and what it was worked out from.
pub(crate) struct FileCache {
    pub clip: std::sync::Arc<lumen_project::model::Clip>,
    pub tempo: Vec<lumen_core::time::TempoPoint>,
    pub rate: u32,
    pub frames: u64,
    pub file: source::WholeFile,
}

/// Samples of one file (by its media key and length, so a file relinked
/// under the same key is read again) from device-rate frame `first`.
pub(crate) struct SampleCache {
    pub key: u64,
    pub length: u64,
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
    /// From the first beat the clip plays to the last, and the beats the
    /// file covers.
    pub window: (f64, f64),
    pub file: (f64, f64),
    /// The wave was drawn sample by sample.
    pub samples: bool,
    /// Where the clip's handles were.
    pub handles: Option<handles::EditorHandles>,
    /// The beats of the part played first, and of the part after a wrap.
    pub played: source::Played,
}
