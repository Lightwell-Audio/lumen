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

impl App {
    /// The strip: colour bar, name and counts, tools, grid, Quantize, Lane,
    /// Pop out.
    pub(super) fn editor_toolbar(
        &mut self,
        ui: &mut Ui,
        toolbar: Rect,
        ed: &mut NoteEditor,
        clip: &Arc<Clip>,
        colour: Color32,
        editable: bool,
    ) {
        let mut strip = HeaderStrip::new(ui, toolbar);
        strip.title(&clip.name, &self.clip_counts(clip, editable), colour);
        let tips = [
            "Select, move and resize notes",
            "Click to add a note, drag to set its length, click a note to remove it",
        ];
        if let Some(tool) = strip.tools(self, ed.tool, tips) {
            ed.tool = tool;
        }
        self.header_grid_chip(&mut strip.row, ed.time);
        if editable {
            self.quantize_button(&mut strip.row, ed, clip);
        }
        strip.right(|ui| {
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
        });
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
