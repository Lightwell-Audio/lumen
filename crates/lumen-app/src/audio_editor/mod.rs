//! The audio clip editor (M9.5 S1, `docs/milestones/M9.5.md`): the Clip
//! tab of an opened audio clip. The clip's whole file is drawn on the
//! timeline, the part it plays bright and the rest dimmed, under a ruler of
//! bars and file time, with the playhead; zoom and scroll are the note
//! editor's gestures, on a view of its own (ADR-0048).
//!
//! - `source`: where the file sits on the timeline (pure, tested);
//! - `handles`: where the grips, fades and gain line are, and what the
//!   pointer is on (pure, tested);
//! - `detect`: the transients and tempo of the file, on a worker (S5a);
//! - `header`: which of the header's layouts fits (pure, tested);
//! - `strip`: which of the properties strip's layouts fits (pure, tested);
//! - `paint`: the wave, column by column from the peaks, and samples at
//!   depth; the ruler.
//!
//! What the editor does with the app (`impl App`) is `app::audio_editor`.

pub(crate) mod brace;
pub(crate) mod crossing;
pub(crate) mod detect;
pub(crate) mod handles;
pub(crate) mod header;
pub(crate) mod paint;
pub(crate) mod source;
pub(crate) mod strip;
pub(crate) mod warp_strip;

use eframe::egui::Rect;
use lumen_core::time::Ticks;
use lumen_project::envelope::Kind;
use lumen_project::model::Id;

use crate::arrangement::auto_state::Hands;
use crate::note_editor::Tool;

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
    /// The transients and tempo of the opened clip's file (S5a).
    pub detect: detect::Detection,
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
    /// The properties strip's layout, last frame.
    pub strip: strip::Layout,
    /// Properties (Alt+Enter) asked for the strip's keyboard: Gain takes it
    /// on the next frame, or Mute when a lock disables Gain (D32).
    pub focus_strip: bool,
    /// The strip's group that had the keyboard last frame, so a fold moves
    /// the keyboard to the ⋯ button instead of dropping it (D36).
    pub strip_group: Option<strip::Group>,
    /// The ⋯ menu shows its Fades page.
    pub strip_fades: bool,
    /// The ⋯ menu is on its Warp page (S5a).
    pub strip_warp: bool,
    /// The ⋯ menu is on its Pitch page (S6).
    pub strip_pitch: bool,
    /// The Clip chip's popover was open last frame: closing it applies a
    /// name that was being typed.
    pub chip_was_open: bool,
    /// The edit cursor (M9.5 CU A): a beat on the heard stretch. Not saved;
    /// it goes when another clip opens or a range is picked.
    pub cursor: Option<Ticks>,
    /// Where a range picked from the keyboard is anchored (D28).
    pub anchor: Option<Ticks>,
    /// The tool in hand: Pointer, or Draw for the envelope.
    pub tool: Tool,
    /// The envelope shown over the wave (the pills, DR A).
    pub pill: Option<Kind>,
    /// The envelope's gesture, picks and keyboard focus: the editor's
    /// own, so the arrangement's lanes keep theirs (D20).
    pub hands: Hands,
    /// The envelope point under the pointer.
    pub hover_point: Option<usize>,
    /// A drag of the brace's end: which, and the project it began on.
    pub brace: Option<BraceDrag>,
    /// The brace part under the pointer, last frame.
    pub brace_hover: Option<brace::BracePart>,
    /// Why the brace's start stopped (LS A), while it holds there.
    pub brace_note: Option<&'static str>,
    /// The clip Loop clip repeats, while it is on (M9.5 S2c, LC A).
    pub loop_clip: Option<lumen_project::model::Id>,
    /// What the ruler's scrub last sent to the engine: where, and how.
    pub scrub_sent: Option<(lumen_core::time::Ticks, lumen_engine::ScrubStyle)>,
    /// Whether the ruler was drawn since the last frame began, so a scrub
    /// whose ruler went away can be let go of.
    pub scrub_drawn: bool,
    /// Where the pop-out window opened, taken once as it opens.
    pub window_at: Option<crate::settings::WindowPlace>,
    /// Frames read for zero crossings (D23).
    pub crossings: Crossings,
    /// An edge landed on a crossing this frame: its guide lights.
    pub crossed: bool,
    /// The pointer is over the envelope, which says what it does.
    pub envelope_hover: bool,
    /// The warp strip's pointer, keys and focus (M9.5 S5b).
    pub markers: WarpEditor,
    /// Keys taken for the wave, envelope and loop stops, for the next frame.
    pub keys: Vec<(
        crate::app::audio_editor::stops::Stop,
        eframe::egui::Key,
        eframe::egui::Modifiers,
    )>,
}

impl AudioEditor {
    /// Lights a pill, or puts it out: one envelope at a time. Without an
    /// envelope the pencil has nothing to draw, so Pointer comes back.
    pub(crate) fn toggle_pill(&mut self, kind: Kind) {
        self.pill = if self.pill == Some(kind) {
            None
        } else {
            Some(kind)
        };
        self.hands.picks = None;
        self.hover_point = None;
        if self.pill.is_none() {
            self.tool = Tool::Pointer;
        }
    }

    /// Chooses `tool`. Draw with no pill lit lights Gain, so the pencil
    /// always works (DR A).
    pub(crate) fn choose_tool(&mut self, tool: Tool) {
        self.tool = tool;
        if tool == Tool::Draw && self.pill.is_none() {
            self.pill = Some(Kind::Gain);
        }
    }

    /// B in the editor: Pointer and Draw.
    pub(crate) fn toggle_draw(&mut self) {
        self.choose_tool(match self.tool {
            Tool::Pointer => Tool::Draw,
            Tool::Draw => Tool::Pointer,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_lights_gain_and_putting_the_pill_out_brings_the_pointer_back() {
        let mut ed = AudioEditor::default();
        ed.toggle_draw();
        assert_eq!((ed.tool, ed.pill), (Tool::Draw, Some(Kind::Gain)));
        ed.toggle_pill(Kind::Pan);
        assert_eq!((ed.tool, ed.pill), (Tool::Draw, Some(Kind::Pan)));
        ed.toggle_pill(Kind::Pan);
        assert_eq!((ed.tool, ed.pill), (Tool::Pointer, None));
    }
}

/// What the pointer is on in the warp strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StripHover {
    Marker(usize),
    Ghost(warp_strip::Ghost),
}

/// What a right-click on the warp strip opened its menu on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StripMenu {
    /// The marker at this index.
    Marker(usize),
    /// The transient at `source` seconds of the file.
    Hit { source: f64 },
}

/// A drag of one warp marker: the project it began on (the drag redoes its
/// edit from there each frame, so it is one undo step), how far the marker
/// was from the pointer when it was picked up, and for a slide where the
/// audio under it started.
#[derive(Debug, Clone)]
pub(crate) struct MarkerDrag {
    pub index: usize,
    pub base: std::sync::Arc<lumen_project::model::Project>,
    pub grab: Ticks,
    pub slide: Option<Slide>,
}

/// A slide: the second the marker pinned, the beat the pointer pressed at,
/// and the seconds of the file in a beat there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Slide {
    pub source: f64,
    pub beat: f64,
    pub per_beat: f64,
}

/// The warp strip's state between frames (not saved).
#[derive(Default)]
pub(crate) struct WarpEditor {
    pub hover: Option<StripHover>,
    pub drag: Option<MarkerDrag>,
    /// The strip and the x of each marker's flag, as drawn last (tests).
    pub drawn: Option<warp_strip::WarpStrip>,
    /// The marker last picked or moved: its hairline is lit.
    pub picked: Option<usize>,
    /// The marker a right-click opened its menu on, or the transient it did.
    pub menu: Option<StripMenu>,
    /// The marker that last had the keyboard. A command run from the palette
    /// acts on it: the palette's field holds the keyboard by then. Another of
    /// the editor's stops taking it clears this.
    pub keyboard: Option<usize>,
    /// The markers' Tab stops, drawn last frame, in time order.
    pub ids: Vec<eframe::egui::Id>,
    /// Keys taken for the focused marker, for the next frame.
    pub keys: Vec<(usize, eframe::egui::Key, eframe::egui::Modifiers)>,
    /// A marker to bring into view on the next frame.
    pub reveal: Option<usize>,
    /// A marker to give the keyboard on the next frame (one just added, or the
    /// one before a deleted marker).
    pub focus_next: Option<usize>,
    /// The marker stop that has the keyboard, set while the stops are made
    /// and taken by the drawing of the strip in the same frame.
    pub ring: Option<(usize, eframe::egui::Response)>,
    /// The marker with the keyboard, last frame.
    pub focused_before: Option<usize>,
    /// The drag stopped at a speed limit (or the clip's start would leave its
    /// file), and what it says.
    pub held: Option<&'static str>,
}

/// A drag of one of the brace's ends.
#[derive(Debug, Clone)]
pub(crate) struct BraceDrag {
    pub part: brace::BracePart,
    pub base: std::sync::Arc<lumen_project::model::Project>,
    /// How far the end was from the pointer when it was picked up.
    pub grab: Ticks,
}

/// The zero-crossing reads: their own reader, so a display read and a
/// crossing read do not replace each other, and the last frames read.
#[derive(Default)]
pub(crate) struct Crossings {
    pub reader: StripReader,
    pub last: Option<SampleCache>,
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
    /// The loop brace's band, and its ends while the clip loops.
    pub band: Rect,
    pub brace: Option<(f32, f32)>,
}
