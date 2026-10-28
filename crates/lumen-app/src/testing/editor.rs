//! Test seams for the clip editor (M5): the piano roll, its controller lane,
//! note edits and grid cells.

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::NoteEdit;
use crate::App;
use crate::timeview::TimeView;

impl App {
    /// Edits the notes of a clip as the piano roll will (M5 slice 3a's
    /// `Document` methods), for tests of notes committed while playing.
    pub fn edit_notes(&mut self, clip: Id, selection: &[usize], edit: NoteEdit) {
        use crate::document::Resize;
        use lumen_project::edit::notes::End;
        let base = self.document.project().clone();
        match edit {
            NoteEdit::Move { time, pitch } => {
                self.document
                    .move_notes(&base, clip, selection, time, pitch, None);
            }
            NoteEdit::Delete => {
                self.document.delete_notes(clip, selection);
            }
            NoteEdit::Mute(muted) => {
                self.document.mute_notes(clip, selection, Some(muted));
            }
            NoteEdit::Lengthen(by) => {
                let resize = Resize {
                    end: End::End,
                    by,
                    min: Ticks::ZERO,
                };
                self.document
                    .resize_notes(&base, clip, selection, resize, None);
            }
        }
    }

    /// The clip open in the editor.
    pub fn editor_open_clip(&self) -> Option<Id> {
        self.note_editor.clip
    }

    /// The draw tool is chosen (else the pointer).
    pub fn editor_draw_tool(&self) -> bool {
        self.note_editor.tool == crate::note_editor::Tool::Draw
    }

    /// The picked notes, as indices into the open clip's notes.
    pub fn editor_picked(&self) -> Vec<usize> {
        self.note_editor.picked().to_vec()
    }

    /// The editor is in its own window.
    pub fn editor_popped(&self) -> bool {
        self.note_editor.popped
    }

    /// A gesture is in progress (a button is down in the notes).
    pub fn editor_gesture(&self) -> bool {
        self.note_editor.gesture.is_some()
    }

    /// The detail panel shows the Clip tab.
    pub fn clip_tab_shown(&self) -> bool {
        self.detail_tab == crate::app::detail::DetailTab::Clip
    }

    /// The detail panel shows the Devices tab.
    pub fn devices_tab_shown(&self) -> bool {
        self.detail_tab == crate::app::detail::DetailTab::Devices
    }

    /// The detail panel shows the Mixer tab.
    pub fn mixer_tab_shown(&self) -> bool {
        self.detail_tab == crate::app::detail::DetailTab::Mixer
    }

    /// The track the detail panel is about (DP1).
    pub fn detail_subject(&self) -> Option<Id> {
        self.subject()
    }

    /// The clip open in the Clip tab, and whether it is an audio clip.
    pub fn opened_clip_in_detail(&self) -> Option<(Id, bool)> {
        self.opened_clip()
            .map(|id| (id, self.opened_audio() == Some(id)))
    }

    /// Where the notes are drawn, if the editor has been.
    pub fn editor_body(&self) -> Option<egui::Rect> {
        self.note_editor.last_view.map(|v| v.body)
    }

    /// The clip editor's own horizontal view (its zoom and scroll), where it
    /// drew last: separate from [`App::view`], the arrangement's (ADR-0048).
    pub fn editor_view(&self) -> Option<TimeView> {
        self.note_editor.last_view.map(|v| v.time)
    }

    /// The point at the start of clip time `local`, in the row of `pitch`
    /// (the row's middle), where the editor drew it last.
    pub fn editor_point(&self, local: Ticks, pitch: u8) -> Option<egui::Pos2> {
        let v = self.note_editor.last_view?;
        let y = v.row_top(i32::from(pitch)) + v.row_height / 2.0;
        Some(egui::pos2(v.x_of(local), y))
    }

    /// The lane under the roll is showing (it was drawn last frame).
    pub fn lane_shown(&self) -> bool {
        self.note_editor.last_lane.is_some() && !self.note_editor.lane.hidden
    }

    /// The name of the lane's tab chosen ("Velocity", "Mod wheel", ...).
    pub fn lane_name(&self) -> String {
        self.note_editor.lane.kind.name()
    }

    /// The lane's plotting area, where it drew last.
    pub fn lane_body(&self) -> Option<egui::Rect> {
        self.note_editor.last_lane.map(|v| v.body)
    }

    /// The point where the open lane shows `value` at clip time `local`.
    pub fn lane_point(&self, local: Ticks, value: i32) -> Option<egui::Pos2> {
        let v = self.note_editor.last_lane?;
        Some(egui::pos2(v.x_of(local), v.y_of(value)))
    }

    /// Where the bend handle's dot is, when a curve was just drawn.
    pub fn lane_bend_dot(&self) -> Option<egui::Pos2> {
        let v = self.note_editor.last_lane?;
        let b = self.note_editor.lane.bend?;
        Some(crate::note_editor::lane::paint::bend_dot(&v, &b))
    }

    /// A gesture is in progress in the lane.
    pub fn lane_gesture(&self) -> bool {
        self.note_editor.lane.gesture.is_some()
    }

    /// The picked points of the controller lane (indices into the clip's
    /// events).
    pub fn lane_picked(&self) -> Vec<usize> {
        self.note_editor.lane.picked.clone()
    }

    /// Makes `pitch` visible in the editor, as scrolling would.
    pub fn editor_show_pitch(&mut self, pitch: u8) {
        if let Some(v) = self.note_editor.last_view {
            self.note_editor.scroll = crate::note_editor::view::scroll_to_centre(
                i32::from(pitch),
                v.row_height,
                v.body.height(),
            );
        }
    }

    /// The grid step that `t` falls in now (`[start, end)`).
    pub fn grid_cell_at(&self, t: Ticks) -> (Ticks, Ticks) {
        self.grid_cell(t)
    }

    /// [`Self::grid_cell_at`] as the piano roll's own zoom sets it.
    pub fn editor_grid_cell_at(&self, t: Ticks) -> Option<(Ticks, Ticks)> {
        let zoom = self.note_editor.last_view?.time.zoom;
        Some(self.grid_cell_at_zoom(t, zoom))
    }
}

/// The piano roll's body drawn on its own with a sample clip (slice 3b),
/// for the shots harness: the real roll arrives with slice 3c.
pub struct PianoRollDemo {
    boxes: std::sync::Arc<Vec<lumen_render::BoxInstance>>,
}

impl PianoRollDemo {
    /// The sample clip in track colour `colour` (an index of
    /// [`clip_colour`]'s palette).
    pub fn new(colour: usize) -> Self {
        Self {
            boxes: std::sync::Arc::new(crate::piano_roll::demo::sample_boxes(
                crate::theme::CLIP_COLORS[colour],
            )),
        }
    }

    /// Installs the typefaces and the theme (`light` or dark), as the app
    /// does at start, in a context that has no `App`.
    pub fn prepare(ctx: &egui::Context, light: bool) {
        crate::fonts::install(ctx);
        let appearance = if light {
            crate::theme::Appearance::Light
        } else {
            crate::theme::Appearance::Dark
        };
        crate::theme::apply(ctx, appearance, 0.0);
    }

    /// Draws the roll over all of `ui`'s free area.
    pub fn show(&self, ui: &mut egui::Ui, points_per_beat: f64, row_height: f32) {
        crate::piano_roll::demo::show(ui, &self.boxes, points_per_beat, row_height);
    }
}
