use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::edit::velocity;
use lumen_project::model::{Clip, ClipContent};

pub(crate) mod geometry;
pub(crate) mod gesture;
pub(crate) mod paint;
mod ui;

pub(in crate::note_editor) use ui::{LaneInput, reserved};

use geometry::LaneKind;
use gesture::{LaneGesture, LaneTool};
use paint::{BendHandle, LanePoint};

#[derive(Default)]
pub(crate) struct LaneState {
    pub hidden: bool,
    pub kind: LaneKind,
    pub tool: LaneTool,
    pub extra: Vec<u8>,
    pub gesture: Option<LaneGesture>,
    pub picked: Vec<usize>,
    picked_for: Option<Arc<Clip>>,
    pub bend: Option<BendHandle>,
    bend_for: Option<Arc<Clip>>,
    pub focus: bool,
    cache: Option<Cache>,
}

struct Cache {
    clip: Arc<Clip>,
    kind: LaneKind,
    stems: Vec<(Ticks, Vec<usize>)>,
    points: Vec<LanePoint>,
}

pub(crate) fn events_of(clip: &Clip) -> &[lumen_project::model::ClipEvent] {
    match &clip.content {
        ClipContent::Midi { events, .. } => events,
        _ => &[],
    }
}

impl LaneState {
    pub fn sync(&mut self, clip: &Arc<Clip>) {
        let same = |c: &Option<Arc<Clip>>| c.as_ref().is_some_and(|c| Arc::ptr_eq(c, clip));
        if !same(&self.picked_for) {
            self.picked.clear();
            self.picked_for = Some(clip.clone());
        }
        if !same(&self.bend_for) {
            self.bend = None;
            self.bend_for = Some(clip.clone());
        }
        let fresh = self
            .cache
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(&c.clip, clip) && c.kind == self.kind);
        if !fresh {
            let notes = super::notes_of(clip);
            let stems = if self.kind == LaneKind::Velocity {
                velocity::stems(notes)
            } else {
                Vec::new()
            };
            let points = match self.kind.lane() {
                Some(lane) => events_of(clip)
                    .iter()
                    .enumerate()
                    .filter_map(|(index, e)| {
                        lane.value_of(e.kind).map(|value| LanePoint {
                            at: e.at,
                            value,
                            index,
                        })
                    })
                    .collect(),
                None => Vec::new(),
            };
            self.cache = Some(Cache {
                clip: clip.clone(),
                kind: self.kind,
                stems,
                points,
            });
        }
    }

    pub fn rebind(&mut self, clip: &Arc<Clip>, picked: Vec<usize>, bend: Option<BendHandle>) {
        self.picked = picked;
        self.picked_for = Some(clip.clone());
        self.bend = bend;
        self.bend_for = Some(clip.clone());
    }

    pub fn choose(&mut self, kind: LaneKind) {
        self.kind = kind;
        self.gesture = None;
        self.bend = None;
        self.picked.clear();
    }

    pub fn use_tool(&mut self, tool: LaneTool) {
        self.tool = tool;
        self.bend = None;
    }

    pub fn cancel_gesture(&mut self) -> bool {
        self.gesture.take().is_some()
    }
}
