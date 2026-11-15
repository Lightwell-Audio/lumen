use std::collections::BTreeMap;
use std::collections::BTreeSet;

use eframe::egui::Pos2;
use lumen_core::automation::Lane;
use lumen_core::curve::Handle;
use lumen_core::time::Ticks;
use lumen_project::automation::Target;
use lumen_project::edit::automation::{Copied, Wave};
use lumen_project::model::{Id, Project};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Picked {
    pub track: Id,
    pub target: Target,
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Picks {
    pub track: Id,
    pub target: Target,
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

    pub fn list(&self) -> Vec<usize> {
        self.indices.iter().copied().collect()
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tool {
    #[default]
    Pointer,
    Pencil,
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

#[derive(Debug, Clone)]
pub(crate) struct Moving {
    pub base: Lane,
    pub picks: Vec<usize>,
    pub anchor: usize,
    pub anchor_at: Ticks,
    pub anchor_height: f32,
    pub press_at: Ticks,
    pub press_height: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct Boxing {
    pub from: Pos2,
    pub to: Pos2,
}

#[derive(Debug, Clone)]
pub(crate) struct Drawing {
    pub base: Lane,
    pub raw: Vec<(Ticks, f32)>,
}

#[derive(Debug, Clone)]
pub(crate) struct Waving {
    pub base: Lane,
    pub kind: Wave,
    pub from: Ticks,
    pub start: f32,
    pub end: f32,
    pub to: Ticks,
    pub period: Ticks,
}

#[derive(Debug, Clone)]
pub(crate) struct Bending {
    pub base: Lane,
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

#[derive(Debug, Clone)]
pub(crate) struct Active {
    pub track: Id,
    pub target: Target,
    pub how: Gesture,
}

#[derive(Debug, Clone)]
pub(crate) struct LaneClip {
    pub copied: Copied,
    pub range: (f32, f32),
}

#[derive(Debug, Clone)]
pub(crate) struct ParamChooser {
    pub track: Id,
    pub device: Id,
    pub search: String,
    pub group: Option<i32>,
    pub focus: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapRequest {
    Learn(Id, Target),
    StopLearning,
    Clear(Id, Target),
    Window,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Hands {
    pub active: Option<Active>,
    pub picks: Option<Picks>,
    pub tool: Tool,
    pub focus: bool,
}

impl Hands {
    pub fn toggle_pick(&mut self, here: Picked) {
        match &mut self.picks {
            Some(picks) if picks.on(here.track, here.target) => {
                if !picks.indices.remove(&here.index) {
                    picks.indices.insert(here.index);
                }
                if picks.indices.is_empty() {
                    self.picks = None;
                }
            }
            _ => self.picks = Some(Picks::only(here)),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct AutoView {
    pub learning: Option<(Id, Target)>,
    pub requests: Vec<MapRequest>,
    pub chooser: Option<ParamChooser>,
    pub names: BTreeMap<(Id, u32), String>,
    pub texts: BTreeMap<(Id, u32), (f64, String)>,
    pub text_asked: Option<((Id, u32), f64, std::time::Instant)>,
    pub texts_seen: u64,
    pub shown: BTreeMap<Id, Target>,
    pub picks: Option<Picks>,
    pub active: Option<Active>,
    pub hover: Option<Picked>,
    pub tool: Tool,
    pub wave: Wave,
    pub focus: bool,
    pub tip: Option<String>,
    pub clip: Option<LaneClip>,
    pub at: lumen_core::time::Ticks,
    pub modes: BTreeMap<Id, crate::recording::auto_record::Mode>,
    pub recording: std::collections::BTreeSet<(Id, Target)>,
}

impl AutoView {
    pub fn lend(&mut self) -> Hands {
        Hands {
            active: self.active.take(),
            picks: self.picks.take(),
            tool: self.tool,
            focus: self.focus,
        }
    }

    pub fn restore(&mut self, hands: Hands) {
        self.active = hands.active;
        self.picks = hands.picks;
        self.tool = hands.tool;
        self.focus = hands.focus;
    }

    pub fn mode_of(&self, track: Id) -> crate::recording::auto_record::Mode {
        self.modes.get(&track).copied().unwrap_or_default()
    }

    pub fn set_mode(&mut self, track: Id, mode: crate::recording::auto_record::Mode) {
        if mode == crate::recording::auto_record::Mode::Read {
            self.modes.remove(&track);
        } else {
            self.modes.insert(track, mode);
        }
    }

    pub fn name_of(&self, project: &Project, target: Target) -> String {
        if let Target::Device { device, param } = target
            && let Some(name) = self.names.get(&(device, param))
        {
            return name.clone();
        }
        super::auto_scale::name(project, target)
    }

    pub fn value_text(&self, target: Target, value: f32) -> String {
        if let Target::Device { device, param } = target
            && let Some((_, text)) = self.texts.get(&(device, param))
        {
            return text.clone();
        }
        super::auto_scale::text(target, value)
    }

    pub fn resting(&self) -> Option<Picked> {
        let moving = self.active.as_ref().and_then(|a| match &a.how {
            Gesture::Move(m) => Some(Picked {
                track: a.track,
                target: a.target,
                index: m.anchor,
            }),
            _ => None,
        });
        moving
            .or(self.hover)
            .or_else(|| self.picks.as_ref().and_then(Picks::single))
            .filter(|p| matches!(p.target, Target::Device { .. }))
    }

    pub fn dims(&self, track: Id) -> bool {
        self.shown.get(&track).is_some_and(|t| t.clip().is_none())
    }

    pub fn show(&mut self, track: Id, target: Target) {
        if self.shown.insert(track, target) != Some(target) {
            self.picks = None;
        }
    }

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
                .and_then(|t| t.lane_view(target))
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

    pub fn use_tool(&mut self, tool: Tool) {
        if let Tool::Shape(wave) = tool {
            self.wave = wave;
        }
        self.tool = tool;
    }

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
