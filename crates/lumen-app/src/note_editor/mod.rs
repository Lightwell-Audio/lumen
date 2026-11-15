pub(crate) mod gesture;
pub(crate) mod glyphs;
pub(crate) mod lane;
mod ops;
pub(crate) mod paint;
mod pointer;
pub(crate) mod quantize;
mod toolbar;
pub(crate) mod ui;
pub(crate) mod view;
pub(crate) mod window;

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::edit::NoteSelection;
use lumen_project::model::{Clip, ClipContent, Id, Note, TrackKind};

use crate::app::detail::DetailTab;
use crate::app::panes::{self, Pane};
use crate::arrangement::select::TimeRange;
use crate::timeview::TimeView;
use crate::{App, document::NewClip};

pub(crate) use ops::{NoteOp, VELOCITY_STEP};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tool {
    #[default]
    Pointer,
    Draw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MenuFor {
    #[default]
    Empty,
    Notes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorZoom {
    In,
    Out,
}

#[derive(Default)]
pub(crate) struct NoteEditor {
    pub clip: Option<Id>,
    pub tool: Tool,
    pub selection: Option<NoteSelection>,
    pub gesture: Option<gesture::Gesture>,
    pub lane: lane::LaneState,
    pub quantize: quantize::QuantizeState,
    pub last_lane: Option<lane::geometry::LaneView>,
    pub popped: bool,
    pub time: TimeView,
    pub zooms: Vec<EditorZoom>,
    pub row_height: f32,
    pub scroll: f32,
    pub framed_for: Option<Id>,
    pub hovered: bool,
    pub hovered_before: bool,
    pub pointer_at: Option<(Ticks, i32)>,
    pub window_focused: bool,
    pub window_opened_at: Option<crate::settings::WindowPlace>,
    pub live: bool,
    pub menu_for: MenuFor,
    pub hide_unmapped: bool,
    pub drum: crate::drum_editor::State,
    pub menu_at: Option<Ticks>,
    pub boxed: Vec<usize>,
    pub last_empty_press: Option<(f64, egui::Pos2)>,
    boxes: Option<NoteBoxes>,
    generation: u64,
    pub last_view: Option<view::EditorView>,
}

struct NoteBoxes {
    clip: std::sync::Arc<Clip>,
    picked: Vec<usize>,
    colour: egui::Color32,
    rows: view::Rows,
    boxes: std::sync::Arc<Vec<lumen_render::BoxInstance>>,
    generation: u64,
}

impl NoteEditor {
    pub fn boxes_for(
        &mut self,
        clip: &std::sync::Arc<Clip>,
        colour: egui::Color32,
        rows: view::Rows,
    ) -> (std::sync::Arc<Vec<lumen_render::BoxInstance>>, u64) {
        let picked = self.picked().to_vec();
        let fresh = self.boxes.as_ref().is_some_and(|b| {
            std::sync::Arc::ptr_eq(&b.clip, clip)
                && b.picked == picked
                && b.colour == colour
                && b.rows == rows
        });
        if !fresh {
            let selected: std::collections::BTreeSet<usize> = picked.iter().copied().collect();
            let notes = crate::piano_roll::notes::ClipNotes {
                start: clip.start,
                length: clip.length,
                loop_length: clip.loop_length,
                loop_phase: clip.loop_phase,
                notes: notes_of(clip),
                carried: &[],
                outside: true,
            };
            let boxes: Vec<lumen_render::BoxInstance> = crate::piano_roll::notes::note_instances(
                &notes,
                &selected,
                &crate::piano_roll::notes::NoteColors::new(colour),
            )
            .into_iter()
            .filter_map(|mut instance| {
                let row = rows.row_of(instance.row as i32)?;
                instance.row = (crate::note_editor::view::PITCHES as u32) - 1 - row as u32;
                Some(instance)
            })
            .collect();
            self.generation += 1;
            self.boxes = Some(NoteBoxes {
                clip: clip.clone(),
                picked,
                colour,
                rows,
                boxes: std::sync::Arc::new(boxes),
                generation: self.generation,
            });
        }
        let b = self.boxes.as_ref().expect("just built");
        (b.boxes.clone(), b.generation)
    }
}

impl NoteEditor {
    pub fn picked(&self) -> &[usize] {
        self.selection.as_ref().map_or(&[], |s| s.indices())
    }

    pub fn cancel_gesture(&mut self) -> bool {
        let notes = self.gesture.take().is_some();
        let lane = self.lane.cancel_gesture();
        let reorder = self.drum.reorder.take().is_some();
        notes || lane || reorder
    }
}

pub(crate) fn notes_of(clip: &Clip) -> &[Note] {
    match &clip.content {
        ClipContent::Midi { notes, .. } => notes,
        _ => &[],
    }
}

impl App {
    pub(crate) fn editor_clip(&self, id: Id) -> Option<(std::sync::Arc<Clip>, egui::Color32)> {
        self.document.project().tracks.iter().find_map(|t| {
            t.clips
                .iter()
                .find(|c| c.id == id && matches!(c.content, ClipContent::Midi { .. }))
                .map(|c| {
                    (
                        c.clone(),
                        crate::theme::wave(crate::arrangement::clip_color(t, c)),
                    )
                })
        })
    }

    pub(crate) fn clip_drum_map(&self, clip: Id) -> Option<&lumen_project::model::DrumMap> {
        let project = self.document.project();
        let track = project
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip))?;
        project.drum_map_of(track)
    }

    pub(crate) fn clip_key_sounds(
        &self,
        clip: Id,
    ) -> Option<Vec<crate::piano_roll::draw::KeySound>> {
        let map = self.clip_drum_map(clip)?;
        Some(
            map.sounds
                .iter()
                .map(|sound| crate::piano_roll::draw::KeySound {
                    input: sound.input,
                    name: sound.name.clone(),
                    icon: sound.icon.into(),
                    color: crate::theme::wave(crate::arrangement::color_to(sound.color)),
                    silent: !map.heard(sound),
                })
                .collect(),
        )
    }

    pub(crate) fn editor_has_drum_map(&self) -> bool {
        self.note_editor
            .clip
            .is_some_and(|clip| self.clip_drum_map(clip).is_some())
    }

    pub(crate) fn toggle_hide_unmapped_keys(&mut self) {
        if !self.editor_has_drum_map() {
            return;
        }
        self.note_editor.hide_unmapped = !self.note_editor.hide_unmapped;
        self.note_editor.framed_for = None;
    }

    pub(crate) fn open_note_editor(&mut self, clip: Id) {
        if self.editor_clip(clip).is_none() {
            return;
        }
        let same = self.note_editor.clip == Some(clip);
        if !same {
            self.end_audition();
        }
        let project = self.document.project();
        self.note_editor.selection = NoteSelection::in_clip(project, clip);
        if !same {
            self.note_editor.gesture = None;
            self.note_editor.framed_for = None;
            self.note_editor.drum.steps.gesture = None;
            self.note_editor.drum.steps.cursor = None;
            // A drum clip opens in Steps when it fits the grid, else in Lanes
            // (D3), unless the track's view was chosen.
            self.pick_drum_view_for(clip);
        }
        self.note_editor.clip = Some(clip);
        self.opened_in_note_editor(clip);
        self.detail_tab = DetailTab::Clip;
        self.show_detail = true;
        self.arrangement.tool_keys = false;
        self.nav
            .steps
            .push(crate::app::navigate::ViewStep::ShowClip(clip));
    }

    pub(crate) fn close_note_editor(&mut self) {
        self.end_audition();
        let ed = &mut self.note_editor;
        ed.clip = None;
        ed.selection = None;
        ed.gesture = None;
        ed.drum.reorder = None;
        ed.drum.steps.gesture = None;
        ed.popped = false;
    }

    pub(crate) fn refresh_note_editor(&mut self, ctx: &egui::Context) {
        if let Some(clip) = self.note_editor.clip {
            if self.editor_clip(clip).is_none() {
                self.close_note_editor();
            } else {
                let project = self.document.project();
                if let Some(selection) = &mut self.note_editor.selection {
                    selection.refresh(project);
                }
                if let Some(gesture) = &self.note_editor.gesture {
                    let fresh = self.editor_clip(clip).map(|(c, _)| c);
                    let same = |c: &std::sync::Arc<Clip>| {
                        std::sync::Arc::ptr_eq(c, &gesture.clip)
                            || notes_of(c) == notes_of(&gesture.clip)
                    };
                    if !fresh.as_ref().is_some_and(same) {
                        self.note_editor.gesture = None;
                    }
                }
            }
        }
        let ed = &self.note_editor;
        let docked_focus = ed.clip.is_some()
            && !ed.popped
            && self.detail_tab == DetailTab::Clip
            && self.show_detail
            && panes::focused(ctx) == Some(Pane::Detail);
        self.note_editor.live = self.note_editor.clip.is_some()
            && (self.note_editor.hovered || docked_focus || self.note_editor.window_focused);
        self.note_editor.hovered_before = self.note_editor.hovered;
        self.note_editor.hovered = false;
    }

    pub(crate) fn note_keys_picked(&self) -> bool {
        self.note_editor.live && !self.note_editor.picked().is_empty()
    }

    pub(crate) fn note_keys(&self) -> bool {
        self.note_editor.live
    }

    pub(crate) fn notes_are_picked(&self) -> bool {
        self.note_editor.clip.is_some() && !self.note_editor.picked().is_empty()
    }

    pub(crate) fn midi_track_for_new_clip(&self) -> Option<Id> {
        let tracks = &self.document.project().tracks;
        let is_midi = |id: &Id| {
            tracks
                .iter()
                .any(|t| t.id == *id && t.kind == TrackKind::Midi)
        };
        if let Some(track) = self.selected_midi_tracks().first().copied() {
            return Some(track);
        }
        if let Some(track) = self
            .arrangement
            .range
            .as_ref()
            .and_then(|range| range.tracks.iter().copied().find(is_midi))
        {
            return Some(track);
        }
        self.arrangement.insert.filter(is_midi)
    }

    pub(crate) fn create_midi_clip_here(&mut self) {
        let Some(track) = self.midi_track_for_new_clip() else {
            return;
        };
        let place = match &self.arrangement.range {
            Some(TimeRange { start, end, .. }) => NewClip::Range(*start, *end),
            None => NewClip::Bar(self.transport.position()),
        };
        self.put_midi_clip(track, place);
    }

    pub(crate) fn put_midi_clip(&mut self, track: Id, place: NewClip) {
        if let Some(clip) = self.document.create_midi_clip(track, place) {
            self.arrangement.clips = [clip].into_iter().collect();
            self.arrangement.range = None;
            self.open_note_editor(clip);
        }
    }
}
