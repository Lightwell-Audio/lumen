//! A sample clip and a way to draw the piano roll body on its own, for the
//! shots harness and the frame benchmark (the real roll arrives in slice
//! 3c, which calls [`draw::roll`] the same way).

use std::collections::BTreeSet;
use std::sync::Arc;

use eframe::egui::{self, Rect};
use lumen_core::grid::Grid;
use lumen_core::meter::{Meter, MeterMap};
use lumen_core::time::Ticks;
use lumen_project::model::Note;
use lumen_render::BoxInstance;

use super::draw::{self, RollView};
use super::notes::{ClipNotes, NoteColors, note_instances};
use crate::canvas::{GridLook, Surface};
use crate::theme;
use crate::timeview::TimeView;

/// Two bars of chords and a melody, looped over eight bars; some notes
/// selected, one muted, velocities varying.
pub(crate) fn sample_boxes(color: egui::Color32) -> Vec<BoxInstance> {
    let q = Ticks::quarters;
    let mut notes = Vec::new();
    for (i, root) in [48_u8, 53, 55, 50].into_iter().enumerate() {
        for (j, step) in [0_u8, 4, 7, 12].into_iter().enumerate() {
            notes.push(Note::new(
                q(i as i64 * 2),
                q(2),
                root + step,
                70 + (j as u8) * 15,
            ));
        }
    }
    for (i, p) in [72_u8, 74, 76, 77, 79, 77, 76, 74].into_iter().enumerate() {
        notes.push(Note::new(
            q(i as i64) + Ticks(i as i64 * 1000),
            Ticks(lumen_core::time::PPQ / 2),
            p,
            40 + i as u8 * 12,
        ));
    }
    notes[5].4 = true;
    let selected: BTreeSet<usize> = [1, 2, 3, 17].into();
    note_instances(
        &ClipNotes {
            start: q(0),
            length: q(32),
            loop_length: Some(q(8)),
            notes: &notes,
        },
        &selected,
        &NoteColors::new(color),
    )
}

/// Draws the sample roll in `ui`'s whole area: key column, lanes, grid and
/// notes, at `zoom` points a beat and `row_height` points a pitch.
pub(crate) fn show(ui: &mut egui::Ui, boxes: &Arc<Vec<BoxInstance>>, zoom: f64, row_height: f32) {
    let rect: Rect = ui.available_rect_before_wrap();
    let p = theme::p();
    let grid = GridLook {
        grid: Grid::default(),
        meter: MeterMap::constant(Meter::FOUR_FOUR).expect("4/4 is a meter"),
        colors: [p.grid_bar, p.grid_beat, p.grid_sub],
    };
    let view = RollView {
        time: TimeView { left: 0.0, zoom },
        // The top of pitch 84 at the top edge.
        scroll: ((127.0 - 84.0) * row_height)
            .min(draw::content_height(row_height) - rect.height())
            .max(0.0),
        row_height,
    };
    draw::roll(
        ui.painter(),
        Surface::Docked,
        rect,
        56.0,
        view,
        &grid,
        draw::RollNotes {
            boxes: Arc::clone(boxes),
            generation: 1,
            under: |_: &eframe::egui::Painter| {},
        },
    );
}
