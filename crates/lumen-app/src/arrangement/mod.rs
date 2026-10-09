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
//! Tracks fold and unfold, as in Ableton. Folded is a compact row; unfolded
//! is taller, with volume and pan in the header (automation lanes join it in
//! M7). Dragging a track's bottom edge resizes it: a folded track dragged
//! taller unfolds, and an unfolded one dragged small folds. Heights glide
//! (motion: GLIDE) and are saved with the project, but are not undo steps.
//!
//! Groups (M4 slice 3, the owner's H1): a group's tracks follow it, stepped
//! in by [`INDENT`] a level, with the colour rail of every group they are
//! in running down beside them. A group's arrow hides or shows its tracks
//! (`collapsed`, saved with the project); its height is still dragged.

mod clips;
mod drop;
pub mod handles;
mod headers;
mod menus;
mod nesting;
mod reorder;
mod samples;
pub mod select;
mod waves;

pub use drop::FileDrop;
pub use samples::Update as StripUpdate;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use eframe::egui::{self, Color32, Id as EguiId, Modifiers};
use lumen_project::model::{Clip, Color, Id, Project, Track, TrackKind};

use crate::document::{Document, Place};
use crate::motion;

/// Width of the header column.
pub const HEADER_WIDTH: f32 = 204.0;

/// Track heights come in steps of this (owner request): elements drawn by
/// height later (automation lanes, meters) can rely on whole steps.
pub const STEP: f32 = 22.0;

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

/// Indents stop deepening past this many levels, so names keep room.
const MAX_INDENT_LEVELS: usize = 4;

/// The indent of a track `depth` groups deep.
pub(crate) fn indent(depth: usize) -> f32 {
    INDENT * depth.min(MAX_INDENT_LEVELS) as f32
}

const BUTTON: f32 = 19.0;

/// One row of the arrangement, in content coordinates.
#[derive(Debug, Clone, Copy)]
struct Row {
    kind: RowKind,
    y: f32,
    height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RowKind {
    /// Index into the project's tracks.
    Track(usize),
    /// Index into the files loading for import.
    Pending(usize),
}

/// The arrangement's view state: vertical scroll, heights, folding,
/// selection and the track being renamed.
#[derive(Debug, Default)]
pub struct Arrangement {
    /// Vertical scroll, in points.
    pub top: f32,
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
    /// Selected clips (slice 6a). Selecting clips clears the track
    /// selection, and the other way round.
    pub clips: BTreeSet<Id>,
    /// The clip drag or marquee in progress.
    pub gesture: Option<select::Gesture>,
    /// The clip under the pointer, its half and the side it would trim,
    /// for the H1 hover and the L3 handles.
    pub hover: Option<select::Hover>,
    /// The marquee being drawn, on screen.
    pub marquee: Option<egui::Rect>,
    /// The selected time range (selecting one clears the clip selection,
    /// and the other way round).
    pub range: Option<select::TimeRange>,
    /// Import audio was chosen in a track's menu: the app runs it next
    /// frame, onto that track (now the selection).
    pub(crate) import_requested: bool,
    /// The clip the arrow keys last chose (M3 review H2): where the next
    /// arrow goes from while it is selected.
    pub(crate) clip_cursor: Option<Id>,
    /// What the info line should say about the headers under the pointer
    /// (M4 slice 0); the app takes it after drawing.
    pub(crate) info: Option<crate::app::info::Info>,
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
    /// The height a track is drawn at (before the glide).
    fn target_height(&self, id: Id) -> f32 {
        if self.expanded.contains(&id) {
            self.heights.get(&id).copied().unwrap_or(UNFOLDED)
        } else {
            FOLDED
        }
    }

    /// Whether track `id` is hidden in a folded group.
    fn hidden(&self, project: &Project, id: Id) -> bool {
        !self.collapsed.is_empty()
            && lumen_project::groups::ancestors(&project.tracks, id)
                .any(|g| self.collapsed.contains(&g))
    }

    /// The tracks shown, in order: those not hidden in a folded group.
    pub(crate) fn visible<'p>(&self, project: &'p Project) -> impl Iterator<Item = &'p Arc<Track>> {
        project
            .tracks
            .iter()
            .filter(move |t| !self.hidden(project, t.id))
    }

    /// Every row with its position; heights glide to their targets. Tracks
    /// hidden in a folded group have no row.
    fn rows(&self, ctx: &egui::Context, project: &Project, pending: usize) -> Vec<Row> {
        let mut rows = Vec::with_capacity(project.tracks.len() + pending);
        let mut y = 0.0;
        for (i, track) in project.tracks.iter().enumerate() {
            if self.hidden(project, track.id) {
                continue;
            }
            let height = motion::glide(
                ctx,
                EguiId::new(("track height", track.id)),
                self.target_height(track.id),
                true,
            );
            rows.push(Row {
                kind: RowKind::Track(i),
                y,
                height,
            });
            y += height;
        }
        for i in 0..pending {
            rows.push(Row {
                kind: RowKind::Pending(i),
                y,
                height: FOLDED,
            });
            y += FOLDED;
        }
        rows
    }

    fn content_height(rows: &[Row]) -> f32 {
        rows.last().map_or(0.0, |r| r.y + r.height)
    }

    /// Forgets selected, resized or unfolded tracks that no longer exist.
    fn prune(&mut self, project: &Project) {
        let exists = |id: &Id| project.tracks.iter().any(|t| t.id == *id);
        self.selected.retain(exists);
        self.heights.retain(|id, _| exists(id));
        self.expanded.retain(exists);
        self.collapsed.retain(exists);
        if self.renaming.as_ref().is_some_and(|(id, _)| !exists(id)) {
            self.renaming = None;
        }
    }

    /// The track whose row is `y` points below the lanes' top edge.
    pub fn track_at(
        &self,
        ctx: &egui::Context,
        project: &Project,
        pending: usize,
        y: f32,
    ) -> Option<Id> {
        let y = y + self.top;
        self.rows(ctx, project, pending)
            .iter()
            .find(|r| (r.y..r.y + r.height).contains(&y))
            .and_then(|r| match r.kind {
                RowKind::Track(i) => project.tracks.get(i).map(|t| t.id),
                RowKind::Pending(_) => None,
            })
    }

    /// `steps` height steps taller (or shorter, if negative), as dragging
    /// the edge does: below the unfolded height the track folds (Alt+wheel,
    /// slice 4d).
    pub fn step_height(&mut self, id: Id, steps: i32) {
        let from = self.target_height(id);
        let wanted = snap_height((from + steps as f32 * STEP).clamp(FOLDED, MAX_HEIGHT));
        if wanted < FOLD_BELOW {
            self.expanded.remove(&id);
        } else {
            self.expanded.insert(id);
            self.heights.insert(id, wanted);
        }
        self.fitted = None;
    }

    /// H: every track as tall as fits in `visible` points, in whole steps
    /// (folded if even the unfolded height does not fit); H again puts
    /// heights, folding and scroll back as they were.
    pub fn toggle_fit_heights(&mut self, project: &Project, visible: f32) {
        if let Some((heights, expanded, top)) = self.fitted.take() {
            self.heights = heights;
            self.expanded = expanded;
            self.top = top;
            return;
        }
        self.fitted = Some((self.heights.clone(), self.expanded.clone(), self.top));
        let shown: Vec<Id> = self.visible(project).map(|t| t.id).collect();
        let count = shown.len().max(1) as f32;
        let each = fit_height(visible, count);
        for track in project.tracks.iter().filter(|t| shown.contains(&t.id)) {
            if each >= UNFOLDED {
                self.expanded.insert(track.id);
                self.heights.insert(track.id, each);
            } else {
                self.expanded.remove(&track.id);
            }
        }
        self.top = 0.0;
    }

    /// Scrolls vertically by `delta` points, within the content.
    pub fn scroll(&mut self, delta: f32, content: f32, visible: f32) {
        self.top = (self.top - delta).clamp(0.0, (content - visible + FOLDED).max(0.0));
    }

    /// Selects track `id` alone and scrolls just enough to show it in the
    /// `visible` height (the palette's "Go to track").
    pub fn reveal(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        pending: usize,
        id: Id,
        visible: f32,
    ) {
        if project.tracks.iter().any(|t| t.id == id) {
            self.selected = vec![id];
            self.anchor = Some(id);
            for group in lumen_project::groups::ancestors(&project.tracks, id) {
                self.collapsed.remove(&group);
            }
            self.scroll_to_track(ctx, project, pending, id, visible);
        }
    }

    /// Scrolls just enough to show track `id` in the `visible` height.
    pub fn scroll_to_track(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        pending: usize,
        id: Id,
        visible: f32,
    ) {
        let Some(index) = project.tracks.iter().position(|t| t.id == id) else {
            return;
        };
        let rows = self.rows(ctx, project, pending);
        if let Some(row) = rows.iter().find(|r| r.kind == RowKind::Track(index)) {
            if row.y < self.top {
                self.top = row.y;
            } else if row.y + row.height > self.top + visible {
                self.top = (row.y + row.height - visible).max(0.0);
            }
        }
    }

    /// Height of everything, for scrolling.
    pub fn height_of_content(&self, ctx: &egui::Context, project: &Project, pending: usize) -> f32 {
        Self::content_height(&self.rows(ctx, project, pending))
    }

    /// Selects `id` the way a click does: alone, toggled with Ctrl, or as a
    /// range from the last click with Shift.
    fn click_select(&mut self, project: &Project, id: Id, modifiers: Modifiers) {
        if modifiers.command {
            if let Some(i) = self.selected.iter().position(|s| *s == id) {
                self.selected.remove(i);
            } else {
                self.selected.push(id);
            }
            self.anchor = Some(id);
        } else if modifiers.shift
            && let Some(anchor) = self.anchor
        {
            let shown: Vec<Id> = self.visible(project).map(|t| t.id).collect();
            let index = |id: Id| shown.iter().position(|t| *t == id);
            if let (Some(a), Some(b)) = (index(anchor), index(id)) {
                let (lo, hi) = (a.min(b), a.max(b));
                self.selected = shown[lo..=hi].to_vec();
            }
        } else {
            self.selected = vec![id];
            self.anchor = Some(id);
        }
    }

    /// Moves the selection one track up (`-1`) or down (`1`) from the last
    /// selected track; with `extend`, adds that track instead (Shift).
    /// With nothing selected, starts at the first or the last track.
    fn step_through(&mut self, tracks: &[Id], by: isize, extend: bool) {
        if tracks.is_empty() {
            return;
        }
        let current = self
            .selected
            .last()
            .and_then(|id| tracks.iter().position(|t| t == id));
        let next = match current {
            None if by < 0 => tracks.len() - 1,
            None => 0,
            Some(i) => i.saturating_add_signed(by).min(tracks.len() - 1),
        };
        let id = tracks[next];
        if extend {
            self.selected.retain(|s| *s != id);
            self.selected.push(id);
        } else {
            self.selected = vec![id];
            self.anchor = Some(id);
        }
    }

    /// The tracks a header button acts on: the whole selection when the
    /// clicked track is part of it, otherwise just that track.
    fn targets(&self, id: Id) -> Vec<Id> {
        if self.selected.len() > 1 && self.selected.contains(&id) {
            self.selected.clone()
        } else {
            vec![id]
        }
    }

    /// Moves the selected tracks one place up (`-1`) or down (`1`) among
    /// the tracks of their group, over a neighbouring group whole; they
    /// stay in their group (dragging or Ctrl+Shift+G takes them out).
    pub(crate) fn move_selected(&mut self, doc: &mut Document, by: isize) {
        let tracks = &doc.project().tracks;
        let ids = lumen_project::groups::outermost(tracks, &self.selected);
        let Some(first) = ids
            .first()
            .and_then(|id| tracks.iter().find(|t| t.id == *id))
        else {
            return;
        };
        let group = first.parent;
        let moving = lumen_project::groups::with_contents(tracks, &ids);
        // The other tracks of the same group, in order.
        let siblings: Vec<Id> = tracks
            .iter()
            .filter(|t| t.parent == group && !moving.contains(&t.id))
            .map(|t| t.id)
            .collect();
        let position = |id: Id| tracks.iter().position(|t| t.id == id).unwrap_or(0);
        let first_at = position(first.id);
        // Siblings above the moving tracks, before the move.
        let above = siblings.iter().filter(|&&s| position(s) < first_at).count();
        let wanted = above
            .checked_add_signed(by)
            .filter(|&n| n <= siblings.len());
        let Some(wanted) = wanted else {
            return;
        };
        if wanted == above {
            return;
        }
        // Before the sibling now at `wanted`, or after the group's last.
        let rest: Vec<Id> = tracks
            .iter()
            .filter(|t| !moving.contains(&t.id))
            .map(|t| t.id)
            .collect();
        let index = match siblings.get(wanted) {
            Some(&before) => rest.iter().position(|&r| r == before).unwrap_or(rest.len()),
            None => {
                let last = siblings[wanted - 1];
                let end = lumen_project::groups::span(tracks, position(last)).end;
                rest.iter()
                    .position(|&r| position(r) >= end)
                    .unwrap_or(rest.len())
            }
        };
        let _ = doc.move_tracks(&ids, Place { index, group });
    }

    /// Adds a track of `kind` after the selection and selects it.
    pub(crate) fn add_track(&mut self, doc: &mut Document, kind: TrackKind) {
        let after = self.selected.last().copied();
        self.selected = vec![doc.add_track(kind, after)];
    }

    /// Ctrl+G: groups the selected tracks and selects the new group.
    pub(crate) fn group_selected(&mut self, doc: &mut Document) {
        if let Some(group) = doc.group_tracks(&self.selected) {
            self.selected = vec![group];
            self.anchor = Some(group);
        }
    }

    /// Ctrl+Shift+G: removes the selected groups (their tracks become the
    /// selection), or takes the selected tracks out of their groups.
    pub(crate) fn ungroup_selected(&mut self, doc: &mut Document) {
        let project = doc.project();
        let inside: Vec<Id> = project
            .tracks
            .iter()
            .filter(|t| t.parent.is_some_and(|p| self.selected.contains(&p)))
            .filter(|t| {
                project
                    .tracks
                    .iter()
                    .any(|g| Some(g.id) == t.parent && g.kind == TrackKind::Group)
            })
            .map(|t| t.id)
            .collect();
        if doc.ungroup(&self.selected).is_ok() && !inside.is_empty() {
            self.selected = inside;
        }
    }

    /// Duplicates the selected tracks and selects the copies.
    pub(crate) fn duplicate_selected(&mut self, doc: &mut Document) {
        if !self.selected.is_empty() {
            self.selected = doc.duplicate_tracks(&self.selected);
        }
    }

    /// Deletes the selected tracks.
    pub(crate) fn delete_selected(&mut self, doc: &mut Document) {
        if !self.selected.is_empty() {
            doc.delete_tracks(&self.selected);
            self.selected.clear();
        }
    }

    /// Starts renaming the last selected track in its header.
    pub(crate) fn rename_selected(&mut self, doc: &Document) {
        if let Some(id) = self.selected.last().copied()
            && let Some(track) = doc.project().tracks.iter().find(|t| t.id == id)
        {
            self.renaming = Some((id, track.name.clone()));
        }
    }

    /// Selects the track `by` places from the last selected one (or the
    /// first or last track), or adds it to the selection when `extend`.
    pub(crate) fn step_selection(&mut self, doc: &Document, by: isize, extend: bool) {
        let tracks: Vec<Id> = self.visible(doc.project()).map(|t| t.id).collect();
        self.step_through(&tracks, by, extend);
    }

    /// Folds (`fold`) or unfolds the selected tracks; selected groups
    /// hide or show their tracks instead (as their arrow does).
    pub(crate) fn fold_selected(&mut self, project: &Project, fold: bool) {
        for &id in &self.selected {
            let group = project
                .tracks
                .iter()
                .any(|t| t.id == id && t.kind == TrackKind::Group);
            match (group, fold) {
                (true, true) => self.collapsed.insert(id),
                (true, false) => self.collapsed.remove(&id),
                (false, true) => self.expanded.remove(&id),
                (false, false) => self.expanded.insert(id),
            };
        }
        // Selected tracks now hidden leave the selection.
        self.forget_hidden(project);
    }

    /// Takes tracks hidden in folded groups out of the selection.
    fn forget_hidden(&mut self, project: &Project) {
        let selected = std::mem::take(&mut self.selected);
        self.selected = selected
            .into_iter()
            .filter(|&id| !self.hidden(project, id))
            .collect();
    }

    /// Hides or shows group `id`'s tracks (its arrow).
    pub(crate) fn toggle_collapsed(&mut self, project: &Project, id: Id) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
            self.selected
                .retain(|&s| !lumen_project::groups::is_inside(&project.tracks, s, id));
        }
    }

    /// Folds every group (`fold`) or unfolds them all (Alt+click on a
    /// group's arrow, and the palette).
    pub(crate) fn collapse_all(&mut self, project: &Project, fold: bool) {
        if fold {
            let groups = project.tracks.iter().filter(|t| t.kind == TrackKind::Group);
            self.collapsed = groups.map(|t| t.id).collect();
            self.forget_hidden(project);
        } else {
            self.collapsed.clear();
        }
    }

    pub fn toggle_fold(&mut self, id: Id) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
    }

    /// The saved view state, written into a project being saved.
    pub fn save_into(&self, view: &mut lumen_project::model::ViewState) {
        view.top = self.top;
        view.track_heights = self.heights.clone();
        view.expanded = self.expanded.clone();
        view.collapsed = self.collapsed.clone();
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
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::headers::track_description;
    use super::*;

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
        let mut view = lumen_project::model::ViewState::default();
        a.save_into(&mut view);
        let b = Arrangement::restore(&view);
        assert_eq!(
            (b.top, b.expanded, b.heights),
            (a.top, a.expanded, a.heights)
        );
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
