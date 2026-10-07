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

mod clips;
mod headers;

pub use clips::TimeView;

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{self, Color32, Id as EguiId, Key, Modifiers};
use lumen_project::model::{Color, Id, Project, TrackKind};

use crate::document::Document;
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
const TITLE: f32 = 15.0;

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
}

fn color_of(c: Color) -> Color32 {
    Color32::from_rgb((c.0 >> 16) as u8, (c.0 >> 8) as u8, c.0 as u8)
}

fn color_from(c: Color32) -> Color {
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

    /// Every row with its position; heights glide to their targets.
    fn rows(&self, ctx: &egui::Context, project: &Project, pending: usize) -> Vec<Row> {
        let mut rows = Vec::with_capacity(project.tracks.len() + pending);
        let mut y = 0.0;
        for (i, track) in project.tracks.iter().enumerate() {
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
        if self.renaming.as_ref().is_some_and(|(id, _)| !exists(id)) {
            self.renaming = None;
        }
    }

    /// Scrolls vertically by `delta` points, within the content.
    pub fn scroll(&mut self, delta: f32, content: f32, visible: f32) {
        self.top = (self.top - delta).clamp(0.0, (content - visible + FOLDED).max(0.0));
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
            let index = |id: Id| project.tracks.iter().position(|t| t.id == id);
            if let (Some(a), Some(b)) = (index(anchor), index(id)) {
                let (lo, hi) = (a.min(b), a.max(b));
                self.selected = project.tracks[lo..=hi].iter().map(|t| t.id).collect();
            }
        } else {
            self.selected = vec![id];
            self.anchor = Some(id);
        }
    }

    /// Moves the selection one track up (`-1`) or down (`1`) from the last
    /// selected track; with `extend`, adds that track instead (Shift).
    /// With nothing selected, starts at the first or the last track.
    fn step_selection(&mut self, tracks: &[Id], by: isize, extend: bool) {
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

    /// Moves the selected tracks one place up (`-1`) or down (`1`).
    fn nudge(&mut self, doc: &mut Document, by: isize) {
        let tracks = &doc.project().tracks;
        let positions: Vec<usize> = tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| self.selected.contains(&t.id))
            .map(|(i, _)| i)
            .collect();
        let (Some(&first), Some(&last)) = (positions.first(), positions.last()) else {
            return;
        };
        let target = if by < 0 {
            first.checked_sub(1)
        } else {
            (last + 1 < tracks.len()).then_some(first + 1)
        };
        if let Some(index) = target {
            let ids: Vec<Id> = positions.iter().map(|&i| tracks[i].id).collect();
            doc.move_tracks(&ids, index);
        }
    }

    /// Track shortcuts (not while typing in a field): Ctrl+T adds an audio
    /// track and Ctrl+Shift+T a MIDI track, after the selection; Ctrl+D
    /// duplicates the selected tracks; Delete removes them; F2 renames;
    /// Alt+Up and Alt+Down move them.
    pub fn shortcuts(&mut self, ctx: &egui::Context, doc: &mut Document) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |modifiers, key| {
            ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(modifiers, key)))
        };
        // Keyboard-only selection and folding (audit H2), unless a control
        // has focus and uses the arrow keys itself (a slider).
        if ctx.memory(|m| m.focused().is_none()) {
            let tracks: Vec<Id> = doc.project().tracks.iter().map(|t| t.id).collect();
            for (shift, key, by) in [
                (false, Key::ArrowUp, -1),
                (false, Key::ArrowDown, 1),
                (true, Key::ArrowUp, -1),
                (true, Key::ArrowDown, 1),
            ] {
                let modifiers = if shift {
                    Modifiers::SHIFT
                } else {
                    Modifiers::NONE
                };
                if pressed(modifiers, key) {
                    self.step_selection(&tracks, by, shift);
                }
            }
            if pressed(Modifiers::NONE, Key::ArrowLeft) {
                for id in &self.selected {
                    self.expanded.remove(id);
                }
            }
            if pressed(Modifiers::NONE, Key::ArrowRight) {
                self.expanded.extend(self.selected.iter().copied());
            }
        }
        let after = self.selected.last().copied();
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::T) {
            self.selected = vec![doc.add_track(TrackKind::Midi, after)];
        } else if pressed(Modifiers::COMMAND, Key::T) {
            self.selected = vec![doc.add_track(TrackKind::Audio, after)];
        } else if pressed(Modifiers::COMMAND, Key::D) && !self.selected.is_empty() {
            self.selected = doc.duplicate_tracks(&self.selected);
        } else if (pressed(Modifiers::NONE, Key::Delete)
            || pressed(Modifiers::NONE, Key::Backspace))
            && !self.selected.is_empty()
        {
            doc.delete_tracks(&self.selected);
            self.selected.clear();
        } else if pressed(Modifiers::ALT, Key::ArrowUp) {
            self.nudge(doc, -1);
        } else if pressed(Modifiers::ALT, Key::ArrowDown) {
            self.nudge(doc, 1);
        } else if pressed(Modifiers::NONE, Key::F2)
            && let Some(id) = self.selected.last().copied()
            && let Some(track) = doc.project().tracks.iter().find(|t| t.id == id)
        {
            self.renaming = Some((id, track.name.clone()));
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
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::headers::track_description;
    use super::*;

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
        a.nudge(&mut doc, -1);
        assert_eq!(order(&doc), [ids[0], ids[2], ids[1]]);
        a.nudge(&mut doc, -1);
        a.nudge(&mut doc, -1);
        assert_eq!(order(&doc), [ids[2], ids[0], ids[1]], "stops at the top");
        a.nudge(&mut doc, 1);
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
        a.step_selection(&tracks, 1, false);
        assert_eq!(
            a.selected,
            [Id(1)],
            "Down with nothing selected starts at the top"
        );
        a.step_selection(&tracks, 1, false);
        assert_eq!(a.selected, [Id(2)]);
        a.step_selection(&tracks, 1, true);
        assert_eq!(a.selected, [Id(2), Id(3)], "Shift extends");
        a.step_selection(&tracks, 1, false);
        assert_eq!(a.selected, [Id(3)], "stops at the bottom");
        let mut b = Arrangement::default();
        b.step_selection(&tracks, -1, false);
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
