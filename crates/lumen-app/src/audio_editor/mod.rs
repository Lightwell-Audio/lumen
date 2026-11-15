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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fit {
    Clip,
    File,
    Back,
}

#[derive(Default)]
pub(crate) struct AudioEditor {
    pub time: TimeView,
    pub framed_for: Option<Id>,
    pub zooms: Vec<EditorZoom>,
    pub fit: Option<Fit>,
    pub back: Option<TimeView>,
    pub hovered: bool,
    pub hovered_before: bool,
    pub reader: StripReader,
    pub last: Option<Drawn>,
    pub file: Option<FileCache>,
    pub samples: Option<SampleCache>,
    pub detect: detect::Detection,
    pub gesture: Option<Gesture>,
    pub menu_part: Option<handles::Part>,
    pub reveal: Option<handles::Part>,
    pub focused_before: Option<handles::Part>,
    pub popover: Option<Popover>,
    pub layout: header::Layout,
    pub strip: strip::Layout,
    pub focus_strip: bool,
    pub strip_group: Option<strip::Group>,
    pub strip_fades: bool,
    pub strip_warp: bool,
    pub strip_pitch: bool,
    pub chip_was_open: bool,
    pub cursor: Option<Ticks>,
    pub anchor: Option<Ticks>,
    pub tool: Tool,
    pub pill: Option<Kind>,
    pub hands: Hands,
    pub hover_point: Option<usize>,
    pub brace: Option<BraceDrag>,
    pub brace_hover: Option<brace::BracePart>,
    pub brace_note: Option<&'static str>,
    pub loop_clip: Option<lumen_project::model::Id>,
    pub scrub_sent: Option<(lumen_core::time::Ticks, lumen_engine::ScrubStyle)>,
    pub scrub_drawn: bool,
    pub window_at: Option<crate::settings::WindowPlace>,
    pub crossings: Crossings,
    pub crossed: bool,
    pub envelope_hover: bool,
    pub markers: WarpEditor,
    pub keys: Vec<(
        crate::app::audio_editor::stops::Stop,
        eframe::egui::Key,
        eframe::egui::Modifiers,
    )>,
}

impl AudioEditor {
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

    pub(crate) fn choose_tool(&mut self, tool: Tool) {
        self.tool = tool;
        if tool == Tool::Draw && self.pill.is_none() {
            self.pill = Some(Kind::Gain);
        }
    }

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StripHover {
    Marker(usize),
    Ghost(warp_strip::Ghost),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum StripMenu {
    Marker(usize),
    Hit { source: f64 },
}

#[derive(Debug, Clone)]
pub(crate) struct MarkerDrag {
    pub index: usize,
    pub base: std::sync::Arc<lumen_project::model::Project>,
    pub grab: Ticks,
    pub slide: Option<Slide>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Slide {
    pub source: f64,
    pub beat: f64,
    pub per_beat: f64,
}

#[derive(Default)]
pub(crate) struct WarpEditor {
    pub hover: Option<StripHover>,
    pub drag: Option<MarkerDrag>,
    pub drawn: Option<warp_strip::WarpStrip>,
    pub picked: Option<usize>,
    pub menu: Option<StripMenu>,
    pub keyboard: Option<usize>,
    pub ids: Vec<eframe::egui::Id>,
    pub keys: Vec<(usize, eframe::egui::Key, eframe::egui::Modifiers)>,
    pub reveal: Option<usize>,
    pub focus_next: Option<usize>,
    pub ring: Option<(usize, eframe::egui::Response)>,
    pub focused_before: Option<usize>,
    pub held: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub(crate) struct BraceDrag {
    pub part: brace::BracePart,
    pub base: std::sync::Arc<lumen_project::model::Project>,
    pub grab: Ticks,
}

#[derive(Default)]
pub(crate) struct Crossings {
    pub reader: StripReader,
    pub last: Option<SampleCache>,
}

pub(crate) struct Popover {
    pub part: handles::Part,
    pub text: String,
    pub initial: String,
    pub error: Option<String>,
    pub focused: bool,
}

pub(crate) struct FileCache {
    pub clip: std::sync::Arc<lumen_project::model::Clip>,
    pub tempo: Vec<lumen_core::time::TempoPoint>,
    pub rate: u32,
    pub frames: u64,
    pub file: source::WholeFile,
}

pub(crate) struct SampleCache {
    pub key: u64,
    pub length: u64,
    pub first: i64,
    pub frames: Vec<[i16; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Drawn {
    pub clip: Id,
    pub body: Rect,
    pub time: TimeView,
    pub window: (f64, f64),
    pub file: (f64, f64),
    pub samples: bool,
    pub handles: Option<handles::EditorHandles>,
    pub played: source::Played,
    pub band: Rect,
    pub brace: Option<(f32, f32)>,
}
