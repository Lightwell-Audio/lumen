pub(crate) mod audio_in;
pub(crate) mod audio_take_look;
pub(crate) mod auto_envelope;
pub(crate) mod auto_params;
pub(crate) mod auto_scale;
pub(crate) mod auto_state;
mod auto_tools;
pub(crate) mod auto_view;
mod buttons;
mod clip_look;
pub(crate) mod clip_marks;
mod clips;
pub(crate) mod control_menu;
mod drop;
pub(crate) mod follow;
mod frozen_badge;
mod glide;
pub mod handles;
mod header_row;
mod headers;
mod levels;
mod listen_light;
mod menus;
mod midi_io;
mod mini_notes;
mod monitor_badge;
mod nesting;
mod pinned;
pub(crate) mod recording_look;
mod reorder;
mod row_glide;
mod rows;
pub(crate) mod samples;
pub mod select;
mod selection_look;
mod sends;
mod strip;
mod strip_output;
pub(crate) mod strips;
mod tracks;
mod warp_ticks;
pub(crate) mod waves;

pub(crate) use listen_light::{Live, shown as listen_light_shown};

pub use drop::FileDrop;
pub use samples::Update as StripUpdate;

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{self, Color32};
use lumen_project::model::{Clip, Color, Id, Track};

pub const HEADER_WIDTH: f32 = crate::theme::size::HEADER_COLUMN;

pub const MAX_HEADER_WIDTH: f32 = 480.0;

pub const STEP: f32 = crate::theme::size::TRACK_STEP;

pub const FOLDED: f32 = 2.0 * STEP;

pub const UNFOLDED: f32 = 4.0 * STEP;

const MAX_HEIGHT: f32 = 14.0 * STEP;

const FOLD_BELOW: f32 = UNFOLDED;

fn snap_height(height: f32) -> f32 {
    (height / STEP).round() * STEP
}

pub(crate) const TITLE: f32 = 15.0;

pub(crate) const INDENT: f32 = 12.0;

impl Arrangement {
    pub fn note_undo(&mut self) {
        self.glides.note_undo();
    }

    pub(crate) fn drawn_start(
        &self,
        project: &lumen_project::model::Project,
        id: Id,
        now: f64,
    ) -> Option<lumen_core::time::Ticks> {
        let shown = self.glides.shown(project, now);
        shown
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == id)
            .map(|c| c.start)
    }

    pub(crate) fn wash(&self, id: Id, now: f64) -> f32 {
        self.glides.wash(id, now)
    }
}

pub(crate) fn indent(depth: usize) -> f32 {
    INDENT * depth.min(lumen_project::groups::MAX_NESTING) as f32
}

#[derive(Debug, Clone, Copy)]
struct Row {
    kind: RowKind,
    y: f32,
    height: f32,
    zone: Zone,
    clipped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RowKind {
    Track(usize),
    Pending(usize),
    Master,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CueControls {
    pub mode: bool,
    pub ready: bool,
    pub volume_db: f32,
}

impl CueControls {
    pub fn cueing(self) -> bool {
        self.mode && self.ready
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    Main,
    Returns,
    Master,
}

#[derive(Debug)]
pub(crate) struct DeviceDrop {
    pub to: Id,
    pub result: Result<Id, crate::document::Refusal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputNote {
    pub text: String,
    pub warn: bool,
}

#[derive(Debug, Default)]
pub struct Arrangement {
    pub(crate) tool: crate::app::tools::ArrangementTool,
    pub(crate) tool_keys: bool,
    pub top: f32,
    pub output_notes: std::collections::HashMap<Id, OutputNote>,
    glides: glide::ClipGlides,
    row_glides: row_glide::RowGlides,
    pub heights: BTreeMap<Id, f32>,
    pub expanded: BTreeSet<Id>,
    pub collapsed: BTreeSet<Id>,
    pub selected: Vec<Id>,
    anchor: Option<Id>,
    pub header_clicked: Option<Id>,
    renaming: Option<(Id, String)>,
    renaming_in_strip: bool,
    resizing: Option<(Id, f32)>,
    hot_edge: Option<(Id, f32)>,
    gpu: clips::GpuClips,
    pub(crate) drum_pictures: std::collections::BTreeMap<Id, (usize, usize, f32)>,
    drum_cache: std::collections::BTreeMap<Id, crate::drum_editor::arrangement::Picture>,
    fitted: Option<(BTreeMap<Id, f32>, BTreeSet<Id>, f32)>,
    drag: Option<reorder::Drag>,
    pub(crate) device_drop: Option<DeviceDrop>,
    pub(crate) browser_drop: Option<(String, Id)>,
    pub(crate) auto: auto_state::AutoView,
    pub clips: BTreeSet<Id>,
    pub gesture: Option<select::Gesture>,
    pub hover: Option<select::Hover>,
    pub marquee: Option<egui::Rect>,
    pub range: Option<select::TimeRange>,
    pub(crate) import_requested: bool,
    pub(crate) bounce_requested: bool,
    pub(crate) track_command: Option<crate::commands::Command>,
    pub(crate) clip_cursor: Option<Id>,
    pub(crate) insert: Option<Id>,
    pub(crate) menu_spot: Option<(lumen_core::time::Ticks, Option<Id>)>,
    pub(crate) info: Option<crate::app::info::Info>,
    pub(crate) note: Option<&'static str>,
    width: Option<f32>,
    view_height: f32,
    returns_size: Option<f32>,
    pub(crate) returns_folded: bool,
    returns_top: f32,
    sizing_returns: Option<f32>,
    hot_return: Option<Id>,
    hot_return_next: Option<Id>,
    senders: BTreeMap<Id, usize>,
    held_send: Option<(Id, Id)>,
    held_next: Option<(Id, Id)>,
    sender_order: BTreeMap<Id, Vec<Id>>,
    pub(crate) cued: BTreeSet<Id>,
    pub(crate) cue: CueControls,
    pub(crate) additive_solo: bool,
    pub(crate) live: Live,
    pub(crate) recording: Option<recording_look::RecordingLook>,
    pub(crate) audio_takes: Vec<audio_take_look::AudioTakeLook>,
    pub(crate) preview: Option<std::sync::Arc<lumen_project::model::Project>>,
    pub(crate) meters: crate::meters::Meters,
    pub(crate) track_colour_meters: bool,
    pub(crate) outputs: OutputChoices,
    pub(crate) inputs: audio_in::InputChoices,
    mixer_offset: f32,
    mixer_push: f32,
    mixer_scroll_to: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OutputChoices {
    pub pairs: Vec<(u16, String)>,
    pub main: Option<u16>,
    pub cue: Option<u16>,
    pub status: Option<String>,
    pub wanted: bool,
}

fn fit_height(visible: f32, count: f32) -> f32 {
    ((visible / count / STEP).floor() * STEP).clamp(FOLDED, MAX_HEIGHT)
}

pub(crate) fn color_of(c: Color) -> Color32 {
    crate::theme::colour_of(c.0)
}

pub(crate) fn clip_color(track: &Track, clip: &Clip) -> Color32 {
    if clip.muted {
        crate::theme::CLIP_MUTED
    } else {
        color_of(clip.color.unwrap_or(track.color))
    }
}

pub(crate) fn color_from(c: Color32) -> Color {
    Color(u32::from(c.r()) << 16 | u32::from(c.g()) << 8 | u32::from(c.b()))
}

pub(crate) fn color_to(c: Color) -> Color32 {
    Color32::from_rgb(
        ((c.0 >> 16) & 0xFF) as u8,
        ((c.0 >> 8) & 0xFF) as u8,
        (c.0 & 0xFF) as u8,
    )
}

impl Arrangement {
    pub fn header_width(&self) -> f32 {
        self.width.unwrap_or(HEADER_WIDTH)
    }

    pub fn set_header_width(&mut self, width: f32) {
        let width = width.clamp(HEADER_WIDTH, MAX_HEADER_WIDTH).round();
        self.width = (width != HEADER_WIDTH).then_some(width);
    }

    pub fn save_into(&self, view: &mut lumen_project::model::ViewState) {
        view.header_width = self.header_width();
        view.top = self.top;
        view.track_heights = self.heights.clone();
        view.expanded = self.expanded.clone();
        view.collapsed = self.collapsed.clone();
        view.returns_height = self.returns_size;
        view.returns_folded = self.returns_folded;
    }

    pub fn restore(view: &lumen_project::model::ViewState) -> Self {
        Self {
            top: view.top.max(0.0),
            heights: view
                .track_heights
                .iter()
                .map(|(id, h)| (*id, snap_height(h.clamp(FOLD_BELOW, MAX_HEIGHT))))
                .collect(),
            expanded: view.expanded.clone(),
            collapsed: view.collapsed.clone(),
            width: (view.header_width.is_finite() && view.header_width != HEADER_WIDTH)
                .then(|| view.header_width.clamp(HEADER_WIDTH, MAX_HEADER_WIDTH)),
            returns_size: view
                .returns_height
                .filter(|h| h.is_finite())
                .map(|h| h.max(0.0)),
            returns_folded: view.returns_folded,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::headers::track_description;
    use super::*;

    use eframe::egui::Modifiers;
    use lumen_project::model::TrackKind;

    use crate::document::Document;

    #[test]
    fn a_clip_no_longer_drawn_leaves_the_drum_cache() {
        let mut arrangement = Arrangement::default();
        let picture = || crate::drum_editor::arrangement::Picture::default();
        arrangement.drum_cache.insert(Id(1), picture());
        arrangement.drum_cache.insert(Id(2), picture());
        let mut drawn = std::collections::BTreeMap::new();
        drawn.insert(Id(1), (2, 3, 1.5));
        arrangement.prune_drum_cache(&drawn);
        assert!(arrangement.drum_cache.contains_key(&Id(1)), "still drawn");
        assert!(
            !arrangement.drum_cache.contains_key(&Id(2)),
            "no longer drawn, so gone"
        );
    }

    #[test]
    fn fitted_heights_are_whole_steps_that_fit() {
        assert_eq!(fit_height(500.0, 10.0), 44.0);
        assert_eq!(fit_height(500.0, 3.0), 154.0);
        assert_eq!(fit_height(5_000.0, 1.0), MAX_HEIGHT);
        assert_eq!(fit_height(100.0, 50.0), FOLDED);
    }

    #[test]
    fn heights_come_in_whole_steps() {
        assert_eq!(snap_height(95.0), 4.0 * STEP);
        assert_eq!(snap_height(100.0), 5.0 * STEP);
        assert_eq!(FOLDED, 2.0 * STEP);
        assert_eq!(UNFOLDED, 4.0 * STEP);
        let mut view = lumen_project::model::ViewState::default();
        view.track_heights.insert(Id(1), 150.0);
        view.track_heights.insert(Id(2), 20.0);
        let a = Arrangement::restore(&view);
        assert_eq!(a.heights[&Id(1)], 7.0 * STEP);
        assert_eq!(a.heights[&Id(2)], UNFOLDED, "never below the unfolded size");
    }

    #[test]
    fn heights_fold_and_unfold() {
        let mut a = Arrangement::default();
        let id = Id(7);
        assert_eq!(a.target_height(id), FOLDED);
        a.toggle_fold(id);
        assert_eq!(a.target_height(id), UNFOLDED);
        a.heights.insert(id, 180.0);
        assert_eq!(a.target_height(id), 180.0);
        a.toggle_fold(id);
        assert_eq!(
            a.target_height(id),
            FOLDED,
            "folding keeps the unfolded height for later"
        );
        a.toggle_fold(id);
        assert_eq!(a.target_height(id), 180.0);
    }

    #[test]
    fn tracks_move_with_alt_and_the_arrow_keys() {
        let mut doc = Document::new(crate::empty_project());
        let ids: Vec<Id> = (0..3)
            .map(|_| doc.add_track(TrackKind::Audio, None))
            .collect();
        let mut a = Arrangement {
            selected: vec![ids[2]],
            ..Arrangement::default()
        };
        let order =
            |doc: &Document| -> Vec<Id> { doc.project().tracks.iter().map(|t| t.id).collect() };
        a.move_selected(&mut doc, -1);
        assert_eq!(order(&doc), [ids[0], ids[2], ids[1]]);
        a.move_selected(&mut doc, -1);
        a.move_selected(&mut doc, -1);
        assert_eq!(order(&doc), [ids[2], ids[0], ids[1]], "stops at the top");
        a.move_selected(&mut doc, 1);
        assert_eq!(order(&doc), [ids[0], ids[2], ids[1]]);
    }

    #[test]
    fn returns_stay_pinned_and_offer_no_move() {
        let mut doc = Document::new(crate::empty_project());
        let audio = doc.add_track(TrackKind::Audio, None);
        let send = doc.add_track(TrackKind::Return, None);
        let mut a = Arrangement {
            selected: vec![send],
            ..Arrangement::default()
        };
        assert!(!a.can_move_selected(&doc, -1));
        assert!(!a.can_move_selected(&doc, 1));
        a.move_selected(&mut doc, -1);
        let order: Vec<Id> = doc.project().tracks.iter().map(|t| t.id).collect();
        assert_eq!(order, [audio, send], "the return did not move");
        a.selected = vec![audio];
        assert!(!a.can_move_selected(&doc, -1), "already at the top");
        assert!(
            !a.can_move_selected(&doc, 1),
            "a track does not go below the returns"
        );
        let second = doc.add_track(TrackKind::Audio, None);
        a.selected = vec![second, send];
        a.move_selected(&mut doc, -1);
        let order: Vec<Id> = doc.project().tracks.iter().map(|t| t.id).collect();
        assert_eq!(order, [second, audio, send]);
    }

    #[test]
    fn view_state_round_trips() {
        let mut a = Arrangement {
            top: 120.0,
            ..Arrangement::default()
        };
        a.expanded.insert(Id(3));
        a.heights.insert(Id(3), 7.0 * STEP);
        a.set_header_width(300.0);
        let mut view = lumen_project::model::ViewState::default();
        a.save_into(&mut view);
        let b = Arrangement::restore(&view);
        assert_eq!(
            (
                b.top,
                b.expanded.clone(),
                b.heights.clone(),
                b.header_width()
            ),
            (a.top, a.expanded, a.heights, 300.0)
        );
        let mut d = Arrangement {
            returns_size: Some(110.0),
            returns_folded: true,
            ..Arrangement::default()
        };
        d.save_into(&mut view);
        assert_eq!(
            (view.returns_height, view.returns_folded),
            (Some(110.0), true)
        );
        let e = Arrangement::restore(&view);
        assert_eq!((e.returns_size, e.returns_folded), (Some(110.0), true));
        d.returns_size = None;
        d.save_into(&mut view);
        assert_eq!(view.returns_height, None, "natural size is not written");
        let mut c = Arrangement::default();
        c.set_header_width(2_000.0);
        assert_eq!(c.header_width(), MAX_HEADER_WIDTH);
        c.set_header_width(10.0);
        assert_eq!(c.header_width(), HEADER_WIDTH);
    }

    #[test]
    fn returns_come_after_the_tracks_and_new_ones_after_the_last_return() {
        let mut doc = Document::new(crate::empty_project());
        let mut a = Arrangement::default();
        a.add_track(&mut doc, TrackKind::Audio);
        a.add_track(&mut doc, TrackKind::Return);
        let first_return = a.selected[0];
        a.add_track(&mut doc, TrackKind::Audio);
        a.add_track(&mut doc, TrackKind::Return);
        let second_return = a.selected[0];
        a.add_track(&mut doc, TrackKind::Midi);
        let kinds = |a: &Arrangement, doc: &Document| -> Vec<TrackKind> {
            a.visible(doc.project()).map(|t| t.kind).collect()
        };
        assert_eq!(
            kinds(&a, &doc),
            [
                TrackKind::Audio,
                TrackKind::Audio,
                TrackKind::Midi,
                TrackKind::Return,
                TrackKind::Return
            ]
        );
        let returns: Vec<Id> = doc
            .project()
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Return)
            .map(|t| t.id)
            .collect();
        assert_eq!(returns, [first_return, second_return]);
        assert_eq!(
            pinned::letter_of(doc.project(), second_return).as_deref(),
            Some("B")
        );
        a.returns_folded = true;
        assert_eq!(kinds(&a, &doc).len(), 3);
        a.add_track(&mut doc, TrackKind::Return);
        assert!(!a.returns_folded, "adding a return shows them");
    }

    #[test]
    fn the_keyboard_alone_can_select_tracks() {
        let tracks = [Id(1), Id(2), Id(3)];
        let mut a = Arrangement::default();
        a.step_through(&tracks, 1, false);
        assert_eq!(
            a.selected,
            [Id(1)],
            "Down with nothing selected starts at the top"
        );
        a.step_through(&tracks, 1, false);
        assert_eq!(a.selected, [Id(2)]);
        a.step_through(&tracks, 1, true);
        assert_eq!(a.selected, [Id(2), Id(3)], "Shift extends");
        a.step_through(&tracks, 1, false);
        assert_eq!(a.selected, [Id(3)], "stops at the bottom");
        let mut b = Arrangement::default();
        b.step_through(&tracks, -1, false);
        assert_eq!(
            b.selected,
            [Id(3)],
            "Up with nothing selected starts at the bottom"
        );
    }

    #[test]
    fn headers_describe_themselves_to_screen_readers() {
        let mut doc = Document::new(crate::empty_project());
        let id = doc.add_track(TrackKind::Midi, None);
        doc.set_mute(&[id], true);
        doc.set_armed(&[id], true);
        let track = doc.project().tracks[0].clone();
        assert_eq!(
            track_description(&track),
            "Track MIDI 1, MIDI track, muted, armed for recording"
        );
    }

    #[test]
    fn selection_follows_clicks() {
        let mut doc = Document::new(crate::empty_project());
        let ids: Vec<Id> = (0..4)
            .map(|_| doc.add_track(TrackKind::Audio, None))
            .collect();
        let mut a = Arrangement::default();
        let project = doc.project();
        a.click_select(project, ids[1], Modifiers::NONE);
        assert_eq!(a.selected, [ids[1]]);
        a.click_select(project, ids[3], Modifiers::SHIFT);
        assert_eq!(a.selected, [ids[1], ids[2], ids[3]]);
        a.click_select(project, ids[2], Modifiers::COMMAND);
        assert_eq!(a.selected, [ids[1], ids[3]]);
        assert_eq!(
            a.targets(ids[3]),
            [ids[1], ids[3]],
            "buttons act on the selection"
        );
        assert_eq!(
            a.targets(ids[0]),
            [ids[0]],
            "or on a track outside it alone"
        );
    }
}
