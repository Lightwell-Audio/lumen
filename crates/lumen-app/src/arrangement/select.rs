//! Clip selection and moving (M3 slice 6a), in the owner's split-clip
//! model: a clip's top half moves it (Ctrl copies), its bottom half selects
//! a time range (slice 6b), and a drag in empty lanes draws a marquee.
//!
//! Hit-testing and the arithmetic of a drag are pure and tested here; the
//! app turns them into document edits (`App::clip_editing`).

use std::collections::BTreeSet;
use std::sync::Arc;

use eframe::egui::{self, Pos2, Rect, pos2};
use lumen_core::time::Ticks;
use lumen_project::model::{ClipContent, Id, Project, TrackKind};

use super::clips::INSET;
use super::{Arrangement, RowKind};
use crate::document::ClipMove;
use crate::timeview::TimeView;

/// Each half of a clip is at least this tall, so both stay easy to hit on
/// short tracks (the owner's split-clip choice).
pub const MIN_HALF: f32 = 11.0;
/// Within this many points of a clip's side, a drag trims it (slice 6c; a
/// quarter of the clip when it is narrow).
pub const EDGE_ZONE: f32 = 6.0;

pub use lumen_project::edit::Edge;

/// The side of `rect` that `x` is close enough to trim, if any.
pub fn edge_at(rect: Rect, x: f32) -> Option<Edge> {
    let zone = EDGE_ZONE.min(rect.width() / 4.0);
    if x - rect.left() <= zone {
        Some(Edge::Start)
    } else if rect.right() - x <= zone {
        Some(Edge::End)
    } else {
        None
    }
}

/// Which half of a clip the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    /// Moves the clip.
    Top,
    /// Selects a time range.
    Bottom,
}

/// Where the line between the halves of a clip `rect` falls.
pub fn split_y(rect: Rect) -> f32 {
    let mid = rect.center().y;
    if rect.height() < 2.0 * MIN_HALF {
        mid
    } else {
        mid.clamp(rect.top() + MIN_HALF, rect.bottom() - MIN_HALF)
    }
}

pub fn half_at(rect: Rect, y: f32) -> Half {
    if y < split_y(rect) {
        Half::Top
    } else {
        Half::Bottom
    }
}

/// A clip under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipHit {
    /// Index of its track in the project.
    pub track: usize,
    pub clip: Id,
    /// Where it is drawn.
    pub rect: Rect,
    pub half: Half,
    /// A side close enough to trim, which wins over the halves.
    pub edge: Option<Edge>,
}

/// A clip picked up by a drag, as it was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grabbed {
    pub clip: Id,
    /// Index of its track in the project.
    pub track: usize,
    pub start: Ticks,
    /// Audio content (lands only on audio tracks), or MIDI.
    pub audio: bool,
}

/// What a pointer drag on the lanes is doing. Edits keep `base`, the
/// project as the drag found it: every frame redoes the edit from there.
#[derive(Debug, Clone)]
pub enum Gesture {
    /// Moving (or copying) clips: picked up at `at` on track `track`;
    /// `anchor` is the clip under the pointer, the one that snaps.
    Move {
        base: Arc<Project>,
        grabbed: Vec<Grabbed>,
        anchor: Id,
        at: Ticks,
        track: usize,
        /// Ids for the copies, when Ctrl started the drag.
        copies: Option<Vec<Id>>,
    },
    /// A marquee from `from`; clips selected before it (Ctrl or Shift) stay.
    Marquee { from: Pos2, kept: BTreeSet<Id> },
    /// A time range from `at` on track `track` (a clip's bottom half).
    Range { at: Ticks, track: usize },
    /// Trimming a side of `clip`; with `looping` (Shift at the start) the
    /// end extends by repeating.
    Trim {
        base: Arc<Project>,
        clip: Id,
        edge: Edge,
        looping: bool,
    },
    /// Sliding `clip`'s content, picked up at `at` (Ctrl+Alt).
    Slip {
        base: Arc<Project>,
        clip: Id,
        at: Ticks,
    },
}

/// A time range across neighbouring tracks, independent of clips (slice
/// 6b edits it: cut, copy, duplicate, delete, delete time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeRange {
    pub start: Ticks,
    pub end: Ticks,
    /// The tracks it spans, top to bottom.
    pub tracks: Vec<Id>,
}

impl TimeRange {
    /// The range between `a` and `b` (either order) over the tracks from
    /// index `from` to `to` (either order); `None` when it is empty.
    pub fn between(project: &Project, a: Ticks, b: Ticks, from: usize, to: usize) -> Option<Self> {
        let (start, end) = (a.min(b).max(Ticks::ZERO), a.max(b).max(Ticks::ZERO));
        let (lo, hi) = (from.min(to), from.max(to));
        let tracks: Vec<Id> = project.tracks.get(lo..=hi)?.iter().map(|t| t.id).collect();
        (start < end).then_some(Self { start, end, tracks })
    }
}

/// Where the clips of a drag go when it has moved `delta` (unsnapped) and
/// `tracks` rows down (negative: up). The anchor's new start snaps; the
/// others keep their distance from it. Nothing goes before zero. Clips
/// change track only if every one lands on a track of its kind; otherwise
/// they stay on their own.
pub fn drag_moves(
    project: &Project,
    grabbed: &[Grabbed],
    anchor: Id,
    delta: Ticks,
    tracks: isize,
    snap: impl Fn(Ticks) -> Ticks,
) -> Vec<ClipMove> {
    let Some(first) = grabbed.iter().find(|g| g.clip == anchor) else {
        return Vec::new();
    };
    let earliest = grabbed.iter().map(|g| g.start).min().unwrap_or_default();
    let shift = (snap(first.start + delta) - first.start).max(Ticks::ZERO - earliest);
    let lands = |g: &Grabbed| {
        let to = usize::try_from(g.track as isize + tracks).ok()?;
        let kind = project.tracks.get(to)?.kind;
        let fits = if g.audio {
            kind == TrackKind::Audio
        } else {
            kind == TrackKind::Midi
        };
        fits.then_some(to)
    };
    let across = grabbed.iter().all(|g| lands(g).is_some());
    grabbed
        .iter()
        .map(|g| {
            let track = if across {
                lands(g).unwrap_or(g.track)
            } else {
                g.track
            };
            ClipMove {
                clip: g.clip,
                track: project.tracks[track].id,
                start: g.start + shift,
            }
        })
        .collect()
}

impl Arrangement {
    /// Every track's clips with where they are drawn, for the tracks in
    /// view: `(track index, clip id, rect)`.
    pub(crate) fn clip_rects(
        &self,
        ctx: &egui::Context,
        canvas: Rect,
        project: &Project,
        pending: usize,
        time: TimeView,
    ) -> Vec<(usize, Id, Rect)> {
        let mut out = Vec::new();
        for row in self.rows(ctx, project, pending) {
            let RowKind::Track(index) = row.kind else {
                continue;
            };
            let top = canvas.top() + row.y - self.top;
            if top > canvas.bottom() || top + row.height < canvas.top() {
                continue;
            }
            for clip in &project.tracks[index].clips {
                let x0 = time.x(canvas.left(), clip.start.as_quarters());
                let x1 = time.x(canvas.left(), (clip.start + clip.length).as_quarters());
                if x1 < canvas.left() || x0 > canvas.right() {
                    continue;
                }
                let rect = Rect::from_min_max(
                    pos2(x0, top + INSET),
                    pos2(x1.max(x0 + 1.0), top + row.height - INSET),
                );
                out.push((index, clip.id, rect));
            }
        }
        out
    }

    /// The clip under `pointer`, and which half. Later clips win where
    /// clips overlap, as they are drawn on top.
    pub fn clip_at(
        &self,
        ctx: &egui::Context,
        pointer: Pos2,
        canvas: Rect,
        project: &Project,
        pending: usize,
        time: TimeView,
    ) -> Option<ClipHit> {
        self.clip_rects(ctx, canvas, project, pending, time)
            .into_iter()
            .rev()
            .find(|(_, _, rect)| rect.contains(pointer))
            .map(|(track, clip, rect)| ClipHit {
                track,
                clip,
                rect,
                half: half_at(rect, pointer.y),
                edge: edge_at(rect, pointer.x),
            })
    }

    /// The clips a marquee `area` touches.
    pub fn clips_in(
        &self,
        ctx: &egui::Context,
        area: Rect,
        canvas: Rect,
        project: &Project,
        pending: usize,
        time: TimeView,
    ) -> Vec<Id> {
        self.clip_rects(ctx, canvas, project, pending, time)
            .into_iter()
            .filter(|(_, _, rect)| rect.intersects(area))
            .map(|(_, id, _)| id)
            .collect()
    }

    /// The index of the track whose row is at `y` on screen, or the nearest
    /// one above or below the tracks.
    pub fn track_index_at(
        &self,
        ctx: &egui::Context,
        project: &Project,
        pending: usize,
        canvas: Rect,
        y: f32,
    ) -> Option<usize> {
        let y = y - canvas.top() + self.top;
        let rows: Vec<_> = self
            .rows(ctx, project, pending)
            .into_iter()
            .filter_map(|r| match r.kind {
                RowKind::Track(i) => Some((i, r)),
                RowKind::Pending(_) => None,
            })
            .collect();
        let (first, last) = (rows.first()?, rows.last()?);
        if y < first.1.y {
            return Some(first.0);
        }
        rows.iter()
            .find(|(_, r)| (r.y..r.y + r.height).contains(&y))
            .map_or(Some(last.0), |(i, _)| Some(*i))
    }

    /// What `clips` are, picked up for a drag.
    pub fn grab(project: &Project, clips: &BTreeSet<Id>) -> Vec<Grabbed> {
        let mut out = Vec::new();
        for (track, t) in project.tracks.iter().enumerate() {
            for clip in &t.clips {
                if clips.contains(&clip.id) && !clip.locked {
                    out.push(Grabbed {
                        clip: clip.id,
                        track,
                        start: clip.start,
                        audio: matches!(clip.content, ClipContent::Audio { .. }),
                    });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;
    use lumen_project::model::Track;

    #[test]
    fn a_range_spans_the_tracks_between_its_corners_either_way() {
        use TrackKind::Audio;
        let p = project(&[Audio, Audio, Audio, Audio]);
        let r = TimeRange::between(&p, Ticks::quarters(8), Ticks::quarters(4), 3, 1).unwrap();
        assert_eq!((r.start, r.end), (Ticks::quarters(4), Ticks::quarters(8)));
        assert_eq!(r.tracks, [Id(2), Id(3), Id(4)]);
        assert!(
            TimeRange::between(&p, Ticks::ZERO, Ticks::ZERO, 0, 0).is_none(),
            "empty"
        );
        assert!(
            TimeRange::between(&p, Ticks::ZERO, Ticks(1), 0, 9).is_none(),
            "no such track"
        );
    }

    #[test]
    fn sides_trim_within_a_few_points_or_a_quarter_of_a_narrow_clip() {
        let wide = Rect::from_min_size(pos2(100.0, 0.0), vec2(200.0, 40.0));
        assert_eq!(edge_at(wide, 104.0), Some(Edge::Start));
        assert_eq!(edge_at(wide, 296.0), Some(Edge::End));
        assert_eq!(edge_at(wide, 150.0), None);
        let narrow = Rect::from_min_size(pos2(0.0, 0.0), vec2(12.0, 40.0));
        assert_eq!(edge_at(narrow, 6.0), None, "the middle still moves it");
        assert_eq!(edge_at(narrow, 2.0), Some(Edge::Start));
    }

    #[test]
    fn halves_split_in_the_middle_but_stay_hittable() {
        let tall = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 80.0));
        assert_eq!(split_y(tall), 40.0);
        assert_eq!(half_at(tall, 39.0), Half::Top);
        assert_eq!(half_at(tall, 41.0), Half::Bottom);
        let short = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 18.0));
        assert_eq!(split_y(short), 9.0, "too short for 11 each: halve it");
    }

    fn project(kinds: &[TrackKind]) -> Project {
        let mut project = crate::empty_project();
        project.tracks = kinds
            .iter()
            .enumerate()
            .map(|(i, &kind)| {
                std::sync::Arc::new(Track {
                    id: Id(i as u64 + 1),
                    name: format!("T{i}"),
                    kind,
                    color: lumen_project::model::Color(0),
                    parent: None,
                    gain_db: 0.0,
                    pan: 0.0,
                    mute: false,
                    solo: false,
                    armed: false,
                    devices: vec![],
                    clips: vec![],
                })
            })
            .collect();
        project
    }

    fn grabbed(clip: u64, track: usize, quarters: i64) -> Grabbed {
        Grabbed {
            clip: Id(clip),
            track,
            start: Ticks::quarters(quarters),
            audio: true,
        }
    }

    /// Snaps to whole bars.
    fn bars(t: Ticks) -> Ticks {
        let bar = Ticks::quarters(4).0;
        Ticks((t.0 as f64 / bar as f64).round() as i64 * bar)
    }

    #[test]
    fn the_anchor_snaps_and_the_others_keep_their_distance() {
        use TrackKind::Audio;
        let p = project(&[Audio, Audio, Audio]);
        let clips = [grabbed(10, 0, 4), grabbed(11, 1, 6)];
        // Dragged 3.2 quarters right: the anchor (bar 2) snaps to bar 3.
        let moves = drag_moves(
            &p,
            &clips,
            Id(10),
            Ticks(Ticks::quarters(3).0 + 300_000),
            1,
            bars,
        );
        let got: Vec<_> = moves
            .iter()
            .map(|m| (m.clip.0, m.track.0, m.start))
            .collect();
        assert_eq!(
            got,
            [(10, 2, Ticks::quarters(8)), (11, 3, Ticks::quarters(10))]
        );
        // Never before zero: the earliest clip stops at the start.
        let moves = drag_moves(&p, &clips, Id(11), Ticks::quarters(-40), 0, |t| t);
        assert_eq!(moves[0].start, Ticks::ZERO);
        assert_eq!(moves[1].start, Ticks::quarters(2));
    }

    #[test]
    fn clips_change_track_only_where_every_one_fits() {
        use TrackKind::{Audio, Group, Midi};
        let p = project(&[Audio, Audio, Midi, Group]);
        let clips = [grabbed(10, 0, 0), grabbed(11, 1, 0)];
        let tracks = |down| {
            drag_moves(&p, &clips, Id(10), Ticks::ZERO, down, |t| t)
                .iter()
                .map(|m| m.track.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(tracks(0), [1, 2]);
        // One down would put the second clip on the MIDI track: both stay.
        assert_eq!(tracks(1), [1, 2]);
        assert_eq!(tracks(-1), [1, 2], "above the first track");
        let one = [grabbed(10, 1, 0)];
        let moved = drag_moves(&p, &one, Id(10), Ticks::ZERO, -1, |t| t);
        assert_eq!(moved[0].track, Id(1));
    }
}
