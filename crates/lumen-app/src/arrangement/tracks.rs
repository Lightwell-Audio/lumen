use std::sync::Arc;

use eframe::egui::Modifiers;
use lumen_project::model::{Id, Project, Track, TrackKind};

use super::Arrangement;
use crate::document::{Document, Place};

impl Arrangement {
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
        if self.selected.contains(&id) {
            self.header_clicked = Some(id);
        }
    }

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

    pub(super) fn targets(&self, id: Id) -> Vec<Id> {
        if self.selected.len() > 1 && self.selected.contains(&id) {
            self.selected.clone()
        } else {
            vec![id]
        }
    }

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

    pub(crate) fn move_selected(&mut self, doc: &mut Document, by: isize) {
        if let Some((ids, place)) = self.planned_move(&doc.project().tracks, by) {
            let _ = doc.move_tracks(&ids, place);
        }
    }

    pub(crate) fn can_move_selected(&self, doc: &Document, by: isize) -> bool {
        self.planned_move(&doc.project().tracks, by).is_some()
    }

    fn planned_move(&self, tracks: &[Arc<Track>], by: isize) -> Option<(Vec<Id>, Place)> {
        let pinned = |id: &Id| {
            tracks
                .iter()
                .any(|t| t.id == *id && t.kind == TrackKind::Return)
        };
        let mut ids = lumen_project::groups::outermost(tracks, &self.selected);
        ids.retain(|id| !pinned(id));
        let first = ids
            .first()
            .and_then(|id| tracks.iter().find(|t| t.id == *id))?;
        let group = first.parent;
        let moving = lumen_project::groups::with_contents(tracks, &ids);
        let siblings: Vec<Id> = tracks
            .iter()
            .filter(|t| t.parent == group && !moving.contains(&t.id) && !pinned(&t.id))
            .map(|t| t.id)
            .collect();
        let position = |id: Id| tracks.iter().position(|t| t.id == id).unwrap_or(0);
        let first_at = position(first.id);
        let above = siblings.iter().filter(|&&s| position(s) < first_at).count();
        let wanted = above
            .checked_add_signed(by)
            .filter(|&n| n <= siblings.len() && n != above)?;
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

    pub(crate) fn add_track(&mut self, doc: &mut Document, kind: TrackKind) {
        let after = self.selected.last().copied().filter(|id| {
            doc.project()
                .tracks
                .iter()
                .any(|t| t.id == *id && t.kind != TrackKind::Return)
        });
        self.add_track_after(doc, kind, after);
    }

    pub(super) fn add_track_after(
        &mut self,
        doc: &mut Document,
        kind: TrackKind,
        after: Option<Id>,
    ) {
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
            after
        };
        if kind == TrackKind::Return {
            self.returns_folded = false;
        }
        self.selected = vec![doc.add_track(kind, after)];
    }

    pub(crate) fn group_selected(&mut self, doc: &mut Document) {
        match doc.group_tracks(&self.selected) {
            Ok(group) => {
                self.selected = vec![group];
                self.anchor = Some(group);
            }
            Err(refused) => self.note = Some(refused.reason()),
        }
    }

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

    pub(crate) fn delete_selected(&mut self, doc: &mut Document) {
        if !self.selected.is_empty() {
            doc.delete_tracks(&self.selected);
            self.selected.clear();
        }
    }

    pub(crate) fn rename_selected(&mut self, doc: &Document) {
        if let Some(id) = self.selected.last().copied()
            && let Some(track) = doc.project().tracks.iter().find(|t| t.id == id)
        {
            self.renaming = Some((id, track.name.clone()));
            self.renaming_in_strip = false;
        }
    }

    pub(crate) fn step_selection(&mut self, doc: &Document, by: isize, extend: bool) {
        let tracks: Vec<Id> = self.visible(doc.project()).map(|t| t.id).collect();
        self.step_through(&tracks, by, extend);
    }

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
        self.forget_hidden(project);
    }

    pub(super) fn forget_hidden(&mut self, project: &Project) {
        let selected = std::mem::take(&mut self.selected);
        self.selected = selected
            .into_iter()
            .filter(|&id| !self.hidden(project, id))
            .collect();
    }

    pub(crate) fn toggle_collapsed(&mut self, project: &Project, id: Id) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
            self.selected
                .retain(|&s| !lumen_project::groups::is_inside(&project.tracks, s, id));
        }
    }

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
