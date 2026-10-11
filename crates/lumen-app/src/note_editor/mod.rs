//! The clip editor for MIDI notes (M5 slice 3c; owner's C2, Look A, Draw A,
//! Live A). It lives in the detail panel's Clip tab and can pop out into
//! its own window; both views draw the same [`NoteEditor`] state.
//!
//! - `view`: the geometry (the editor's own time axis and pitch scroll);
//! - `gesture`: the mouse gestures as pure arithmetic, with the preview;
//! - `paint`: the drawing boundary (slice 3b's GPU renderer plugs in here);
//! - `ui`: the toolbar, the input and the menu;
//! - `ops`: the note edits as commands and menus run them;
//! - `window`: the pop-out.
//!
//! The document changes once per gesture, on release, as one undo step;
//! the notes follow the pointer meanwhile as a preview. Escape cancels.

pub(crate) mod gesture;
pub(crate) mod lane;
mod ops;
pub(crate) mod paint;
pub(crate) mod ui;
pub(crate) mod view;
mod window;

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::edit::NoteSelection;
use lumen_project::model::{Clip, ClipContent, Id, Note, TrackKind};

use crate::app::detail::DetailTab;
use crate::app::panes::{self, Pane};
use crate::arrangement::select::TimeRange;
use crate::timeview::TimeView;
use crate::{App, document::NewClip};

pub(crate) use ops::NoteOp;

/// What a click or drag in the notes does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tool {
    #[default]
    Pointer,
    Draw,
}

/// What the right-click menu is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum MenuFor {
    #[default]
    Empty,
    Notes,
}

/// A zoom key pressed while the editor has the keyboard, waiting for the
/// editor to draw (it knows its own width and the pointer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorZoom {
    In,
    Out,
}

/// The editor's state, shared by the panel and the window.
#[derive(Default)]
pub(crate) struct NoteEditor {
    /// The clip being edited.
    pub clip: Option<Id>,
    pub tool: Tool,
    /// The picked notes; follows the editor's own edits and clears when the
    /// notes change under it (undo, redo).
    pub selection: Option<NoteSelection>,
    pub gesture: Option<gesture::Gesture>,
    /// The editor is in its own window.
    pub popped: bool,
    /// The editor's own horizontal view: its zoom and scroll, independent of
    /// the arrangement (owner decision, ADR-0048). Fitted to each clip when
    /// it opens (`framed_for`).
    pub time: TimeView,
    /// Zoom keys waiting for the next frame.
    pub zooms: Vec<EditorZoom>,
    /// Points a pitch is tall (0 until first drawn).
    pub row_height: f32,
    /// Points scrolled down from the top of pitch 127.
    pub scroll: f32,
    /// The clip the view was last framed for: fitted to its span and
    /// centred vertically when it first draws.
    pub framed_for: Option<Id>,
    /// Written while drawing: the pointer is over the editor, and the
    /// clip time and pitch it is at.
    pub hovered: bool,
    /// What `hovered` was when the frame began (the drawing resets it).
    pub hovered_before: bool,
    pub pointer_at: Option<(Ticks, i32)>,
    /// The pop-out window has the keyboard.
    pub window_focused: bool,
    /// The editor's keys are live: set at the start of each frame.
    pub live: bool,
    pub menu_for: MenuFor,
    /// The clip time under the pointer when the menu was opened: "Split
    /// here" cuts there. Latched, because while the menu is used the pointer
    /// is over the menu, not the roll, so `pointer_at` is empty.
    pub menu_at: Option<Ticks>,
    /// Notes a marquee in progress would pick.
    pub boxed: Vec<usize>,
    /// The roll's note boxes, kept until the clip or the picks change.
    boxes: Option<NoteBoxes>,
    generation: u64,
    /// Where the notes were drawn last (tests).
    pub last_view: Option<view::EditorView>,
}

/// The GPU boxes of the open clip's notes and what they were built from.
struct NoteBoxes {
    clip: std::sync::Arc<Clip>,
    picked: Vec<usize>,
    colour: egui::Color32,
    boxes: std::sync::Arc<Vec<lumen_render::BoxInstance>>,
    generation: u64,
}

impl NoteEditor {
    /// The boxes for `clip` in `colour` with the picked notes outlined,
    /// rebuilt only when the clip, the picks or the colour changed.
    pub fn boxes_for(
        &mut self,
        clip: &std::sync::Arc<Clip>,
        colour: egui::Color32,
    ) -> (std::sync::Arc<Vec<lumen_render::BoxInstance>>, u64) {
        let picked = self.picked().to_vec();
        let fresh = self.boxes.as_ref().is_some_and(|b| {
            std::sync::Arc::ptr_eq(&b.clip, clip) && b.picked == picked && b.colour == colour
        });
        if !fresh {
            let selected: std::collections::BTreeSet<usize> = picked.iter().copied().collect();
            let notes = crate::piano_roll::notes::ClipNotes {
                start: clip.start,
                length: clip.length,
                loop_length: clip.loop_length,
                notes: notes_of(clip),
            };
            let boxes = crate::piano_roll::notes::note_instances(
                &notes,
                &selected,
                &crate::piano_roll::notes::NoteColors::new(colour),
            );
            self.generation += 1;
            self.boxes = Some(NoteBoxes {
                clip: clip.clone(),
                picked,
                colour,
                boxes: std::sync::Arc::new(boxes),
                generation: self.generation,
            });
        }
        let b = self.boxes.as_ref().expect("just built");
        (b.boxes.clone(), b.generation)
    }
}

impl NoteEditor {
    /// The picked indices.
    pub fn picked(&self) -> &[usize] {
        self.selection.as_ref().map_or(&[], |s| s.indices())
    }

    /// Ends a gesture without changing anything. True if one was running.
    pub fn cancel_gesture(&mut self) -> bool {
        self.gesture.take().is_some()
    }
}

/// The notes of a MIDI clip.
pub(crate) fn notes_of(clip: &Clip) -> &[Note] {
    match &clip.content {
        ClipContent::Midi { notes, .. } => notes,
        _ => &[],
    }
}

impl App {
    /// The open clip and its colour, if it is still a MIDI clip.
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

    /// Opens MIDI clip `clip` in the Clip tab (showing the detail panel),
    /// and brings it into view in the arrangement. Not a MIDI clip: nothing.
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
        }
        self.note_editor.clip = Some(clip);
        self.detail_tab = DetailTab::Clip;
        self.show_detail = true;
        self.nav
            .steps
            .push(crate::app::navigate::ViewStep::ShowClip(clip));
    }

    /// Stops editing (the clip is gone).
    pub(crate) fn close_note_editor(&mut self) {
        self.end_audition();
        let ed = &mut self.note_editor;
        ed.clip = None;
        ed.selection = None;
        ed.gesture = None;
        ed.popped = false;
    }

    /// Once a frame, before the keys: the open clip is still there, the
    /// selection is current, and the editor knows whether its keys are live.
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
                    // Undo, or any change to the notes, ends a gesture.
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

    /// The editor's keys are live and notes are picked: the arrows, Delete,
    /// Ctrl+D, 0 and Esc mean notes.
    pub(crate) fn note_keys_picked(&self) -> bool {
        self.note_editor.live && !self.note_editor.picked().is_empty()
    }

    /// The editor's keys are live (Ctrl+A selects its notes).
    pub(crate) fn note_keys(&self) -> bool {
        self.note_editor.live
    }

    /// A MIDI clip is open and has notes picked (commands from the palette
    /// and menus work without the pointer in the editor).
    pub(crate) fn notes_are_picked(&self) -> bool {
        self.note_editor.clip.is_some() && !self.note_editor.picked().is_empty()
    }

    /// The first selected arrangement clip, if it is a MIDI clip.
    pub(crate) fn selected_midi_clip(&self) -> Option<Id> {
        let id = self.first_selected_clip()?;
        self.editor_clip(id).map(|_| id)
    }

    /// The MIDI track Create MIDI clip puts a clip on: the first selected
    /// MIDI track, else the first MIDI track of the time range, else the
    /// track last clicked on the lanes (ADR-0049).
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

    /// Create MIDI clip: over the time range if there is one, else one bar
    /// at the edit position (the playhead). Opens the new clip.
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

    /// Makes a MIDI clip, selects it and opens it in the editor.
    pub(crate) fn put_midi_clip(&mut self, track: Id, place: NewClip) {
        if let Some(clip) = self.document.create_midi_clip(track, place) {
            self.arrangement.clips = [clip].into_iter().collect();
            self.arrangement.range = None;
            self.open_note_editor(clip);
        }
    }
}
