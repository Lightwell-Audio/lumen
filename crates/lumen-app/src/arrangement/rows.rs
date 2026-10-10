//! Rows and zones: where each track is drawn, heights, scrolling and
//! revealing a track (split from `mod.rs`, M4 review M1).

use std::sync::Arc;

use eframe::egui;
use lumen_project::model::{Id, Project, Track, TrackKind};

use super::{
    Arrangement, FOLD_BELOW, FOLDED, MAX_HEIGHT, Row, RowKind, STEP, UNFOLDED, Zone, fit_height,
    sends, snap_height,
};
use crate::motion;

impl Arrangement {
    /// The height a track is drawn at (before the glide).
    pub(super) fn target_height(&self, id: Id) -> f32 {
        if self.expanded.contains(&id) {
            // A track with sends to show opens a step taller, so the
            // knobs have their own row (slice 4).
            let unfolded = if self.with_sends.contains(&id) {
                UNFOLDED + STEP
            } else if let Some(&rows) = self.senders.get(&id) {
                // A return opens tall enough to list what sends to it
                // (MT-MIX-4 runs 1 and 2, the owner's R2 and L4).
                (UNFOLDED + rows.saturating_sub(1) as f32 * STEP).min(MAX_HEIGHT)
            } else {
                UNFOLDED
            };
            self.heights.get(&id).copied().unwrap_or(unfolded)
        } else {
            FOLDED
        }
    }

    /// Whether track `id` is hidden in a folded group.
    pub(super) fn hidden(&self, project: &Project, id: Id) -> bool {
        !self.collapsed.is_empty()
            && lumen_project::groups::ancestors(&project.tracks, id)
                .any(|g| self.collapsed.contains(&g))
    }

    /// The tracks shown, in order: those not hidden in a folded group,
    /// then the returns unless folded away (they are drawn at the bottom).
    pub(crate) fn visible<'p>(&self, project: &'p Project) -> impl Iterator<Item = &'p Arc<Track>> {
        let returns = |t: &&Arc<Track>| t.kind == TrackKind::Return;
        let tracks = project
            .tracks
            .iter()
            .filter(move |t| !returns(t) && !self.hidden(project, t.id));
        let folded = self.returns_folded;
        tracks.chain(project.tracks.iter().filter(move |t| !folded && returns(t)))
    }

    /// Every row with its position; heights glide to their targets. Tracks
    /// hidden in a folded group have no row. The scrolling tracks come
    /// first, then the pinned returns and the master.
    pub(super) fn rows(&self, ctx: &egui::Context, project: &Project, pending: usize) -> Vec<Row> {
        let mut rows = Vec::with_capacity(project.tracks.len() + pending + 1);
        let mut y = 0.0;
        let now = ctx.input(|i| i.time);
        for (i, track) in project.tracks.iter().enumerate() {
            if track.kind == TrackKind::Return {
                continue;
            }
            // A track hidden in a folded group glides shut, then goes
            // (M4.5, M2); unfolding, it grows back.
            let hidden = self.hidden(project, track.id);
            let target = if hidden {
                0.0
            } else {
                self.target_height(track.id)
            };
            let height = motion::glide(ctx, super::row_glide::height_id(track.id), target, true);
            if hidden && height < 0.5 {
                continue;
            }
            rows.push(Row {
                kind: RowKind::Track(i),
                y: y + self.row_glides.offset(track.id, now),
                height,
                zone: Zone::Main,
                clipped: height + 0.5 < self.target_height(track.id),
            });
            y += height;
        }
        for i in 0..pending {
            rows.push(Row {
                kind: RowKind::Pending(i),
                y,
                height: FOLDED,
                zone: Zone::Main,
                clipped: false,
            });
            y += FOLDED;
        }
        // The pinned area, placed so that `lanes' top + y − scroll` lands
        // in it whatever the scroll.
        let zones = self.zones(ctx, project);
        let mut y = self.top + zones.returns.0 - self.returns_top;
        for (i, track) in project.tracks.iter().enumerate() {
            if track.kind != TrackKind::Return || self.returns_folded {
                continue;
            }
            let height = motion::glide(
                ctx,
                super::row_glide::height_id(track.id),
                self.target_height(track.id),
                true,
            );
            rows.push(Row {
                kind: RowKind::Track(i),
                y,
                height,
                zone: Zone::Returns,
                clipped: false,
            });
            y += height;
        }
        rows.push(Row {
            kind: RowKind::Master,
            y: self.top + zones.master.0,
            height: zones.master.1 - zones.master.0,
            zone: Zone::Master,
            clipped: false,
        });
        rows
    }

    /// The bottom of the scrolling tracks, in content coordinates.
    pub(super) fn main_bottom(rows: &[Row]) -> f32 {
        rows.iter()
            .rfind(|r| r.zone == Zone::Main)
            .map_or(0.0, |r| r.y + r.height)
    }

    /// Height of everything, for scrolling: the scrolling tracks and the
    /// pinned area under them.
    pub(super) fn content_height(
        &self,
        ctx: &egui::Context,
        project: &Project,
        rows: &[Row],
    ) -> f32 {
        Self::main_bottom(rows) + (self.view_height - self.zones(ctx, project).main).max(0.0)
    }

    /// The height left to the scrolling tracks in lanes `height` tall.
    pub(crate) fn main_height(&self, ctx: &egui::Context, project: &Project, height: f32) -> f32 {
        let pinned = (self.view_height - self.zones(ctx, project).main).max(0.0);
        (height - pinned).max(FOLDED)
    }

    /// The zone a point `y` below the lanes' top falls in (`None`: the
    /// divider or the caption).
    pub(super) fn zone_at(&self, ctx: &egui::Context, project: &Project, y: f32) -> Option<Zone> {
        let zones = self.zones(ctx, project);
        if y < zones.main {
            Some(Zone::Main)
        } else if (zones.returns.0..zones.returns.1).contains(&y) {
            Some(Zone::Returns)
        } else if y >= zones.master.0 {
            Some(Zone::Master)
        } else {
            None
        }
    }

    /// The span a zone covers on screen, given the lanes' top and bottom.
    pub(super) fn zone_span(
        &self,
        ctx: &egui::Context,
        project: &Project,
        zone: Zone,
        lanes: egui::Rangef,
    ) -> egui::Rangef {
        let zones = self.zones(ctx, project);
        let at = |y: f32| (lanes.min + y).min(lanes.max);
        match zone {
            Zone::Main => egui::Rangef::new(lanes.min, at(zones.main)),
            Zone::Returns => egui::Rangef::new(at(zones.returns.0), at(zones.returns.1)),
            Zone::Master => egui::Rangef::new(at(zones.master.0), at(zones.master.1)),
        }
    }

    /// Forgets selected, resized or unfolded tracks that no longer exist.
    pub(super) fn prune(&mut self, project: &Project) {
        let exists =
            |id: &Id| *id == project.master.id || project.tracks.iter().any(|t| t.id == *id);
        self.selected.retain(exists);
        self.heights.retain(|id, _| exists(id));
        self.expanded.retain(exists);
        self.collapsed.retain(exists);
        let returns = project.tracks.iter().any(|t| t.kind == TrackKind::Return);
        self.with_sends = project
            .tracks
            .iter()
            .filter(|t| returns && sends::has_sends(t.kind))
            .map(|t| t.id)
            .collect();
        self.senders = project
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Return)
            .map(|t| {
                let count = sends::senders(project, t.id, self.held_send).len();
                (t.id, sends::sender_rows(count))
            })
            .collect();
        self.sender_order.retain(|id, _| exists(id));
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
        let zone = self.zone_at(ctx, project, y)?;
        let y = y + self.top;
        self.rows(ctx, project, pending)
            .iter()
            .filter(|r| r.zone == zone)
            .find(|r| (r.y..r.y + r.height).contains(&y))
            .and_then(|r| match r.kind {
                RowKind::Track(i) => project.tracks.get(i).map(|t| t.id),
                RowKind::Pending(_) | RowKind::Master => None,
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

    /// The scrolling tracks' area within lanes `canvas`, and the height of
    /// what scrolls in it (for the scroll bar, M4.5).
    pub fn scroll_extent(
        &self,
        ctx: &egui::Context,
        project: &Project,
        pending: usize,
        canvas: egui::Rect,
    ) -> (egui::Rect, f32) {
        let main = self.zone_span(ctx, project, Zone::Main, canvas.y_range());
        let area = egui::Rect::from_x_y_ranges(canvas.x_range(), main);
        // The furthest scroll leaves a folded row in view, as the wheel does.
        let content = self.height_of_content(ctx, project, pending) + FOLDED;
        (
            area,
            content.max(canvas.height()) - (canvas.height() - area.height()),
        )
    }

    /// Scrolls to `top` points, within the content.
    pub fn scroll_to(&mut self, top: f32, content: f32, visible: f32) {
        self.top = top.clamp(0.0, max_top(content, visible));
    }

    /// Scrolls vertically by `delta` points, within the content.
    pub fn scroll(&mut self, delta: f32, content: f32, visible: f32) {
        self.top = (self.top - delta).clamp(0.0, max_top(content, visible));
    }

    /// Keeps the scroll within the content: after a project opens, a group
    /// folds or tracks are deleted, it could stay past the content and hide
    /// tracks above an empty view (the interface audit's U1). True when it
    /// moved.
    pub(super) fn keep_in_content(&mut self, content: f32, visible: f32) -> bool {
        let top = self.top.clamp(0.0, max_top(content, visible));
        let moved = top != self.top;
        self.top = top;
        moved
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
        let visible = visible - (self.view_height - self.zones(ctx, project).main).max(0.0);
        if let Some(row) = rows
            .iter()
            .find(|r| r.kind == RowKind::Track(index) && r.zone == Zone::Main)
        {
            if row.y < self.top {
                self.top = row.y;
            } else if row.y + row.height > self.top + visible {
                self.top = (row.y + row.height - visible).max(0.0);
            }
        }
    }

    /// Height of everything, for scrolling.
    pub fn height_of_content(&self, ctx: &egui::Context, project: &Project, pending: usize) -> f32 {
        self.content_height(ctx, project, &self.rows(ctx, project, pending))
    }
}

/// The furthest the tracks scroll: the last one stays a folded row in view.
fn max_top(content: f32, visible: f32) -> f32 {
    (content - visible + FOLDED).max(0.0)
}
