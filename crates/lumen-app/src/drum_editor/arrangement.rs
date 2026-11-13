//! A mapped MIDI clip on the arrangement drawn as drum rows (M8 D2's DV A,
//! the owner's choice of B's look): one row per sound the clip plays, in the
//! map's order, each hit a small cell filled from the row's foot to its
//! velocity, in place of the mini piano roll. It is a picture only: editing is
//! in Lanes, and the arrangement's own clip gestures are unchanged.
//!
//! Rows are the clip body divided among the sounds, so a short clip thins
//! them: below [`MIN_ROW`] a row stops being a picture, and the rows drawn are
//! the sounds the clip plays most (in the map's order between them). A clip
//! too short for even one row draws none.
//!
//! Only the hits in view are gathered, one per picture column, as the lanes
//! gather theirs: a clip of a hundred thousand notes costs what its picture
//! costs.

use std::sync::Arc;

use eframe::egui::{Painter, Rect, Stroke, pos2};
use lumen_project::model::{Clip, ClipContent, DrumMap, Note};

use crate::piano_roll::notes::{ClipNotes, Placed};
use crate::theme;
use crate::timeview::TimeView;

/// The shortest a row is drawn: below this it stops being a picture.
pub(crate) const MIN_ROW: f32 = 6.0;
/// The most cells one clip's picture draws in a frame; a deeper zoom gathers
/// more hits per cell rather than drawing them all.
const MAX_CELLS: usize = 4000;
/// The narrowest picture column one hit is gathered into (points).
const BUCKET: f32 = 2.0;

/// The rows a clip's picture draws: the sounds it plays, in the map's order,
/// or the most used ones when they do not fit. Each is the sound's index in
/// the map.
pub(crate) fn rows(map: &DrumMap, notes: &[Note], room: f32) -> Vec<usize> {
    let mut counts = [0_u32; 128];
    for note in notes {
        if let Some(slot) = counts.get_mut(usize::from(note.pitch())) {
            *slot = slot.saturating_add(1);
        }
    }
    let used: Vec<usize> = map
        .sounds
        .iter()
        .enumerate()
        .filter(|(_, sound)| counts.get(usize::from(sound.input)).copied().unwrap_or(0) > 0)
        .map(|(index, _)| index)
        .collect();
    let fits = ((room / MIN_ROW).floor().max(0.0) as usize).min(used.len());
    if fits == 0 {
        return Vec::new();
    }
    if used.len() <= fits {
        return used;
    }
    // Too many for the room: the sounds played most keep their rows, ties in
    // the map's order (the sort is stable).
    let mut ranked = used;
    ranked.sort_by_key(|&index| {
        std::cmp::Reverse(
            counts
                .get(usize::from(map.sounds[index].input))
                .copied()
                .unwrap_or(0),
        )
    });
    ranked.truncate(fits);
    ranked.sort_unstable();
    ranked
}

/// One cell of a picture: its row (an index into `rows`), the leftmost onset
/// in its picture column, its velocity and whether it is muted.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cell {
    row: usize,
    x: f32,
    velocity: u8,
    muted: bool,
}

/// The picture columns a clip gets: `BUCKET` widened until the cells fit
/// [`MAX_CELLS`].
fn bucket(room: f32, rows: usize) -> f32 {
    let rows = rows.max(1) as f32;
    if (room / BUCKET).ceil().max(1.0) * rows <= MAX_CELLS as f32 {
        BUCKET
    } else {
        (room * rows / MAX_CELLS as f32).ceil().max(BUCKET)
    }
}

/// The cells of a clip in `body` (its note area, in points) at `time`: the
/// hits in view, gathered per row and picture column, loudest wins.
fn cells(clip: &Clip, rows: &[usize], map: &DrumMap, body: Rect, time: TimeView) -> Vec<Cell> {
    let notes = match &clip.content {
        ClipContent::Midi { notes, .. } => notes.as_slice(),
        ClipContent::Audio { .. } => return Vec::new(),
    };
    if rows.is_empty() || body.width() <= 0.0 || body.height() <= 0.0 {
        return Vec::new();
    }
    let mut row_of = [usize::MAX; 128];
    for (at, &index) in rows.iter().enumerate() {
        if let Some(sound) = map.sounds.get(index) {
            row_of[usize::from(sound.input)] = at;
        }
    }
    let bucket = bucket(body.width(), rows.len());
    let cols = ((body.width() / bucket).ceil() as usize + 1).max(1);
    let mut seen: Vec<Option<Cell>> = vec![None; rows.len() * cols];
    let place = ClipNotes {
        start: clip.start,
        length: clip.length,
        loop_length: clip.loop_length,
        loop_phase: clip.loop_phase,
        notes,
        carried: &[],
        outside: false,
    };
    place.each_placed(|placed: Placed, note: Note| {
        let Some(&row) = row_of.get(usize::from(note.pitch())) else {
            return;
        };
        if row == usize::MAX {
            return;
        }
        let x = time.x(body.left(), placed.start);
        if x < body.left() - 1.0 || x > body.right() {
            return;
        }
        let col =
            (((x - body.left()) / bucket).floor() as isize).clamp(0, cols as isize - 1) as usize;
        let slot = &mut seen[row * cols + col];
        let muted = note.muted() || placed.rep > 0;
        match slot {
            Some(cell) => {
                cell.x = cell.x.min(x);
                cell.velocity = cell.velocity.max(note.velocity());
                cell.muted &= muted;
            }
            None => {
                *slot = Some(Cell {
                    row,
                    x,
                    velocity: note.velocity().min(127),
                    muted,
                })
            }
        }
    });
    seen.into_iter().flatten().collect()
}

/// Draws a mapped clip's drum rows in `body` (the clip's note area, in
/// points) and answers how many rows and cells it drew (the tests' proof the
/// picture was drawn). `alpha` dims the picture with an inaudible or dimmed
/// track.
pub(crate) fn paint(
    painter: &Painter,
    map: &DrumMap,
    clip: &Arc<Clip>,
    body: Rect,
    time: TimeView,
    alpha: f32,
) -> (usize, usize) {
    let notes = match &clip.content {
        ClipContent::Midi { notes, .. } => notes.as_slice(),
        ClipContent::Audio { .. } => return (0, 0),
    };
    let rows = rows(map, notes, body.height());
    if rows.is_empty() {
        return (0, 0);
    }
    let painter = painter.with_clip_rect(body.intersect(painter.clip_rect()));
    let p = theme::p();
    let row_height = body.height() / rows.len() as f32;
    // A line between sounds, so the clip reads as one row per sound.
    for row in 1..rows.len() {
        let y = body.top() + row as f32 * row_height;
        painter.hline(
            body.x_range(),
            y,
            Stroke::new(
                theme::stroke::HAIRLINE,
                p.text_secondary
                    .gamma_multiply(theme::alpha::WASH_FAINT * alpha),
            ),
        );
    }
    let bucket = bucket(body.width(), rows.len());
    let mut drawn = 0;
    for cell in cells(clip, &rows, map, body, time) {
        if drawn >= MAX_CELLS {
            break;
        }
        drawn += 1;
        let Some(&index) = rows.get(cell.row) else {
            continue;
        };
        let Some(sound) = map.sounds.get(index) else {
            continue;
        };
        let colour = crate::theme::wave(crate::arrangement::color_to(sound.color));
        let colour = if cell.muted {
            colour.gamma_multiply(theme::alpha::EDGE_SOFT * alpha)
        } else {
            colour.gamma_multiply(alpha)
        };
        let top = body.top() + cell.row as f32 * row_height;
        let t = f32::from(cell.velocity) / 127.0;
        let cell_height = ((row_height - 1.4) * t).max(1.5);
        let width = (bucket - 1.2).max(1.5);
        let rect = Rect::from_min_max(
            pos2(cell.x, top + row_height - 1.4 - cell_height),
            pos2(cell.x + width, top + row_height - 1.4),
        );
        painter.rect_filled(rect, theme::radius::MARK, colour);
    }
    (rows.len(), drawn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::Ticks;
    use lumen_project::drums::presets::Preset;
    use lumen_project::model::Id;

    fn map() -> DrumMap {
        Preset::EIGHT_OH_EIGHT.map(Id(1))
    }

    fn clip(notes: Vec<Note>) -> Arc<Clip> {
        Arc::new(Clip {
            id: Id(2),
            name: "Beat".into(),
            start: Ticks::ZERO,
            length: Ticks::quarters(4),
            muted: false,
            color: None,
            locked: false,
            crossfade: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            takes: None,
            warp: None,
            content: ClipContent::Midi {
                notes,
                events: Vec::new(),
            },
        })
    }

    #[test]
    fn the_rows_are_the_sounds_played_in_map_order_and_thin_when_short() {
        let map = map();
        let (kick, snare, hat) = (
            map.sounds[0].input,
            map.sounds[1].input,
            map.sounds[2].input,
        );
        let notes = vec![
            Note::new(Ticks::ZERO, Ticks(30), snare, 100),
            Note::new(Ticks(120), Ticks(30), kick, 100),
            Note::new(Ticks(240), Ticks(30), kick, 100),
            Note::new(Ticks(360), Ticks(30), hat, 100),
        ];
        // Room for all three: map order, not play order.
        assert_eq!(rows(&map, &notes, 60.0), [0, 1, 2]);
        // Room for one: the kick, played twice, keeps its row.
        let one = rows(&map, &notes, MIN_ROW + 1.0);
        assert_eq!(one.len(), 1);
        assert_eq!(map.sounds[one[0]].input, kick);
        // A clip too short for a row draws none.
        assert!(rows(&map, &notes, MIN_ROW - 1.0).is_empty());
        // Nothing played: no rows.
        assert!(rows(&map, &[], 60.0).is_empty());
    }

    #[test]
    fn the_picture_gathers_hits_per_row_and_column_at_their_velocity() {
        let map = map();
        let kick = map.sounds[0].input;
        let notes = vec![
            Note::new(Ticks::ZERO, Ticks(30), kick, 30),
            Note::new(Ticks(1), Ticks(30), kick, 120),
            Note::new(Ticks::quarters(1), Ticks(30), kick, 80),
        ];
        let time = TimeView {
            left: 0.0,
            zoom: 24.0,
        };
        let body = Rect::from_min_max(pos2(100.0, 20.0), pos2(400.0, 50.0));
        let clip = clip(notes);
        let notes_slice = match &clip.content {
            ClipContent::Midi { notes, .. } => notes.as_slice(),
            _ => unreachable!(),
        };
        let rows = rows(&map, notes_slice, body.height());
        let cells = cells(&clip, &rows, &map, body, time);
        assert_eq!(cells.len(), 2, "{cells:?}");
        assert_eq!(cells[0].velocity, 120, "the loudest of the first column");
        assert_eq!(cells[0].x, 100.0);
        assert_eq!(cells[1].x, 124.0, "a beat on at 24 points a beat");
    }

    #[test]
    fn a_loop_repeats_the_picture_and_a_muted_hit_reads_quiet() {
        let map = map();
        let kick = map.sounds[0].input;
        let mut clip = (*clip(vec![Note::new(Ticks::ZERO, Ticks(30), kick, 100)])).clone();
        clip.length = Ticks::quarters(2);
        clip.loop_length = Some(Ticks::quarters(1));
        let clip = Arc::new(clip);
        let time = TimeView {
            left: 0.0,
            zoom: 24.0,
        };
        let body = Rect::from_min_max(pos2(0.0, 0.0), pos2(96.0, 40.0));
        let notes = match &clip.content {
            ClipContent::Midi { notes, .. } => notes.clone(),
            _ => unreachable!(),
        };
        let rows = rows(&map, &notes, body.height());
        let cells = cells(&clip, &rows, &map, body, time);
        assert_eq!(cells.len(), 2, "two repeats: {cells:?}");
        assert!(!cells[0].muted);
        assert!(cells[1].muted, "a repeat reads fainter");
    }
}
