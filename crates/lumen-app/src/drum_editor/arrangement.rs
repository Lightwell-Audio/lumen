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
//! Only the hits in view are gathered, one per picture column of the *visible*
//! part of the clip (never the clip's whole width, which at the deepest zoom
//! is tens of thousands of points): a mark is never wider than a few points,
//! so zooming in narrows the picture rather than turning hits into walls
//! (review H1). The gather is cached by the clip, the map, the visible part and
//! the view, so a frame that changes nothing costs nothing (review H2).

use std::sync::Arc;

use eframe::egui::{Painter, Rect, Stroke, pos2};
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, ClipContent, DrumMap, Note};

use crate::piano_roll::notes::{ClipNotes, Placed};
use crate::theme;
use crate::timeview::TimeView;

/// The shortest a row is drawn: below this it stops being a picture.
pub(crate) const MIN_ROW: f32 = theme::size::DRUM_ROW_MIN;
/// The most cells one clip's picture draws in a frame; a deeper zoom gathers
/// more hits per cell rather than drawing them all.
const MAX_CELLS: usize = 4000;
/// The narrowest picture column one hit is gathered into (points).
const BUCKET: f32 = 2.0;
/// The widest a mark is drawn: a few points, whatever the zoom, so a hit stays
/// a hit and never becomes a wall (review H1).
const MARK_MAX: f32 = 3.0;
/// The least a mark is drawn.
const MARK_MIN: f32 = 1.5;

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

/// The picture columns the *visible* part of a clip gets: `BUCKET` widened
/// until the cells fit [`MAX_CELLS`]. Returns points per column.
fn bucket(room: f32, rows: usize) -> f32 {
    let rows = rows.max(1) as f32;
    if (room / BUCKET).ceil().max(1.0) * rows <= MAX_CELLS as f32 {
        BUCKET
    } else {
        (room * rows / MAX_CELLS as f32).ceil().max(BUCKET)
    }
}

/// Whether a clip's notes are in start order (the editors keep them so; a file
/// may not), cached with the picture so the check runs once per clip.
fn sorted(notes: &[Note]) -> bool {
    notes
        .windows(2)
        .all(|pair| pair[0].start() <= pair[1].start())
}

/// The cells of a clip's *visible* part, gathered per row and picture column,
/// loudest wins. A looping clip's repeats are visited only where they fall in
/// view (review H2); a sorted clip's notes are found by range.
fn cells(
    clip: &Clip,
    rows: &[usize],
    map: &DrumMap,
    visible: Rect,
    body: Rect,
    time: TimeView,
    sorted: bool,
) -> Vec<Cell> {
    let notes = match &clip.content {
        ClipContent::Midi { notes, .. } => notes.as_slice(),
        ClipContent::Audio { .. } => return Vec::new(),
    };
    if rows.is_empty() || visible.width() <= 0.0 || body.width() <= 0.0 {
        return Vec::new();
    }
    let mut row_of = [usize::MAX; 128];
    for (at, &index) in rows.iter().enumerate() {
        if let Some(sound) = map.sounds.get(index) {
            row_of[usize::from(sound.input)] = at;
        }
    }
    let bucket = bucket(visible.width(), rows.len());
    let cols = ((visible.width() / bucket).ceil() as usize + 1).max(1);
    let mut seen: Vec<Option<Cell>> = vec![None; rows.len() * cols];
    // The clip-local times the visible part shows (in ticks), so a sorted
    // clip's notes are found by range rather than scanned (H2).
    let beat = |x: f32| time.beat_at(body.left(), x);
    let from = Ticks(
        ((beat(visible.left()) * lumen_core::time::PPQ as f64).floor() as i64) - clip.start.0,
    );
    let to = Ticks(
        ((beat(visible.right()) * lumen_core::time::PPQ as f64).ceil() as i64) - clip.start.0,
    );
    let mut visit = |note: Note, placed: Placed| {
        let Some(&row) = row_of.get(usize::from(note.pitch())) else {
            return;
        };
        if row == usize::MAX {
            return;
        }
        let x = time.x(body.left(), placed.start);
        if x < visible.left() - 1.0 || x > visible.right() {
            return;
        }
        let col =
            (((x - visible.left()) / bucket).floor() as isize).clamp(0, cols as isize - 1) as usize;
        let slot = &mut seen[row * cols + col];
        let muted = note.muted();
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
    };
    if clip.loop_length.is_none() && sorted {
        // One pass over the notes in view, found by their start (H2).
        let first = notes.partition_point(|note| note.start() < from);
        for note in notes[first..].iter().take_while(|note| note.start() <= to) {
            let start = (clip.start + note.start()).as_quarters();
            visit(
                *note,
                Placed {
                    index: 0,
                    rep: 0,
                    start,
                    end: start,
                },
            );
        }
    } else {
        // A looping clip (or one whose notes are not in order): the repeats
        // are visited once, and only the hits in view are gathered; the
        // result is cached, so this runs only when the picture changes (H2).
        let place = ClipNotes {
            start: clip.start,
            length: clip.length,
            loop_length: clip.loop_length,
            loop_phase: clip.loop_phase,
            notes,
            carried: &[],
            outside: false,
        };
        let (from, to) = (from.as_quarters() - 1.0, to.as_quarters() + 1.0);
        place.each_placed(|placed, note| {
            if placed.start >= from && placed.start <= to {
                visit(note, placed);
            }
        });
    }
    seen.into_iter().flatten().collect()
}

/// What a clip's picture drew last, and from what: rebuilt only when the clip,
/// the map, the visible part or the view changed (review H2).
#[derive(Debug)]
pub(crate) struct Picture {
    clip: Option<Arc<Clip>>,
    map: Option<DrumMap>,
    visible: Rect,
    body: Rect,
    time: TimeView,
    rows: Vec<usize>,
    cells: Vec<Cell>,
    sorted: bool,
    /// The rows and cells drawn, for the tests.
    pub drawn: (usize, usize),
}

impl Default for Picture {
    fn default() -> Self {
        Self {
            clip: None,
            map: None,
            visible: Rect::NOTHING,
            body: Rect::NOTHING,
            time: TimeView::default(),
            rows: Vec::new(),
            cells: Vec::new(),
            sorted: false,
            drawn: (0, 0),
        }
    }
}

impl Picture {
    /// The rows and the cells of `clip` in `visible`, gathered again only when
    /// something they depend on changed.
    fn gather(
        &mut self,
        clip: &Arc<Clip>,
        map: &DrumMap,
        visible: Rect,
        body: Rect,
        time: TimeView,
        notes: &[Note],
    ) {
        let fresh = self
            .clip
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, clip))
            && self.map.as_ref() == Some(map)
            && self.visible == visible
            && self.body == body
            && self.time == time;
        if fresh {
            return;
        }
        let same_clip = self
            .clip
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, clip));
        if !same_clip {
            self.sorted = sorted(notes);
        }
        self.rows = rows(map, notes, body.height());
        self.cells = cells(clip, &self.rows, map, visible, body, time, self.sorted);
        self.clip = Some(clip.clone());
        self.map = Some(map.clone());
        self.visible = visible;
        self.body = body;
        self.time = time;
    }
}

/// Draws a mapped clip's drum rows in `body` (the clip's note area, in points)
/// and answers how many rows and cells it drew (the tests' proof the picture
/// was drawn). `alpha` dims the picture with an inaudible or dimmed track.
pub(crate) fn paint(
    painter: &Painter,
    map: &DrumMap,
    clip: &Arc<Clip>,
    body: Rect,
    time: TimeView,
    alpha: f32,
    picture: &mut Picture,
) -> (usize, usize) {
    let notes = match &clip.content {
        ClipContent::Midi { notes, .. } => notes.as_slice(),
        ClipContent::Audio { .. } => {
            picture.drawn = (0, 0);
            return (0, 0);
        }
    };
    // Only the visible part of the clip is a picture: at the deepest zoom the
    // body is tens of thousands of points wide (review H1).
    let visible = body.intersect(painter.clip_rect());
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        picture.drawn = (0, 0);
        return (0, 0);
    }
    picture.gather(clip, map, visible, body, time, notes);
    let rows = &picture.rows;
    if rows.is_empty() {
        picture.drawn = (0, 0);
        return (0, 0);
    }
    let painter = painter.with_clip_rect(visible);
    let p = theme::p();
    let row_height = body.height() / rows.len() as f32;
    // A line between sounds, so the clip reads as one row per sound.
    for row in 1..rows.len() {
        let y = body.top() + row as f32 * row_height;
        painter.hline(
            visible.x_range(),
            y,
            Stroke::new(
                theme::stroke::HAIRLINE,
                p.text_secondary
                    .gamma_multiply(theme::alpha::WASH_FAINT * alpha),
            ),
        );
    }
    let bucket = bucket(visible.width(), rows.len());
    let width = (bucket - 1.2).clamp(MARK_MIN, MARK_MAX);
    let mut drawn = 0;
    for cell in &picture.cells {
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
        let rect = Rect::from_min_max(
            pos2(cell.x, top + row_height - 1.4 - cell_height),
            pos2(cell.x + width, top + row_height - 1.4),
        );
        painter.rect_filled(rect, theme::radius::MARK, colour);
    }
    picture.drawn = (rows.len(), drawn);
    picture.drawn
}

#[cfg(test)]
mod tests {
    use eframe::egui::pos2;
    use lumen_project::model::Id;

    use super::*;
    use lumen_project::drums::presets::Preset;

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
        let cells = cells(&clip, &rows, &map, body, body, time, sorted(notes_slice));
        assert_eq!(cells.len(), 2, "{cells:?}");
        assert_eq!(cells[0].velocity, 120, "the loudest of the first column");
        assert_eq!(cells[0].x, 100.0);
        assert_eq!(cells[1].x, 124.0, "a beat on at 24 points a beat");
    }

    #[test]
    fn a_loop_repeats_the_picture_and_its_hits_are_not_muted() {
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
        let cells = cells(&clip, &rows, &map, body, body, time, true);
        assert_eq!(cells.len(), 2, "two repeats: {cells:?}");
        // A repeat is a real hit, not a muted one (review M7).
        assert!(cells.iter().all(|cell| !cell.muted));
    }

    #[test]
    fn the_visible_part_bounds_the_picture_and_the_marks_stay_narrow() {
        // A clip a hundred thousand points wide: its picture is gathered for
        // the visible part only, and a mark is never a wall (review H1).
        let map = map();
        let kick = map.sounds[0].input;
        let notes: Vec<Note> = (0..1000)
            .map(|i| Note::new(Ticks(i * 100), Ticks(60), kick, 100))
            .collect();
        let mut clip = (*clip(notes)).clone();
        clip.length = Ticks(100_000);
        let clip = Arc::new(clip);
        let time = TimeView {
            left: 0.0,
            zoom: 2_000.0,
        };
        let body = Rect::from_min_max(pos2(0.0, 0.0), pos2(100_000.0, 40.0));
        let visible = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 40.0));
        let notes = match &clip.content {
            ClipContent::Midi { notes, .. } => notes.clone(),
            _ => unreachable!(),
        };
        let rows = rows(&map, &notes, body.height());
        let cells = cells(&clip, &rows, &map, visible, body, time, true);
        assert!(!cells.is_empty());
        assert!(cells.iter().all(|cell| cell.x <= visible.right()));
        let bucket = bucket(visible.width(), rows.len());
        assert!(
            (bucket - 1.2).clamp(MARK_MIN, MARK_MAX) <= MARK_MAX,
            "a mark is at most {MARK_MAX} points"
        );
        // The gather is cached: the same clip, map, part and view do not
        // gather again (a frame that changes nothing costs nothing).
        let mut picture = Picture::default();
        picture.gather(&clip, &map, visible, body, time, &notes);
        let cells = picture.cells.len();
        picture.gather(&clip, &map, visible, body, time, &notes);
        assert_eq!(picture.cells.len(), cells);
        assert_eq!(picture.visible, visible);
    }
}
