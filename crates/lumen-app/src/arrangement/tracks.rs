//! Track selection and the commands on selected tracks: clicks, the
//! arrow keys, moving, adding, grouping, duplicating, deleting, renaming
//! and folding (split from `mod.rs`, M4 review M1).

use std::sync::Arc;

use eframe::egui::Modifiers;
use lumen_project::model::{Id, Project, Track, TrackKind};

use super::Arrangement;
use crate::document::{Document, Place};

impl Arrangement {
    /// Selects `id` the way a click does: alone, toggled with Ctrl, or as a
    /// range from the last click with Shift.
    pub(super) fn click_select(&mut self, project: &Project, id: Id, modifiers: Modifiers) {
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
    pub(super) fn step_through(&mut self, tracks: &[Id], by: isize, extend: bool) {
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
    pub(super) fn targets(&self, id: Id) -> Vec<Id> {
        if self.selected.len() > 1 && self.selected.contains(&id) {
            self.selected.clone()
        } else {
            vec![id]
        }
    }

    /// A press on a record arm (`was_on`: the arm showed lit). Arming
    /// records on the pressed tracks alone and disarms the rest; Ctrl (Cmd on
    /// a Mac) keeps the others armed; a lit arm only disarms its own tracks.
    pub(super) fn press_arm(
        &self,
        ui: &eframe::egui::Ui,
        doc: &mut Document,
        id: Id,
        was_on: bool,
    ) {
        let targets = self.targets(id);
        if was_on {
            doc.set_armed(&targets, false);
        } else if ui.input(|i| i.modifiers.command) {
            doc.set_armed(&targets, true);
        } else {
            doc.arm_only(&targets);
        }
    }

    /// Moves the selected tracks one place up (`-1`) or down (`1`) among
    /// the tracks of their group, over a neighbouring group whole; they
    /// stay in their group (dragging or Ctrl+Shift+G takes them out).
    pub(crate) fn move_selected(&mut self, doc: &mut Document, by: isize) {
        if let Some((ids, place)) = self.planned_move(&doc.project().tracks, by) {
            let _ = doc.move_tracks(&ids, place);
        }
    }

    /// Whether [`Self::move_selected`] would move anything: a menu leaves
    /// the row out when not (the interface audit's M3).
    pub(crate) fn can_move_selected(&self, doc: &Document, by: isize) -> bool {
        self.planned_move(&doc.project().tracks, by).is_some()
    }

    /// The tracks a move takes and where they go, or `None` when they are
    /// already at that end of their group.
    fn planned_move(&self, tracks: &[Arc<Track>], by: isize) -> Option<(Vec<Id>, Place)> {
        let ids = lumen_project::groups::outermost(tracks, &self.selected);
        let first = ids
            .first()
            .and_then(|id| tracks.iter().find(|t| t.id == *id))?;
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
            .filter(|&n| n <= siblings.len() && n != above)?;
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
        Some((ids, Place { index, group }))
    }

    /// Adds a track of `kind` after the selection and selects it. A
    /// return goes after the last return (they are drawn pinned at the
    /// bottom) and shows the returns if they were folded away.
    pub(crate) fn add_track(&mut self, doc: &mut Document, kind: TrackKind) {
        if kind == TrackKind::Return && doc.returns_full() {
            self.note = Some(crate::document::Refused::TooManyReturns.reason());
            return;
        }
        let after = if kind == TrackKind::Return {
            let tracks = &doc.project().tracks;
            tracks
                .iter()
                .rev()
                .find(|t| t.kind == TrackKind::Return)
                .or(tracks.last())
                .map(|t| t.id)
        } else {
            self.selected.last().copied().filter(|id| {
                doc.project()
                    .tracks
                    .iter()
                    .any(|t| t.id == *id && t.kind != TrackKind::Return)
            })
        };
        if kind == TrackKind::Return {
            self.returns_folded = false;
        }
        self.selected = vec![doc.add_track(kind, after)];
    }

    /// Ctrl+G: groups the selected tracks and selects the new group.
    /// Refused (too deep, or nothing to group), it says why in a note.
    pub(crate) fn group_selected(&mut self, doc: &mut Document) {
        match doc.group_tracks(&self.selected) {
            Ok(group) => {
                self.selected = vec![group];
                self.anchor = Some(group);
            }
            Err(refused) => self.note = Some(refused.reason()),
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
        if self.selected.is_empty() {
            return;
        }
        let copies = doc.duplicate_tracks(&self.selected);
        let returns = |ids: &[Id]| {
            let tracks = &doc.project().tracks;
            ids.iter()
                .filter(|id| {
                    tracks
                        .iter()
                        .any(|t| t.id == **id && t.kind == TrackKind::Return)
                })
                .count()
        };
        if copies.len() < self.selected.len() && returns(&self.selected) > 0 {
            self.note = Some(crate::document::Refused::TooManyReturns.reason());
        }
        if !copies.is_empty() {
            self.selected = copies;
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
            self.renaming_in_strip = false;
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
    pub(super) fn forget_hidden(&mut self, project: &Project) {
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
}
