//! Rows that glide (the interface audit's M2 and M3): a new track grows in
//! from no height, and when tracks are reordered or deleted the rows below
//! glide to their new places instead of jumping. A folding group's tracks
//! glide shut in `rows` (their heights glide to none before they go).

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Id as EguiId};
use lumen_project::model::{Id, Project, TrackKind};

use super::{Arrangement, Zone};
use crate::motion;

/// The memory a track's shown height glides in.
pub(super) fn height_id(track: Id) -> EguiId {
    EguiId::new(("track height", track))
}

#[derive(Debug, Clone, Copy)]
struct Glide {
    /// How far from its place the row is drawn as the glide starts.
    from: f32,
    since: f64,
}

#[derive(Debug, Default)]
pub(super) struct RowGlides {
    session: Option<u64>,
    /// Tracks seen in this document, so a new one can grow in.
    known: HashSet<Id>,
    /// The scrolling tracks' order last frame.
    order: Vec<Id>,
    /// Where each scrolling row was drawn last frame.
    drawn: HashMap<Id, f32>,
    gliding: HashMap<Id, Glide>,
}

impl RowGlides {
    /// Points from its place row `id` is drawn at `now`.
    pub(super) fn offset(&self, id: Id, now: f64) -> f32 {
        let duration = f64::from(motion::time(motion::GLIDE));
        self.gliding.get(&id).map_or(0.0, |g| {
            if duration <= 0.0 {
                return 0.0;
            }
            let t = ((now - g.since) / duration) as f32;
            g.from * (1.0 - motion::spring(t))
        })
    }

    pub(super) fn moving(&self) -> bool {
        !self.gliding.is_empty()
    }
}

impl Arrangement {
    /// Notes new, moved and deleted tracks once a frame, before rows are
    /// laid out: new tracks start from no height, and rows whose place
    /// changed glide there from where they were drawn.
    pub fn begin_frame(
        &mut self,
        ctx: &egui::Context,
        session: u64,
        project: &Project,
        pending: usize,
    ) {
        let now = ctx.input(|i| i.time);
        let duration = f64::from(motion::time(motion::GLIDE));
        self.row_glides
            .gliding
            .retain(|_, g| now - g.since < duration);
        let order: Vec<Id> = project
            .tracks
            .iter()
            .filter(|t| t.kind != TrackKind::Return)
            .map(|t| t.id)
            .collect();
        if self.row_glides.session != Some(session) {
            let glides = &mut self.row_glides;
            glides.session = Some(session);
            glides.known = project.tracks.iter().map(|t| t.id).collect();
            glides.gliding.clear();
            glides.order = order;
            self.note_drawn(ctx, project, pending);
            return;
        }
        if duration > 0.0 {
            for track in &project.tracks {
                if self.row_glides.known.insert(track.id) {
                    // A new track grows in from no height.
                    motion::glide(ctx, height_id(track.id), 0.0, false);
                }
            }
        }
        self.row_glides
            .known
            .retain(|id| project.tracks.iter().any(|t| t.id == *id));
        if order != self.row_glides.order && duration > 0.0 {
            for row in self.rows(ctx, project, pending) {
                let super::RowKind::Track(index) = row.kind else {
                    continue;
                };
                if row.zone != Zone::Main {
                    continue;
                }
                let id = project.tracks[index].id;
                let raw = row.y - self.row_glides.offset(id, now);
                if let Some(&was) = self.row_glides.drawn.get(&id)
                    && (was - raw).abs() > 0.5
                {
                    self.row_glides.gliding.insert(
                        id,
                        Glide {
                            from: was - raw,
                            since: now,
                        },
                    );
                }
            }
        }
        self.row_glides.order = order;
        if self.row_glides.moving() {
            ctx.request_repaint();
        }
        self.note_drawn(ctx, project, pending);
    }

    fn note_drawn(&mut self, ctx: &egui::Context, project: &Project, pending: usize) {
        let drawn = self
            .rows(ctx, project, pending)
            .into_iter()
            .filter(|r| r.zone == Zone::Main)
            .filter_map(|r| match r.kind {
                super::RowKind::Track(i) => Some((project.tracks[i].id, r.y)),
                _ => None,
            })
            .collect();
        self.row_glides.drawn = drawn;
    }
}
