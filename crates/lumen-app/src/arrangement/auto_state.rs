//! What the arrangement remembers about the lanes it shows (M7 slices A1
//! and A2): which lane each track shows, the points picked on it, the tool
//! in hand, and the gesture under way. None of it is saved with the project.
//!
//! A gesture is one value, so a press can only be doing one thing: moving
//! points, dragging out a box, drawing freehand, drawing a shape or bending a
//! segment. Each keeps what it needs to rebuild the lane from the press
//! (`base`), so a drag is always the lane as it was plus the drag, never a
//! drag of a drag.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use eframe::egui::Pos2;
use lumen_core::automation::Lane;
use lumen_core::curve::Handle;
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::edit::automation::{Copied, Wave};
use lumen_project::model::{Id, Project};

/// A point of a lane, named by where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Picked {
    pub track: Id,
    pub target: Target,
    /// Its place in the lane's points.
    pub index: usize,
}

/// The points picked on one lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Picks {
    pub track: Id,
    pub target: Target,
    /// Their places in the lane's points.
    pub indices: BTreeSet<usize>,
}

impl Picks {
    pub fn only(here: Picked) -> Self {
        Self {
            track: here.track,
            target: here.target,
            indices: BTreeSet::from([here.index]),
        }
    }

    pub fn on(&self, track: Id, target: Target) -> bool {
        self.track == track && self.target == target
    }

    pub fn contains(&self, here: Picked) -> bool {
        self.on(here.track, here.target) && self.indices.contains(&here.index)
    }

    /// The places in ascending order, as the pure edits take them.
    pub fn list(&self) -> Vec<usize> {
        self.indices.iter().copied().collect()
    }

    /// The one point picked, if exactly one is.
    pub fn single(&self) -> Option<Picked> {
        let mut it = self.indices.iter();
        match (it.next(), it.next()) {
            (Some(&index), None) => Some(Picked {
                track: self.track,
                target: self.target,
                index,
            }),
            _ => None,
        }
    }
}

/// What the pointer does on a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tool {
    /// Pick, move and add points.
    #[default]
    Pointer,
    /// Draw freehand.
    Pencil,
    /// Draw a ramp, sine, triangle, saw or square across a span.
    Shape(Wave),
}

pub(crate) fn wave_name(wave: Wave) -> &'static str {
    match wave {
        Wave::Ramp => "Ramp",
        Wave::Sine => "Sine",
        Wave::Triangle => "Triangle",
        Wave::Saw => "Saw",
        Wave::Square => "Square",
    }
}

/// Picked points being moved: the lane at the press, which points, and
/// where the pointer and the anchor point were.
#[derive(Debug, Clone)]
pub(crate) struct Moving {
    pub base: Lane,
    pub picks: Vec<usize>,
    /// The point the pointer took hold of.
    pub anchor: usize,
    /// The anchor's time and height (0 to 1) at the press.
    pub anchor_at: Ticks,
    pub anchor_height: f32,
    /// The pointer's time and height at the press.
    pub press_at: Ticks,
    pub press_height: f32,
}

/// A box being dragged out on the lane.
#[derive(Debug, Clone)]
pub(crate) struct Boxing {
    pub from: Pos2,
    pub to: Pos2,
}

/// A freehand stroke: the lane at the press and the places the pointer went.
#[derive(Debug, Clone)]
pub(crate) struct Drawing {
    pub base: Lane,
    /// In time order; going back in time takes the stroke back.
    pub raw: Vec<(Ticks, f32)>,
}

/// A shape being drawn from a press.
#[derive(Debug, Clone)]
pub(crate) struct Waving {
    pub base: Lane,
    pub kind: Wave,
    pub from: Ticks,
    /// Heights (0 to 1) of the press and the pointer now.
    pub start: f32,
    pub end: f32,
    pub to: Ticks,
    /// One cycle: the grid step at the press.
    pub period: Ticks,
}

/// A segment's curve handle being dragged.
#[derive(Debug, Clone)]
pub(crate) struct Bending {
    pub base: Lane,
    /// The segment leaves this point.
    pub index: usize,
    pub handle: Handle,
    pub press: Pos2,
}

#[derive(Debug, Clone)]
pub(crate) enum Gesture {
    Move(Moving),
    Box(Boxing),
    Draw(Drawing),
    Wave(Waving),
    Bend(Bending),
}

/// A gesture and the lane it is on.
#[derive(Debug, Clone)]
pub(crate) struct Active {
    pub track: Id,
    pub target: Target,
    pub how: Gesture,
}

/// Points copied from a lane, and the range of values they were in: they
/// paste into a lane of the same kind of parameter only.
#[derive(Debug, Clone)]
pub(crate) struct LaneClip {
    pub copied: Copied,
    pub range: (f32, f32),
}

/// The chooser for a plugin's parameters (M7 slice A3): whose they are, the
/// search over them and the group chosen.
#[derive(Debug, Clone)]
pub(crate) struct ParamChooser {
    pub track: Id,
    pub device: Id,
    pub search: String,
    /// One group of the plugin's, or all of them.
    pub group: Option<i32>,
    /// The search field takes the keyboard when the chooser opens or changes
    /// plugin.
    pub focus: bool,
    /// The row Enter chooses.
    pub cursor: usize,
}

impl ParamChooser {
    pub fn new(track: Id, device: Id) -> Self {
        Self {
            track,
            device,
            search: String::new(),
            group: None,
            focus: true,
            cursor: 0,
        }
    }
}

/// What a control's menu asks of the app about controller mappings (M7 slice
/// A5): the controls are drawn where the app's state is not at hand, so they
/// ask and the app answers after the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapRequest {
    /// Wait for a controller to move, to map it to this control.
    Learn(Id, Target),
    /// Stop waiting.
    StopLearning,
    /// Take away the controller that moves this control.
    Clear(Id, Target),
    /// Open the Mappings window.
    Window,
}

/// What the arrangement knows of the lanes it shows.
#[derive(Debug, Default)]
pub(crate) struct AutoView {
    /// The control waiting for a controller to move (set by the app every
    /// frame), which draws with an accent ring.
    pub learning: Option<(Id, Target)>,
    /// What the controls' menus asked for this frame.
    pub requests: Vec<MapRequest>,
    /// The plugin parameter chooser, when open.
    pub chooser: Option<ParamChooser>,
    /// What the plugins call the parameters lanes drive, by device and the
    /// plugin's parameter id: "Filter · Cutoff". Filled each frame for the
    /// lanes of the tracks shown.
    pub names: BTreeMap<(Id, u32), String>,
    /// The lane each track shows, by track.
    pub shown: BTreeMap<Id, Target>,
    /// The points picked, on one lane.
    pub picks: Option<Picks>,
    /// The gesture under way.
    pub active: Option<Active>,
    /// The point under the pointer.
    pub hover: Option<Picked>,
    /// The tool in hand.
    pub tool: Tool,
    /// The shape tool last used, which the Shape button returns to.
    pub wave: Wave,
    /// The last lane press was in a lane: Ctrl+A, copy, paste and duplicate
    /// mean its points until a press somewhere else.
    pub focus: bool,
    /// The live value drawn beside the gesture this frame, for the tests.
    pub tip: Option<String>,
    /// What copy last took from a lane.
    pub clip: Option<LaneClip>,
    /// The playhead, which the controls whose lanes play follow (D5): set by
    /// the app every frame.
    pub at: lumen_core::time::Ticks,
    /// What each track does with the controls that move while it plays (M7
    /// slice A4b). Session state: a track not here is in Read, and a project
    /// always opens in Read.
    pub modes: BTreeMap<Id, crate::auto_record::Mode>,
    /// The lanes being written now, for the look of the controls and the chip.
    pub recording: std::collections::BTreeSet<(Id, Target)>,
}

impl AutoView {
    /// What `track` does with the controls that move while it plays.
    pub fn mode_of(&self, track: Id) -> crate::auto_record::Mode {
        self.modes.get(&track).copied().unwrap_or_default()
    }

    /// Sets what `track` does with the controls that move while it plays.
    pub fn set_mode(&mut self, track: Id, mode: crate::auto_record::Mode) {
        if mode == crate::auto_record::Mode::Read {
            self.modes.remove(&track);
        } else {
            self.modes.insert(track, mode);
        }
    }

    /// What `target` is called: the plugin's word for a parameter when it has
    /// said it, else the lane's own name.
    pub fn name_of(&self, project: &Project, target: Target) -> String {
        if let Target::Device { device, param } = target
            && let Some(name) = self.names.get(&(device, param))
        {
            return name.clone();
        }
        super::auto_scale::name(project, target)
    }

    /// Shows the lanes of `tracks`, or hides them when every one already
    /// shows one. A track opens on the first lane it has, else volume.
    pub fn toggle(&mut self, project: &Project, tracks: &[Id]) {
        let all = !tracks.is_empty() && tracks.iter().all(|t| self.shown.contains_key(t));
        for &id in tracks {
            if all {
                self.shown.remove(&id);
            } else if !self.shown.contains_key(&id)
                && let Some(track) = project.tracks.iter().find(|t| t.id == id)
            {
                let target = track
                    .automation
                    .first()
                    .map_or(Target::Volume, |l| l.target);
                self.shown.insert(id, target);
            }
        }
        if all {
            self.picks = None;
            self.focus = false;
        }
    }

    /// Forgets lanes of tracks that are gone, targets a track no longer
    /// has, and points that are not there any more.
    pub fn prune(&mut self, project: &Project) {
        self.shown.retain(|id, target| {
            project
                .tracks
                .iter()
                .find(|t| t.id == *id)
                .is_some_and(|t| t.has_target(*target))
        });
        let len_of = |track: Id, target: Target| {
            project
                .tracks
                .iter()
                .find(|t| t.id == track)
                .and_then(|t| t.lane(target))
                .map(|l| l.lane.len())
        };
        let shown = |track: Id, target: Target, shown: &BTreeMap<Id, Target>| {
            shown.get(&track) == Some(&target)
        };
        if let Some(picks) = &mut self.picks {
            let len = len_of(picks.track, picks.target)
                .filter(|_| shown(picks.track, picks.target, &self.shown));
            match len {
                Some(len) => picks.indices.retain(|&i| i < len),
                None => picks.indices.clear(),
            }
            if picks.indices.is_empty() {
                self.picks = None;
            }
        }
        let exists = |p: &Picked, shown_map: &BTreeMap<Id, Target>| {
            len_of(p.track, p.target).is_some_and(|len| p.index < len)
                && shown(p.track, p.target, shown_map)
        };
        if self.hover.as_ref().is_some_and(|p| !exists(p, &self.shown)) {
            self.hover = None;
        }
        if let Some(active) = &self.active
            && (!shown(active.track, active.target, &self.shown)
                || project.tracks.iter().all(|t| t.id != active.track))
        {
            self.active = None;
        }
    }

    /// The place of the point being moved on this lane, if one is.
    pub fn moving(&self, track: Id, target: Target) -> Option<usize> {
        match &self.active {
            Some(Active {
                track: t,
                target: g,
                how: Gesture::Move(m),
            }) if *t == track && *g == target => Some(m.anchor),
            _ => None,
        }
    }

    /// Takes `tool` in hand; a shape tool is also the one the Shape button
    /// returns to.
    pub fn use_tool(&mut self, tool: Tool) {
        if let Tool::Shape(wave) = tool {
            self.wave = wave;
        }
        self.tool = tool;
    }

    /// Whether the points of `track`'s `target` lane are picked.
    pub fn picked_on(&self, track: Id, target: Target) -> Option<&Picks> {
        self.picks.as_ref().filter(|p| p.on(track, target))
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::automation::Point;
    use lumen_project::automation::AutomationLane;
    use lumen_project::model::{Color, Track, TrackKind};

    use super::*;

    fn lane(points: &[(i64, f32)]) -> Lane {
        Lane::new(
            points
                .iter()
                .map(|&(q, v)| Point::new(Ticks::quarters(q), v))
                .collect(),
        )
        .unwrap()
    }

    fn project_with(lane_points: Option<Lane>) -> Project {
        let mut project = crate::empty_project();
        let mut track = Track::new(Id(7), "T".to_owned(), TrackKind::Audio, Color(0));
        if let Some(l) = lane_points {
            track
                .automation
                .push(AutomationLane::new(Target::Volume, l));
        }
        project.tracks.push(std::sync::Arc::new(track));
        project
    }

    #[test]
    fn toggling_shows_and_hides_lanes_and_opens_on_the_first_lane() {
        let mut project = crate::empty_project();
        let mut track = Track::new(Id(7), "T".to_owned(), TrackKind::Audio, Color(0));
        track
            .automation
            .push(AutomationLane::new(Target::Pan, lane(&[(0, 0.0)])));
        project.tracks.push(std::sync::Arc::new(track));
        let mut view = AutoView::default();
        view.toggle(&project, &[Id(7)]);
        assert_eq!(view.shown.get(&Id(7)), Some(&Target::Pan));
        view.toggle(&project, &[Id(7)]);
        assert!(view.shown.is_empty());
        // A track that is gone is not shown.
        view.toggle(&project, &[Id(99)]);
        assert!(view.shown.is_empty());
    }

    #[test]
    fn picks_and_lanes_are_forgotten_with_what_they_point_at() {
        let project = project_with(Some(lane(&[(0, 0.0), (4, 0.0)])));
        let mut view = AutoView::default();
        view.shown.insert(Id(7), Target::Volume);
        view.picks = Some(Picks {
            track: Id(7),
            target: Target::Volume,
            indices: BTreeSet::from([0, 1, 2]),
        });
        view.prune(&project);
        assert_eq!(
            view.picks.as_ref().map(Picks::list),
            Some(vec![0, 1]),
            "there is no third point"
        );
        view.picks = Some(Picks {
            indices: BTreeSet::from([5]),
            ..view.picks.take().unwrap()
        });
        view.prune(&project);
        assert!(view.picks.is_none(), "nothing left to pick");
        view.shown.insert(Id(7), Target::Send { to: Id(3) });
        view.prune(&project);
        assert!(view.shown.is_empty(), "the track makes no such send");
    }

    #[test]
    fn one_pick_is_single_and_two_are_not() {
        let here = Picked {
            track: Id(1),
            target: Target::Pan,
            index: 3,
        };
        let mut picks = Picks::only(here);
        assert_eq!(picks.single(), Some(here));
        assert!(picks.contains(here));
        picks.indices.insert(5);
        assert_eq!(picks.single(), None);
        assert!(!picks.contains(Picked { index: 4, ..here }));
        assert!(!picks.contains(Picked {
            target: Target::Volume,
            ..here
        }));
    }
}
