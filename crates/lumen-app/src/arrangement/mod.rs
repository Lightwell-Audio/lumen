//! The arrangement's tracks (M3 slice 4a): the header column beside the
//! timeline, and the lanes with their clips.
//!
//! Design (owner's choice from rendered options, 2026-10-07):
//! - **Headers:** a colour rail, the name, and mute, solo and a round
//!   record-arm button always visible.
//! - **Clips:** dark, with a track-coloured title bar and a bright waveform
//!   (the waveform comes in slice 4c).
//! - **Hints:** an armed track's lane has a faint red wash and a soloed
//!   track's a faint blue one; tracks that cannot be heard (muted, or not
//!   soloed while another track is) have their clips dimmed.
//!
//! Tracks fold and unfold. Folded is a compact row; unfolded
//! is taller, with volume and pan in the header (automation lanes join it in
//! M7). Dragging a track's bottom edge resizes it: a folded track dragged
//! taller unfolds, and an unfolded one dragged small folds. Heights glide
//! (motion: GLIDE) and are saved with the project, but are not undo steps.
//!
//! Groups (M4 slice 3, the owner's H1): a group's tracks follow it, stepped
//! in by [`INDENT`] a level, with the colour rail of every group they are
//! in running down beside them. A group's arrow hides or shows its tracks
//! (`collapsed`, saved with the project); its height is still dragged.
//!
//! Returns and the master (M4 slice 4, the owner's A1) are pinned at the
//! bottom, under a divider (`pinned`): rows carry the [`Zone`] they are
//! drawn in, and each zone clips its own.

pub(crate) mod auto_envelope;
pub(crate) mod auto_params;
pub(crate) mod auto_scale;
pub(crate) mod auto_state;
mod auto_tools;
pub(crate) mod auto_view;
mod buttons;
mod clips;
pub(crate) mod control_menu;
mod drop;
pub(crate) mod follow;
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
mod reach;
pub(crate) mod recording_look;
mod reorder;
mod row_glide;
mod rows;
mod samples;
pub mod select;
mod selection_look;
mod sends;
mod strip;
mod strip_output;
pub(crate) mod strips;
mod tracks;
mod waves;

pub(crate) use listen_light::{Live, shown as listen_light_shown};

pub use drop::FileDrop;
pub use samples::Update as StripUpdate;

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{self, Color32};
use lumen_project::model::{Clip, Color, Id, Track};

/// Width of the header column, unless dragged (MT-MIX-3 run 1: names were
/// cut short); double-clicking its edge comes back to this.
pub const HEADER_WIDTH: f32 = crate::theme::size::HEADER_COLUMN;

/// The widest the header column can be dragged.
pub const MAX_HEADER_WIDTH: f32 = 480.0;

/// Track heights come in steps of this (owner request): elements drawn by
/// height later (automation lanes, meters) can rely on whole steps.
pub const STEP: f32 = crate::theme::size::TRACK_STEP;

/// A folded track: two steps.
pub const FOLDED: f32 = 2.0 * STEP;

/// An unfolded track, and the smallest one: room for the volume and pan
/// row. Four steps.
pub const UNFOLDED: f32 = 4.0 * STEP;

/// The tallest a track can be dragged: fourteen steps.
const MAX_HEIGHT: f32 = 14.0 * STEP;

/// Dragging an unfolded track below this folds it.
const FOLD_BELOW: f32 = UNFOLDED;

/// `height` rounded to whole steps.
fn snap_height(height: f32) -> f32 {
    (height / STEP).round() * STEP
}

/// The clip title bar.
pub(crate) const TITLE: f32 = 15.0;

/// How far a group's tracks step in, a level (the owner's H1).
pub(crate) const INDENT: f32 = 12.0;

/// The indent of a track `depth` groups deep.
impl Arrangement {
    /// The next document change comes from undo or redo: what it changes
    /// glides back and is washed in the accent (M4.5).
    pub fn note_undo(&mut self) {
        self.glides.note_undo();
    }

    /// Where clip `id` is drawn at `now` (for tests).
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

    /// How strongly clip `id` is washed at `now` (for tests).
    pub(crate) fn wash(&self, id: Id, now: f64) -> f32 {
        self.glides.wash(id, now)
    }
}

pub(crate) fn indent(depth: usize) -> f32 {
    // Groups go three deep (edits keep to it); an older, deeper project
    // stops stepping in there, so names keep room.
    INDENT * depth.min(lumen_project::groups::MAX_NESTING) as f32
}

/// M, S and arm on a header (the interface audit's S4: 19, against the
/// strip's 20).
const BUTTON: f32 = crate::theme::size::CONTROL_SMALL;

/// One row of the arrangement, in content coordinates (on screen at the
/// lanes' top + `y` − the scroll), drawn inside its zone.
#[derive(Debug, Clone, Copy)]
struct Row {
    kind: RowKind,
    y: f32,
    height: f32,
    zone: Zone,
    /// Shorter than its content while it folds shut: drawn clipped to its
    /// height (M4.5).
    clipped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RowKind {
    /// Index into the project's tracks.
    Track(usize),
    /// Index into the files loading for import.
    Pending(usize),
    /// The master track (`Project::master`).
    Master,
}

/// The cue's settings as the headers use them (slice 4): the master's
/// Solo/Cue switch, whether a cue pair is open, and the cue level.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CueControls {
    pub mode: bool,
    pub ready: bool,
    pub volume_db: f32,
}

impl CueControls {
    /// The S buttons are headphone buttons now.
    pub fn cueing(self) -> bool {
        self.mode && self.ready
    }
}

/// Where a row is drawn: the scrolling tracks, or the area pinned at the
/// bottom (the returns, then the master).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    Main,
    Returns,
    Master,
}

/// The arrangement's view state: vertical scroll, heights, folding,
/// selection and the track being renamed.
/// A device card dropped on a track's header, and what became of it.
#[derive(Debug)]
pub(crate) struct DeviceDrop {
    /// The track it was dropped on.
    pub to: Id,
    /// The device now on that track (the card itself, or its copy).
    pub result: Result<Id, crate::document::Refusal>,
}

/// What a track that plays a plugin's output says about where its sound comes
/// from, and whether that is not working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputNote {
    pub text: String,
    /// The instrument is not running: the track plays nothing.
    pub warn: bool,
}

#[derive(Debug, Default)]
pub struct Arrangement {
    /// The global arrangement tool (M7b E0).
    pub(crate) tool: crate::app::tools::ArrangementTool,
    /// A lane press or explicit tool choice gives B to the arrangement;
    /// opening a MIDI editor gives it back to that editor.
    pub(crate) tool_keys: bool,
    /// Vertical scroll, in points.
    pub top: f32,
    /// What each track that plays a plugin's output says of it, by track
    /// (M6 slice 6d): set every frame by the app, which knows the plugins.
    pub output_notes: std::collections::HashMap<Id, OutputNote>,
    /// Clips gliding to new places, and what undo changed (M4.5).
    glides: glide::ClipGlides,
    /// Rows gliding to new places, and new ones growing in (M4.5).
    row_glides: row_glide::RowGlides,
    /// Unfolded heights of tracks that were resized.
    pub heights: BTreeMap<Id, f32>,
    /// Unfolded tracks.
    pub expanded: BTreeSet<Id>,
    /// Groups folded to hide their tracks (slice M4 3).
    pub collapsed: BTreeSet<Id>,
    /// Selected tracks, in the order they were selected.
    pub selected: Vec<Id>,
    /// Where a Shift-click range starts.
    anchor: Option<Id>,
    /// The track being renamed, with the text so far.
    renaming: Option<(Id, String)>,
    /// The rename field is on the track's mixer strip, not its header
    /// (a double-click or the menu on the strip, M4 slice 5).
    renaming_in_strip: bool,
    /// The track being resized and its height when the drag began.
    resizing: Option<(Id, f32)>,
    /// The track whose bottom edge glows (hovered or dragged), and how
    /// strongly, so the lane can continue the glow.
    hot_edge: Option<(Id, f32)>,
    /// Clip bodies on the GPU (slice 4b).
    gpu: clips::GpuClips,
    /// Heights, folding and scroll before H fitted every track (H again
    /// restores them).
    fitted: Option<(BTreeMap<Id, f32>, BTreeSet<Id>, f32)>,
    /// Tracks being dragged to a new place (slice 4d).
    drag: Option<reorder::Drag>,
    /// A device card was dropped on a track's header: the app follows it
    /// (shows the track, or says why it was refused).
    pub(crate) device_drop: Option<DeviceDrop>,
    /// A device from the browser was let go over a track (its header or its
    /// lane): the browser's id of the device, and the track.
    pub(crate) browser_drop: Option<(String, Id)>,
    /// The automation lanes shown over the clip lanes (M7 slice A1).
    pub(crate) auto: auto_state::AutoView,
    /// Selected clips (slice 6a). Selecting clips clears the track
    /// selection, and the other way round.
    pub clips: BTreeSet<Id>,
    /// The clip drag or marquee in progress.
    pub gesture: Option<select::Gesture>,
    /// The clip under the pointer, its region and the side it would trim,
    /// for the H1 hover and the L3 handles.
    pub hover: Option<select::Hover>,
    /// The marquee being drawn, on screen.
    pub marquee: Option<egui::Rect>,
    /// The selected time range (selecting one clears the clip selection,
    /// and the other way round).
    pub range: Option<select::TimeRange>,
    /// Import audio or MIDI was chosen in a track's menu: the app runs it next
    /// frame, onto that track (now the selection).
    pub(crate) import_requested: bool,
    /// The clip the arrow keys last chose (M3 review H2): where the next
    /// arrow goes from while it is selected.
    pub(crate) clip_cursor: Option<Id>,
    /// The track last clicked in the lanes, on a clip or empty: Paste
    /// lands there (at the playhead) unless tracks are selected: the
    /// insert point sits on a track (owner, after M4 slice 7).
    pub(crate) insert: Option<Id>,
    /// Where the lanes were right-clicked, for the menu's Paste: the time
    /// (snapped) and the track there.
    pub(crate) menu_spot: Option<(lumen_core::time::Ticks, Option<Id>)>,
    /// What the info line should say about the headers under the pointer
    /// (M4 slice 0); the app takes it after drawing.
    pub(crate) info: Option<crate::app::info::Info>,
    /// Why an edit was refused (M4 slice 3: groups too deep); the app
    /// shows it as a note.
    pub(crate) note: Option<&'static str>,
    /// The header column's width when dragged from the default (saved
    /// with the project, as panel sizes are).
    width: Option<f32>,
    /// The lanes' height, set by the headers each frame: the pinned area
    /// sits at its bottom.
    view_height: f32,
    /// The pinned area's height when dragged (saved; `None` fits it to its
    /// rows).
    returns_size: Option<f32>,
    /// The returns folded away under their caption (saved).
    pub(crate) returns_folded: bool,
    /// How far the returns are scrolled inside the pinned area.
    returns_top: f32,
    /// The pinned area's height when its divider drag began.
    sizing_returns: Option<f32>,
    /// The return whose sends and header light together (one hovered
    /// last frame), and the one hovered this frame.
    hot_return: Option<Id>,
    hot_return_next: Option<Id>,
    /// Tracks that show sends (audio, MIDI and groups, while the project
    /// has returns): they unfold a step taller.
    with_sends: BTreeSet<Id>,
    /// The rows each return's senders take: it unfolds tall enough for
    /// them.
    senders: BTreeMap<Id, usize>,
    /// The send (track, return) whose knob in a return's list is being set:
    /// it keeps its row while silent until let go (the owner's S1). Taken
    /// from the frame before; `held_next` collects this frame's.
    held_send: Option<(Id, Id)>,
    held_next: Option<(Id, Id)>,
    /// The order each return lists its senders in, loudest first.
    sender_order: BTreeMap<Id, Vec<Id>>,
    /// Tracks cued with their headphone buttons (slice 4): session state.
    pub(crate) cued: BTreeSet<Id>,
    /// The cue's settings, lent by the app while the headers draw.
    pub(crate) cue: CueControls,
    /// The additive-solo preference (M4 slice 4), set by the app.
    pub(crate) additive_solo: bool,
    /// What live input there is and how much has come, lent by the app each
    /// frame for the headers' listening lights (M5 slice 2e).
    pub(crate) live: Live,
    /// The recording under way, if any, lent by the app each frame (slice 6d).
    pub(crate) recording: Option<recording_look::RecordingLook>,
    /// While recording, the project as the next commit will make it, lent
    /// by the app each frame and drawn in place of the document's: the take
    /// and the clips it records over or into change with the playhead, not
    /// once a commit (after MT-MIDI-7).
    pub(crate) preview: Option<std::sync::Arc<lumen_project::model::Project>>,
    /// Every track's meter and the master's loudness (M4 slice 5), moved
    /// on by the app each frame.
    pub(crate) meters: crate::meters::Meters,
    /// Meters in track colours (the owner's M2 setting), set by the app.
    pub(crate) track_colour_meters: bool,
    /// The device's outputs, as the strips' output menus offer them (M4
    /// slice 5), lent by the app.
    pub(crate) outputs: OutputChoices,
    /// How far the mixer's strips are scrolled, and how hard a strip drag
    /// near their edge pushes them (-1 to 1).
    mixer_offset: f32,
    mixer_push: f32,
    /// Where the mixer's scroll bar was dragged to, for the next frame.
    mixer_scroll_to: Option<f32>,
}

/// The device's output pairs, as the strips' output menus offer them (the
/// owner's B6).
#[derive(Debug, Clone, Default)]
pub(crate) struct OutputChoices {
    /// Each pair (0 is outputs 1 and 2) with its channels' names; empty
    /// until they are looked up.
    pub pairs: Vec<(u16, String)>,
    /// The main pair and the cue pair, as device pairs.
    pub main: Option<u16>,
    pub cue: Option<u16>,
    /// Still looking the device's outputs up, or why they are not known.
    pub status: Option<String>,
    /// A menu asked for them: the app looks them up.
    pub wanted: bool,
}

/// The tallest whole-step height at which `count` tracks fit in `visible`
/// points, between folded and the largest height.
fn fit_height(visible: f32, count: f32) -> f32 {
    ((visible / count / STEP).floor() * STEP).clamp(FOLDED, MAX_HEIGHT)
}

pub(crate) fn color_of(c: Color) -> Color32 {
    Color32::from_rgb((c.0 >> 16) as u8, (c.0 >> 8) as u8, c.0 as u8)
}

/// The colour `clip` is drawn in: its own, or its track's, and grey while
/// muted (the owner's M1, slice 6e).
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

impl Arrangement {
    /// The header column's width.
    pub fn header_width(&self) -> f32 {
        self.width.unwrap_or(HEADER_WIDTH)
    }

    /// Sets the header column's width, between the default and the widest.
    pub fn set_header_width(&mut self, width: f32) {
        let width = width.clamp(HEADER_WIDTH, MAX_HEADER_WIDTH).round();
        self.width = (width != HEADER_WIDTH).then_some(width);
    }

    /// The saved view state, written into a project being saved.
    pub fn save_into(&self, view: &mut lumen_project::model::ViewState) {
        view.header_width = self.header_width();
        view.top = self.top;
        view.track_heights = self.heights.clone();
        view.expanded = self.expanded.clone();
        view.collapsed = self.collapsed.clone();
        view.returns_height = self.returns_size;
        view.returns_folded = self.returns_folded;
    }

    /// Restores the view state of an opened project.
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
    fn fitted_heights_are_whole_steps_that_fit() {
        // 10 tracks in 500 points: 50 each, down to 44 (two steps).
        assert_eq!(fit_height(500.0, 10.0), 44.0);
        // 3 tracks in 500: 166, down to 154 (seven steps).
        assert_eq!(fit_height(500.0, 3.0), 154.0);
        // Never above the largest, never below folded.
        assert_eq!(fit_height(5_000.0, 1.0), MAX_HEIGHT);
        assert_eq!(fit_height(100.0, 50.0), FOLDED);
    }

    #[test]
    fn heights_come_in_whole_steps() {
        assert_eq!(snap_height(95.0), 4.0 * STEP);
        assert_eq!(snap_height(100.0), 5.0 * STEP);
        assert_eq!(FOLDED, 2.0 * STEP);
        assert_eq!(UNFOLDED, 4.0 * STEP);
        // An opened project with an odd height (an older build) snaps.
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
        // The pinned area's height and fold (format v10).
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
        // Out of range widths are kept in range.
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
        // A track added with a return selected goes among the tracks.
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
        // Folded away, they are skipped by the arrow keys too.
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
