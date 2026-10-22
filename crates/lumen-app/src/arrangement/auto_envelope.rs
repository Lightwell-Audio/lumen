//! A clip's envelope on the arrangement (M7 slice A7, ADR-0090): where it is
//! drawn (over its clip, first repeat only), its menu and the faint repeats of a
//! looping clip. The drawing, state and gestures are the lane's
//! (`auto_view.rs`, `auto_state.rs`, `app/auto_editing.rs`).

use eframe::egui::{self, Painter, Rect, Stroke, Ui};
use lumen_core::automation::{Lane, Point};
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::model::{Id, Project, Track};

use super::Arrangement;
use super::auto_view::{Frame, polyline};
use crate::document::Document;
use crate::theme;

/// Where a clip's envelope is drawn across the canvas: the left and right edge
/// of the first repeat of the clip, in screen points. `None` when `target` names
/// no clip of the track.
pub(crate) fn envelope_span(
    project: &Project,
    track: Id,
    target: Target,
    frame: &Frame,
) -> Option<(f32, f32)> {
    let (id, _) = target.clip()?;
    let clip = project
        .tracks
        .iter()
        .find(|t| t.id == track)?
        .clips
        .iter()
        .find(|c| c.id == id)?;
    let end = clip.start + lumen_project::envelope::window(clip);
    Some((frame.x(clip.start), frame.x(end)))
}

impl Arrangement {
    /// The rows of the selected audio clips' envelopes, for `track`'s menu.
    pub(super) fn envelope_entries(&mut self, ui: &mut Ui, track: &Track, target: Target) {
        let picked: Vec<&lumen_project::model::Clip> = track
            .clips
            .iter()
            .filter(|c| self.clips.contains(&c.id))
            .filter(|c| matches!(c.content, lumen_project::model::ClipContent::Audio { .. }))
            .map(|c| &**c)
            .collect();
        if picked.is_empty() {
            return;
        }
        ui.separator();
        crate::widgets::menu_caption(ui, "Clip envelopes");
        for clip in picked {
            for kind in lumen_project::envelope::Kind::ALL {
                let option = Target::envelope(clip.id, kind);
                let count = clip.envelopes.get(kind).map_or(0, |l| l.len());
                let label = match count {
                    0 => format!("{} \u{00B7} {}", kind.name(), clip.name),
                    1 => format!("{} \u{00B7} {}  \u{00B7}  1 point", kind.name(), clip.name),
                    n => format!(
                        "{} \u{00B7} {}  \u{00B7}  {n} points",
                        kind.name(),
                        clip.name
                    ),
                };
                if ui.selectable_label(option == target, label).clicked() {
                    self.auto.show(track.id, option);
                    ui.close();
                }
            }
        }
    }

    /// The menu of a clip's envelope: which envelope, taking it away, and the
    /// way back to the track's own lanes.
    pub(super) fn envelope_menu(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        target: Target,
    ) {
        let Some((clip_id, shown_kind)) = target.clip() else {
            return;
        };
        let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) else {
            return;
        };
        for kind in lumen_project::envelope::Kind::ALL {
            let option = Target::envelope(clip_id, kind);
            let label = match clip.envelopes.get(kind).map_or(0, |l| l.len()) {
                0 => kind.name().to_owned(),
                1 => format!("{}  \u{00B7}  1 point", kind.name()),
                n => format!("{}  \u{00B7}  {n} points", kind.name()),
            };
            if ui.selectable_label(option == target, label).clicked() {
                self.auto.show(track.id, option);
                ui.close();
            }
        }
        ui.separator();
        if clip.envelopes.get(shown_kind).is_some() && ui.button("Remove the envelope").clicked() {
            self.note_lane(doc.remove_lane(track.id, target));
            self.auto.picks = None;
            ui.close();
        }
        if ui.button("Show the track's lanes").clicked() {
            let first = track
                .automation
                .first()
                .map_or(Target::Volume, |l| l.target);
            self.auto.show(track.id, first);
            ui.close();
        }
        if ui.button("Hide automation").clicked() {
            self.auto.shown.remove(&track.id);
            self.auto.picks = None;
            ui.close();
        }
    }

    /// The repeats of a looping clip's envelope, dashed and faint: they play
    /// what the first repeat holds, and are not edited.
    pub(super) fn draw_repeats(
        &self,
        painter: &Painter,
        frame: &Frame,
        track: &Track,
        target: Target,
        visible: Rect,
    ) {
        let Some((id, kind)) = target.clip() else {
            return;
        };
        let Some(clip) = track.clips.iter().find(|c| c.id == id) else {
            return;
        };
        let window = lumen_project::envelope::window(clip);
        let Some(lane) = clip.envelopes.get(kind) else {
            return;
        };
        if clip.loop_length.is_none() || window.0 <= 0 || window >= clip.length {
            return;
        }
        let Some(cut) = lumen_project::envelope::windowed(lane, window) else {
            return;
        };
        let palette = *theme::p();
        let end = clip.start + clip.length;
        let left = frame.ticks_at(visible.left());
        // The first repeat in view, and no more than a screenful of them.
        let first = ((left - clip.start).0 / window.0).max(1);
        for k in first..first + 512 {
            let from = clip.start + Ticks(window.0 * k);
            if from >= end || frame.x(from) > visible.right() {
                break;
            }
            let to = (from + window).min(end);
            let (xa, xb) = (frame.x(from), frame.x(to));
            if xb < visible.left() {
                continue;
            }
            let points = cut
                .points()
                .iter()
                .map(|p| Point {
                    at: p.at + from,
                    ..*p
                })
                .collect();
            let Ok(shifted) = Lane::new(points) else {
                continue;
            };
            let line = polyline(frame, target, &shifted, (xa, xb));
            if line.len() >= 2 {
                let span = Rect::from_x_y_ranges(xa..=xb, frame.row.y_range());
                painter
                    .with_clip_rect(span.intersect(painter.clip_rect()))
                    .extend(egui::Shape::dashed_line(
                        &line,
                        Stroke::new(theme::stroke::LIT, palette.accent.gamma_multiply(0.55)),
                        4.0,
                        4.0,
                    ));
            }
        }
    }
}
