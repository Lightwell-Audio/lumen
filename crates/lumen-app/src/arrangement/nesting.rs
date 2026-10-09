//! How group tracks look (M4 slice 3, the owner's choices from rendered
//! specimens): H1, a group's tracks step in by `INDENT` a level with the
//! colour rail of every group they are in running down beside them; L1, a
//! group's lane shows a thin bar per track inside it, in that track's
//! colour, wherever it has clips; and a group's arm button arms what is
//! inside it, lit fully when all of it is armed and half when some is.

use eframe::egui::{Painter, Rect, pos2, vec2};
use lumen_project::groups;
use lumen_project::model::{Project, Track, TrackKind};

use super::{clip_color, color_of, indent};
use crate::theme;
use crate::timeview::TimeView;

/// The colour rail's width.
pub(super) const RAIL: f32 = 4.0;

/// Room above and below the bars in a group's lane.
const BAR_PAD: f32 = 7.0;
/// A bar's height, and the most room one takes with its gap.
const BAR: f32 = 4.0;
const BAR_PITCH: f32 = 7.0;
/// Thinner than this, bars share rows.
const BAR_MIN: f32 = 2.0;

/// A group's arm button: what is inside it is all armed, partly, or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Arm {
    Off,
    Partly,
    All,
}

impl Arm {
    /// Whether the button reads as armed (and a click disarms).
    pub(super) fn on(self) -> bool {
        self == Arm::All
    }
}

/// The arm state of the track at `index`: its own, or for a group, that of
/// the audio and MIDI tracks inside it.
pub(super) fn arm_state(project: &Project, index: usize) -> Arm {
    let tracks = &project.tracks;
    let track = &tracks[index];
    if track.kind != TrackKind::Group {
        return if track.armed { Arm::All } else { Arm::Off };
    }
    let inside = &tracks[groups::span(tracks, index)];
    let armable = inside.iter().filter(|t| t.kind != TrackKind::Group);
    let (count, armed) = armable.fold((0, 0), |(n, a), t| (n + 1, a + usize::from(t.armed)));
    match armed {
        0 => Arm::Off,
        a if a == count => Arm::All,
        _ => Arm::Partly,
    }
}

/// The rails at the left of `rect` (a header): one for each group the
/// track is in, at that group's indent, then its own (red while armed).
/// Returns where the header's content starts.
pub(super) fn draw_rails(
    painter: &Painter,
    rect: Rect,
    project: &Project,
    track: &Track,
    armed: bool,
) -> f32 {
    let tracks = &project.tracks;
    let outer: Vec<_> = groups::ancestors(tracks, track.id).collect();
    for (level, group) in outer.iter().rev().enumerate() {
        if let Some(g) = tracks.iter().find(|t| t.id == *group) {
            let rail = Rect::from_min_size(
                pos2(rect.left() + indent(level), rect.top()),
                vec2(RAIL, rect.height()),
            );
            painter.rect_filled(rail, 0.0, color_of(g.color));
        }
    }
    let left = rect.left() + indent(outer.len());
    let own = Rect::from_min_size(pos2(left, rect.top()), vec2(RAIL, rect.height()));
    let color = if armed {
        theme::p().record
    } else {
        color_of(track.color)
    };
    painter.rect_filled(own, 0.0, color);
    left
}

/// The lane of the group at `index`: a thin bar per track inside it that
/// holds clips, in that track's colour, where its clips are. Only what is
/// in view is drawn.
pub(super) fn draw_group_lane(
    painter: &Painter,
    lane: Rect,
    canvas: Rect,
    project: &Project,
    index: usize,
    time: TimeView,
) {
    let tracks = &project.tracks;
    let inside: Vec<&Track> = tracks[groups::span(tracks, index)]
        .iter()
        .skip(1)
        .filter(|t| t.kind != TrackKind::Group)
        .map(|t| &**t)
        .collect();
    let room = lane.height() - 2.0 * BAR_PAD;
    let Some(layout) = bar_layout(inside.len(), room) else {
        return;
    };
    let painter = painter.with_clip_rect(lane.intersect(canvas));
    let top = lane.center().y - layout.rows as f32 * layout.pitch / 2.0;
    let x = |q: f64| time.x(canvas.left(), q);
    for (i, track) in inside.iter().enumerate() {
        let row = i * layout.rows / inside.len();
        let y = top + row as f32 * layout.pitch + (layout.pitch - layout.bar) / 2.0;
        for clip in &track.clips {
            let (x0, x1) = (
                x(clip.start.as_quarters()),
                x((clip.start + clip.length).as_quarters()),
            );
            if x1 < canvas.left() || x0 > canvas.right() {
                continue;
            }
            let bar = Rect::from_x_y_ranges(
                x0.max(canvas.left() - 1.0)..=x1.min(canvas.right() + 1.0).max(x0 + 1.0),
                y..=y + layout.bar,
            );
            painter.rect_filled(bar, 1.0, clip_color(track, clip));
        }
    }
}

/// Rows of bars in a group's lane: how many, their pitch and thickness.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BarLayout {
    rows: usize,
    pitch: f32,
    bar: f32,
}

/// Bars for `count` tracks in `room` points: one row each, `BAR` thick,
/// thinning as they crowd; below `BAR_MIN`, tracks share rows.
fn bar_layout(count: usize, room: f32) -> Option<BarLayout> {
    if count == 0 || room < BAR_MIN {
        return None;
    }
    let fits = (room / BAR_MIN).floor() as usize;
    let rows = count.min(fits.max(1));
    let pitch = (room / rows as f32).min(BAR_PITCH);
    let bar = (pitch * BAR / BAR_PITCH).max(BAR_MIN.min(pitch));
    Some(BarLayout { rows, pitch, bar })
}

#[cfg(test)]
mod tests {
    use lumen_project::model::{Color, Id};

    use super::*;

    #[test]
    fn bars_thin_as_tracks_crowd_then_share_rows() {
        // A folded group's lane: 44 points, 30 for bars.
        let two = bar_layout(2, 30.0).unwrap();
        assert_eq!((two.rows, two.pitch, two.bar), (2, BAR_PITCH, BAR));
        let eight = bar_layout(8, 30.0).unwrap();
        assert_eq!(eight.rows, 8);
        assert!(eight.pitch * 8.0 <= 30.0 && eight.bar < BAR && eight.bar >= BAR_MIN);
        let fifty = bar_layout(50, 30.0).unwrap();
        assert_eq!(fifty.rows, 15, "two points a row, shared");
        assert_eq!(bar_layout(0, 30.0), None);
        assert_eq!(bar_layout(3, 1.0), None);
    }

    #[test]
    fn a_group_is_armed_when_what_is_inside_is() {
        let mut project = crate::empty_project();
        let track = |id, kind, parent: Option<u64>, armed| {
            std::sync::Arc::new(Track {
                parent: parent.map(Id),
                armed,
                ..Track::new(Id(id), "t", kind, Color(0))
            })
        };
        project.tracks = vec![
            track(1, TrackKind::Group, None, false),
            track(2, TrackKind::Audio, Some(1), true),
            track(3, TrackKind::Group, Some(1), false),
            track(4, TrackKind::Midi, Some(3), false),
        ];
        assert_eq!(arm_state(&project, 0), Arm::Partly);
        assert_eq!(arm_state(&project, 2), Arm::Off);
        assert_eq!(arm_state(&project, 1), Arm::All);
        std::sync::Arc::make_mut(&mut project.tracks[3]).armed = true;
        assert_eq!(arm_state(&project, 0), Arm::All, "nested ones count");
        project.tracks.truncate(1);
        assert_eq!(arm_state(&project, 0), Arm::Off, "an empty group");
    }
}
