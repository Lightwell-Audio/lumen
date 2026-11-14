//! The note editor's header (M5 slice 5, the owner's approved mock-up; since
//! M9.5 S1 the clip editors' shared header, `app::clip_header`): the clip's
//! colour bar, name and "N bars · N notes"; the Pointer/Draw tools; the grid
//! chip; Quantize; and the Lane and Pop out icon buttons at the right.

use std::sync::Arc;

use eframe::egui::{Color32, Painter, Pos2, Rect, Ui};
use lumen_core::time::Ticks;
use lumen_project::model::Clip;

use super::{NoteEditor, glyphs, notes_of};
use crate::App;
use crate::app::clip_header::{HeaderStrip, icon_toggle};
use crate::commands::Command;
use crate::widgets::{self, raised};

/// What the strip is drawn for (the clip editor's header is shared by both
/// editors, so the facts travel together).
pub(super) struct Strip<'a> {
    pub clip: &'a Arc<Clip>,
    pub colour: Color32,
    /// The clip can be edited (it is not locked or frozen).
    pub editable: bool,
    /// The track reads a drum map: the Keys toggle applies.
    pub mapped: bool,
    /// The drum editor's view switch, on a track with a drum map (DV A).
    pub drum: Option<crate::drum_editor::View>,
}

impl App {
    /// The strip: colour bar, name and counts, tools, grid, Quantize, Lane,
    /// Keys (on a mapped track), Pop out.
    pub(super) fn editor_toolbar(
        &mut self,
        ui: &mut Ui,
        toolbar: Rect,
        ed: &mut NoteEditor,
        strip: Strip<'_>,
    ) {
        let Strip {
            clip,
            colour,
            editable,
            mapped,
            drum,
        } = strip;
        let right = |ui: &mut Ui| {
            let keys = self.keymap.keys(Command::PopOutEditor);
            let (name, tip, glyph): (&str, &str, fn(&Painter, Pos2, Color32)) = if ed.popped {
                (
                    "Dock",
                    "Put the editor back in the detail panel",
                    glyphs::dock,
                )
            } else {
                (
                    "Pop out",
                    "Open the editor in its own window",
                    glyphs::pop_out,
                )
            };
            let response = raised::icon_button(ui, name, glyph);
            if widgets::tip(response, tip, keys.as_ref(), None).clicked() {
                ed.popped = !ed.popped;
            }
            let mut shown = !ed.lane.hidden;
            let response = icon_toggle(ui, "Lane", shown, glyphs::lane);
            if widgets::tip(
                response,
                "Show the velocity and controller lane under the notes",
                self.keymap.keys(Command::ToggleLane).as_ref(),
                None,
            )
            .clicked()
            {
                shown = !shown;
            }
            ed.lane.hidden = !shown;
            if mapped && drum != Some(crate::drum_editor::View::Lanes) {
                let mut hiding = ed.hide_unmapped;
                let response = icon_toggle(ui, "Hide unmapped keys", hiding, glyphs::keys);
                if widgets::tip(
                    response,
                    "Show only the drum map's sounds in the key column",
                    self.keymap.keys(Command::HideUnmappedKeys).as_ref(),
                    None,
                )
                .clicked()
                {
                    hiding = !hiding;
                }
                ed.hide_unmapped = hiding;
            }
        };
        let mut strip = HeaderStrip::new(ui, toolbar, right);
        strip.title(&clip.name, &self.clip_counts(clip, editable), colour);
        // The drum editor's view switch (DV A): the lanes or the piano roll.
        if let Some(view) = drum
            && let Some(picked) = strip.views(view)
            && let Some(track) = self.track_of_clip(clip.id)
        {
            ed.drum.set_view(track, picked);
            ed.framed_for = None;
            // A gesture from the other view no longer means anything.
            ed.gesture = None;
            ed.drum.reorder = None;
        }
        let tips = [
            "Select, move and resize notes",
            "Click to add a note, drag to set its length, click a note to remove it",
        ];
        if let Some(tool) = strip.tools(self, ed.tool, tips) {
            ed.tool = tool;
        }
        self.header_grid_chip(
            &mut strip.row,
            ed.time,
            crate::app::clip_header::GridChip::Full,
        );
        if editable {
            self.quantize_button(&mut strip.row, ed, clip);
        }
    }

    /// "8 bars · 17 notes", with "· locked" when the clip cannot be edited.
    fn clip_counts(&self, clip: &Clip, editable: bool) -> String {
        let meter = &self.transport.timeline.meter;
        let first = meter.bbt_at(clip.start);
        let end = meter.bbt_at(clip.start + clip.length);
        let bars =
            (end.bar - first.bar + i64::from(end.beat > 0 || end.ticks > Ticks::ZERO)).max(1);
        let notes = notes_of(clip).len();
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        let mut text = format!(
            "{bars} bar{} \u{00B7} {notes} note{}",
            plural(bars as usize),
            plural(notes)
        );
        if !editable {
            text.push_str(" \u{00B7} locked");
        }
        text
    }
}
