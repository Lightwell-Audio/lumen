use eframe::egui::{Painter, Rect, pos2, vec2};
use lumen_project::groups;
use lumen_project::model::{Id, Monitor, Project, Track, TrackKind};

use super::{clip_color, color_of, indent};
use crate::theme;
use crate::timeview::TimeView;

pub(super) const RAIL: f32 = 4.0;

const BAR_PAD: f32 = 7.0;
const BAR: f32 = 4.0;
const BAR_PITCH: f32 = 7.0;
const BAR_MIN: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Arm {
    Off,
    Partly,
    All,
    Listening,
}

impl Arm {
    pub(super) fn on(self) -> bool {
        self == Arm::All
    }
}

pub(super) fn arm_look(project: &Project, index: usize, selected: &[Id]) -> Arm {
    let track = &project.tracks[index];
    match arm_state(project, index) {
        Arm::Off
            if track.kind == TrackKind::Midi
                && track.monitor == Monitor::Auto
                && selected.contains(&track.id) =>
        {
            Arm::Listening
        }
        arm => arm,
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq)]
struct BarLayout {
    rows: usize,
    pitch: f32,
    bar: f32,
}

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

    #[test]
    fn a_selected_midi_track_on_auto_shows_the_hollow_ring_until_armed() {
        let mut project = crate::empty_project();
        let midi = std::sync::Arc::new(Track::new(Id(1), "m", TrackKind::Midi, Color(0)));
        let audio = std::sync::Arc::new(Track::new(Id(2), "a", TrackKind::Audio, Color(0)));
        project.tracks = vec![midi, audio];
        assert_eq!(arm_look(&project, 0, &[]), Arm::Off);
        assert_eq!(arm_look(&project, 0, &[Id(1)]), Arm::Listening);
        assert!(!Arm::Listening.on(), "a click arms it");
        assert_eq!(arm_look(&project, 1, &[Id(2)]), Arm::Off, "audio tracks");
        std::sync::Arc::make_mut(&mut project.tracks[0]).monitor = Monitor::In;
        assert_eq!(arm_look(&project, 0, &[Id(1)]), Arm::Off);
        std::sync::Arc::make_mut(&mut project.tracks[0]).monitor = Monitor::Auto;
        std::sync::Arc::make_mut(&mut project.tracks[0]).armed = true;
        assert_eq!(arm_look(&project, 0, &[Id(1)]), Arm::All, "a real arm");
    }
}
