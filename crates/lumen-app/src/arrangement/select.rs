//! Clip selection and moving: a clip's header moves it (Ctrl copies), its body selects
//! a time range (slice 6b). A drag in empty lanes selects a time range on
//! the tracks it spans (ADR-0049); Ctrl+Shift there draws a clip marquee.
//!
//! Hit-testing and the arithmetic of a drag are pure and tested here; the
//! app turns them into document edits (`App::clip_editing`).

use std::collections::BTreeSet;
use std::sync::Arc;

use eframe::egui::{self, Modifiers, Pos2, Rect, pos2};
use lumen_core::time::Ticks;
use lumen_project::model::{ClipContent, Id, Project, TrackKind};

use super::clips::INSET;
use super::handles::{ClipHandles, Handle};
use super::{Arrangement, RowKind, TITLE, Zone};
use crate::document::ClipMove;
use crate::timeview::TimeView;

/// Within this many points of a clip's side, a drag trims it (slice 6c; a
/// quarter of the clip when it is narrow).
pub const EDGE_ZONE: f32 = 6.0;

pub use lumen_project::edit::Edge;

/// Where an arrow key takes the clip selection (M3 review H2; the owner's
/// choice: the arrows follow the selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipStep {
    /// The clip before, on the same track.
    Previous,
    /// The clip after, on the same track.
    Next,
    /// The nearest clip in time on the closest track above that has clips.
    Above,
    /// The same, below.
    Below,
}

/// The clip `step` reaches from clip `from`, if there is one.
pub fn clip_step(project: &Project, from: Id, step: ClipStep) -> Option<Id> {
    let (row, clip) = project
        .tracks
        .iter()
        .enumerate()
        .find_map(|(i, t)| t.clips.iter().find(|c| c.id == from).map(|c| (i, c)))?;
    let by_time = |t: usize| {
        let mut clips: Vec<_> = project.tracks[t].clips.iter().collect();
        clips.sort_by_key(|c| (c.start, c.id));
        clips
    };
    match step {
        ClipStep::Previous | ClipStep::Next => {
            let clips = by_time(row);
            let at = clips.iter().position(|c| c.id == from)?;
            let to = if step == ClipStep::Next {
                at + 1
            } else {
                at.checked_sub(1)?
            };
            clips.get(to).map(|c| c.id)
        }
        ClipStep::Above | ClipStep::Below => {
            let has_clips = |t: &usize| !project.tracks[*t].clips.is_empty();
            let row = if step == ClipStep::Above {
                (0..row).rev().find(has_clips)
            } else {
                (row + 1..project.tracks.len()).find(has_clips)
            }?;
            // Nearest to where the clip starts: one under it first.
            let at = clip.start;
            let distance = |c: &&Arc<lumen_project::model::Clip>| {
                let end = c.start + c.length;
                if at < c.start {
                    c.start - at
                } else if at >= end {
                    at - end + Ticks(1)
                } else {
                    Ticks::ZERO
                }
            };
            by_time(row).into_iter().min_by_key(distance).map(|c| c.id)
        }
    }
}

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

/// Which visible part of a clip the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipRegion {
    /// Moves the clip.
    Header,
    /// Selects a time range.
    Body,
}

/// The bottom of the rendered title bar, including on short tracks.
pub fn title_bottom(rect: Rect) -> f32 {
    rect.top() + TITLE.min(rect.height())
}

pub fn region_at(rect: Rect, y: f32) -> ClipRegion {
    if y < title_bottom(rect) {
        ClipRegion::Header
    } else {
        ClipRegion::Body
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
    pub region: ClipRegion,
    /// A side close enough to trim, which wins over the regions.
    pub edge: Option<Edge>,
    /// A fade or gain handle (slice 6d): fade handles win over the sides,
    /// the sides over the gain line.
    pub handle: Option<Handle>,
}

/// What the pointer is over on a clip, for drawing its hover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hover {
    pub clip: Id,
    pub region: ClipRegion,
    pub edge: Option<Edge>,
    pub handle: Option<Handle>,
}

impl From<&ClipHit> for Hover {
    fn from(hit: &ClipHit) -> Self {
        Self {
            clip: hit.clip,
            region: hit.region,
            edge: hit.edge,
            handle: hit.handle,
        }
    }
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
    /// A time range from `at` on track `track` (a clip's body, or an
    /// empty lane); it spans the tracks down to the pointer's.
    Range { at: Ticks, track: usize },
    /// Trimming a side of `clip`; with `looping` (Shift at the start) the
    /// end extends by repeating. `grab` is how far the edge was from the
    /// pointer when it was picked up, so the edge keeps that distance
    /// instead of jumping to the pointer.
    Trim {
        base: Arc<Project>,
        clip: Id,
        edge: Edge,
        looping: bool,
        grab: Ticks,
    },
    /// Sliding `clip`'s content, picked up at `at` (Ctrl+Alt).
    Slip {
        base: Arc<Project>,
        clip: Id,
        at: Ticks,
    },
    /// Dragging the handle of `clip`'s fade at `edge` along (slice 6d).
    /// `from`: a fade-out that has been dragged past the clip's end, so it
    /// is making a crossfade into the next clip; where the clips met.
    Fade {
        clip: Id,
        edge: Edge,
        from: Option<Ticks>,
    },
    /// Bending that fade by its dot (C3): the handle as it is now, the
    /// pointer's last place, and the fade's size on screen.
    Bend {
        clip: Id,
        edge: Edge,
        /// The fade is the crossfade at the clip's end, curved like a
        /// fade-in.
        crossing: bool,
        handle: lumen_core::curve::Handle,
        last: Pos2,
        size: egui::Vec2,
    },
    /// Dragging `clip`'s gain line from `db`, picked up at height `y`;
    /// `per_point` dB a point.
    Gain {
        clip: Id,
        db: f32,
        y: f32,
        per_point: f32,
    },
    /// The Split tool's press-and-aim gesture; the document changes on release.
    Split {
        from: Ticks,
        to: Ticks,
        clips: Vec<Id>,
    },
    /// Zoom tool's drag selection.
    Zoom { from: Ticks, to: Ticks },
    /// The Draw tool's press-and-drag on an empty MIDI lane: the clip's
    /// length being aimed. The document changes on release.
    Draw { track: Id, from: Ticks, to: Ticks },
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
        let (lo, hi) = (from.min(to), from.max(to));
        let ids: Vec<Id> = project.tracks.get(lo..=hi)?.iter().map(|t| t.id).collect();
        Self::over(project, a, b, &ids)
    }

    /// The range between `a` and `b` (either order) on the tracks `ids`,
    /// as many of them as the project has, top to bottom. `None` when it is
    /// empty or none of `ids` is a track. Tracks need not be neighbours:
    /// the ruler's range covers the selected ones.
    pub fn over(project: &Project, a: Ticks, b: Ticks, ids: &[Id]) -> Option<Self> {
        let (start, end) = (a.min(b).max(Ticks::ZERO), a.max(b).max(Ticks::ZERO));
        let tracks: Vec<Id> = project
            .tracks
            .iter()
            .map(|t| t.id)
            .filter(|id| ids.contains(id))
            .collect();
        (start < end && !tracks.is_empty()).then_some(Self { start, end, tracks })
    }

    /// Whether the range is on exactly the tracks `selected` (in any order).
    /// A range and the track selection go together: when they part, the
    /// range goes.
    pub fn covers_exactly(&self, selected: &[Id]) -> bool {
        self.tracks.len() == selected.len() && self.tracks.iter().all(|t| selected.contains(t))
    }
}

/// What a drag that starts in empty lane space does (ADR-0049).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyLaneDrag {
    /// A time range on the tracks it spans: the default.
    Range,
    /// A clip marquee that keeps the clips already selected.
    Marquee,
}

/// Ctrl+Shift (Cmd+Shift on macOS) draws a clip marquee in empty lanes.
/// Shift or Ctrl alone do not: they make a time range, as the drag does
/// everywhere else, so no modifier stands for two things.
pub fn empty_lane_drag(modifiers: Modifiers) -> EmptyLaneDrag {
    if modifiers.command && modifiers.shift {
        EmptyLaneDrag::Marquee
    } else {
        EmptyLaneDrag::Range
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
            let (RowKind::Track(index), Zone::Main) = (row.kind, row.zone) else {
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

    /// The clip under `pointer`, and which region. Later clips win where
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
            .map(|(track, clip, rect)| {
                let edge = edge_at(rect, pointer.x);
                let handle = project.tracks[track]
                    .clips
                    .iter()
                    .find(|c| c.id == clip && !c.locked)
                    .and_then(|c| {
                        let shown =
                            crate::crossfades::preview(project, project.tracks[track].id, c);
                        ClipHandles::of(&shown, rect, |t| time.x(canvas.left(), t.as_quarters()))
                    })
                    .and_then(|h| h.handle_at(pointer))
                    .filter(|h| edge.is_none() || *h != Handle::Gain);
                ClipHit {
                    track,
                    clip,
                    rect,
                    region: region_at(rect, pointer.y),
                    edge: if handle.is_some() { None } else { edge },
                    handle,
                }
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
            .filter(|r| r.zone == Zone::Main)
            .filter_map(|r| match r.kind {
                RowKind::Track(i) => Some((i, r)),
                RowKind::Pending(_) | RowKind::Master => None,
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

    /// Where track `id`'s clip lane is on screen: the top and bottom of its
    /// clips' height, if the track has a row in view.
    pub(crate) fn lane_span(
        &self,
        ctx: &egui::Context,
        project: &Project,
        pending: usize,
        canvas: Rect,
        id: Id,
    ) -> Option<(f32, f32)> {
        let index = project.tracks.iter().position(|t| t.id == id)?;
        self.rows(ctx, project, pending)
            .into_iter()
            .find(|r| r.zone == Zone::Main && r.kind == RowKind::Track(index))
            .map(|row| {
                let top = canvas.top() + row.y - self.top;
                (top + INSET, top + row.height - INSET)
            })
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
    fn a_range_over_chosen_tracks_skips_the_ones_between() {
        use TrackKind::Audio;
        let p = project(&[Audio, Audio, Audio, Audio]);
        let (a, b) = (Ticks::quarters(4), Ticks::quarters(8));
        let r = TimeRange::over(&p, b, a, &[Id(3), Id(1)]).unwrap();
        assert_eq!((r.start, r.end), (a, b));
        assert_eq!(
            r.tracks,
            [Id(1), Id(3)],
            "top to bottom, not the list's order"
        );
        assert!(r.covers_exactly(&[Id(3), Id(1)]));
        assert!(!r.covers_exactly(&[Id(1)]), "a track more");
        assert!(!r.covers_exactly(&[Id(1), Id(2)]), "another track");
        assert!(TimeRange::over(&p, a, b, &[]).is_none(), "no tracks");
        assert!(TimeRange::over(&p, a, a, &[Id(1)]).is_none(), "empty");
    }

    #[test]
    fn an_empty_lane_drag_makes_a_range_unless_ctrl_shift_is_held() {
        let ctrl_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        assert_eq!(empty_lane_drag(Modifiers::NONE), EmptyLaneDrag::Range);
        assert_eq!(empty_lane_drag(Modifiers::SHIFT), EmptyLaneDrag::Range);
        assert_eq!(empty_lane_drag(Modifiers::COMMAND), EmptyLaneDrag::Range);
        assert_eq!(empty_lane_drag(Modifiers::ALT), EmptyLaneDrag::Range);
        assert_eq!(empty_lane_drag(ctrl_shift), EmptyLaneDrag::Marquee);
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
    fn gesture_regions_follow_the_rendered_title_on_every_track_height() {
        for height in [10.0, 18.0, 80.0, 200.0] {
            let rect = Rect::from_min_size(pos2(0.0, 20.0), vec2(100.0, height));
            let bottom = 20.0 + TITLE.min(height);
            assert_eq!(title_bottom(rect), bottom);
            assert_eq!(region_at(rect, bottom - 0.1), ClipRegion::Header);
            if height > TITLE {
                assert_eq!(region_at(rect, bottom), ClipRegion::Body);
                assert_eq!(region_at(rect, bottom + 0.1), ClipRegion::Body);
            }
        }
    }

    #[test]
    fn arrows_step_along_a_track_and_to_the_nearest_clip_above_or_below() {
        use ClipStep::{Above, Below, Next, Previous};
        use TrackKind::Audio;
        let mut p = project(&[Audio, Audio, Audio, Audio]);
        // Clips (id: start..end in quarters) per track; track 2 is empty.
        let layout: [&[(u64, i64, i64)]; 4] = [
            &[(11, 0, 4), (12, 8, 12)],
            &[(22, 6, 10), (21, 0, 2), (23, 16, 20)],
            &[],
            &[(41, 3, 5), (42, 7, 9)],
        ];
        for (t, clips) in layout.iter().enumerate() {
            let track = std::sync::Arc::make_mut(&mut p.tracks[t]);
            for &(id, start, end) in *clips {
                track.clips.push(Arc::new(lumen_project::model::Clip {
                    id: Id(id),
                    name: format!("C{id}"),
                    start: Ticks::quarters(start),
                    length: Ticks::quarters(end - start),
                    muted: false,
                    color: None,
                    locked: false,
                    crossfade: false,
                    fade_in: Default::default(),
                    fade_out: Default::default(),
                    loop_length: None,
                    loop_phase: Default::default(),
                    envelopes: Default::default(),
                    audio_phase_seconds: 0.0,
                    reversed: false,
                    inverted: false,
                    content: ClipContent::midi(),
                }));
            }
        }
        let step = |from, s| clip_step(&p, Id(from), s).map(|id| id.0);
        // Along a track, in time order whatever the stored order.
        assert_eq!(step(21, Next), Some(22));
        assert_eq!(step(22, Previous), Some(21));
        assert_eq!(step(23, Next), None, "the last stays");
        assert_eq!(step(21, Previous), None, "the first stays");
        // Down from 22 (starts at 6): track 2 is empty, so track 3, where
        // 42 (7..9) is nearer than 41 (3..5).
        assert_eq!(step(22, Below), Some(42));
        // Up from 12 (starts at 8): 22 (6..10) lies under its start.
        assert_eq!(step(12, Below), Some(22));
        assert_eq!(
            step(41, Above),
            Some(21),
            "3 is 1 after 21's end, 3 before 22"
        );
        assert_eq!(step(11, Above), None, "nothing above the top track");
        assert_eq!(step(99, Next), None, "a clip that has gone");
    }

    fn project(kinds: &[TrackKind]) -> Project {
        let mut project = crate::empty_project();
        project.tracks = kinds
            .iter()
            .enumerate()
            .map(|(i, &kind)| {
                std::sync::Arc::new(Track::new(
                    Id(i as u64 + 1),
                    format!("T{i}"),
                    kind,
                    lumen_project::model::Color(0),
                ))
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
