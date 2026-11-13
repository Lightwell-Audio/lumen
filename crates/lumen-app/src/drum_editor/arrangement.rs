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
use lumen_project::model::{Clip, ClipContent, DrumMap, Id, Note};

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

/// The widest a mark is drawn for a picture column `bucket` points wide: a few
/// points, whatever the zoom, so a hit stays a hit and never becomes a wall
/// (review H1).
fn mark_width(bucket: f32) -> f32 {
    (bucket - 1.2).clamp(MARK_MIN, MARK_MAX)
}

/// A picture's gathered cells, and how many hits the gather looked at: the
/// tests' proof the work is bounded by the view (review M2).
#[derive(Debug, Default)]
struct Gathered {
    cells: Vec<Cell>,
    visited: usize,
}

/// The cells of a clip's *visible* part, gathered per row and picture column,
/// loudest wins. A sorted clip's notes are found by range, and a looping
/// clip's repeats are bounded to the ones the view shows, with only the notes
/// in each repeat's window visited (review H2, M2); a clip whose notes are not
/// in order falls back to `ClipNotes::each_placed` and is filtered by the view.
fn cells(
    clip: &Clip,
    rows: &[usize],
    map: &DrumMap,
    visible: Rect,
    body: Rect,
    time: TimeView,
    sorted: bool,
) -> Gathered {
    let notes = match &clip.content {
        ClipContent::Midi { notes, .. } => notes.as_slice(),
        ClipContent::Audio { .. } => return Gathered::default(),
    };
    if rows.is_empty() || visible.width() <= 0.0 || body.width() <= 0.0 {
        return Gathered::default();
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
    let mut visited = 0;
    let mut visit = |note: Note, placed: Placed| {
        visited += 1;
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
    } else if clip.loop_length.is_some() && sorted {
        // A looping clip: only the repeats the view shows, and inside each
        // only the notes whose onset falls in that repeat's visible window
        // (review M2). The first repeat may be partial (the loop's phase); a
        // note carried into it sounds at the clip's start.
        let period = clip
            .loop_length
            .map(|p| p.0)
            .filter(|&p| p > 0)
            .unwrap_or(clip.length.0)
            .max(1);
        let phase = clip.loop_phase.0.rem_euclid(period);
        let first_span = (period - phase).max(1).min(clip.length.0.max(1));
        let end = to.0.min(clip.length.0);
        let mut repeat = if from.0 < first_span {
            0
        } else {
            1 + (from.0 - first_span).div_euclid(period)
        };
        loop {
            let (base, span, offset) = if repeat == 0 {
                (0, first_span, phase)
            } else {
                let base = first_span + (repeat - 1) * period;
                (base, period.min(clip.length.0 - base).max(0), 0)
            };
            if span <= 0 || base > end {
                break;
            }
            let lo = from.0.max(base);
            let hi = to.0.min(base + span);
            let mut place = |note: &Note, local: i64| {
                let start = Ticks(clip.start.0 + local).as_quarters();
                visit(
                    *note,
                    Placed {
                        index: 0,
                        rep: 0,
                        start,
                        end: start,
                    },
                );
            };
            // A note that starts before the phase and still sounds across it
            // is carried to the clip's start, as `ClipNotes::each_placed`
            // places it; the rest of the notes before the phase are not
            // heard. Only when the clip's start is in view.
            if repeat == 0 && phase > 0 && from.0 <= 0 {
                let before = notes.partition_point(|note| note.start() < Ticks(phase));
                for note in &notes[..before] {
                    let sounds_to = (note.start().0 + note.length().0).min(period);
                    if note.start().0 >= 0 && sounds_to > phase {
                        place(note, 0);
                    }
                }
            }
            if hi >= lo {
                // The repeat plays the source from `offset` for `span` ticks:
                // a note starting at the window's end belongs to the next
                // repeat, as in `each_placed`.
                let source_from = lo - base + offset;
                let source_to = hi - base + offset;
                let source_end = offset + span;
                let first = notes.partition_point(|note| note.start() < Ticks(source_from));
                for note in notes[first..].iter().take_while(|note| {
                    note.start() <= Ticks(source_to) && note.start() < Ticks(source_end)
                }) {
                    place(note, base + (note.start().0 - offset));
                }
            }
            repeat += 1;
        }
    } else {
        // A clip whose notes are not in order (a loaded file): every placed
        // note is offered and the view filters it. Sorted clips never come
        // here, so the cost is the clip's own note count, not the view's.
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
    Gathered {
        cells: seen.into_iter().flatten().collect(),
        visited,
    }
}

/// What a clip's picture drew last, and from what: rebuilt only when the clip,
/// the map, the visible part or the view changed (review H2).
#[derive(Debug)]
pub(crate) struct Picture {
    clip: Option<Arc<Clip>>,
    /// The map's id and the document's revision when the cells were gathered:
    /// a map edit bumps the revision, so the picture is not compared field by
    /// field each frame (review M1).
    map: Option<(Id, u64)>,
    visible: Rect,
    body: Rect,
    time: TimeView,
    rows: Vec<usize>,
    cells: Vec<Cell>,
    sorted: bool,
    /// How many hits the last gather looked at (the tests' bound, review M2).
    pub visited: usize,
    /// The rows and cells drawn, for the tests.
    pub drawn: (usize, usize),
    /// The widest mark drawn, in points (the tests' proof of H1).
    pub widest: f32,
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
            visited: 0,
            drawn: (0, 0),
            widest: 0.0,
        }
    }
}

/// What a picture is gathered from besides the clip and the view: the map it
/// draws through, and the document's revision when it was gathered (an edit
/// bumps it, so a map change rebuilds the picture without comparing every
/// sound each frame; review M1).
#[derive(Debug, Clone, Copy)]
pub(crate) struct MapKey<'a> {
    pub map: &'a DrumMap,
    pub revision: u64,
}

impl Picture {
    /// The rows and the cells of `clip` in `visible`, gathered again only when
    /// something they depend on changed. `key` is the map and the document's
    /// revision: comparing those is cheap, where comparing every sound of the
    /// map each frame was not (review M1).
    fn gather(
        &mut self,
        clip: &Arc<Clip>,
        key: MapKey<'_>,
        visible: Rect,
        body: Rect,
        time: TimeView,
        notes: &[Note],
    ) {
        let map = key.map;
        let stamp = (map.id, key.revision);
        let fresh = self
            .clip
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, clip))
            && self.map == Some(stamp)
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
        let gathered = cells(clip, &self.rows, map, visible, body, time, self.sorted);
        self.cells = gathered.cells;
        self.visited = gathered.visited;
        self.clip = Some(clip.clone());
        self.map = Some(stamp);
        self.visible = visible;
        self.body = body;
        self.time = time;
    }
}

/// Draws a mapped clip's drum rows in `body` (the clip's note area, in points)
/// and answers the rows, the cells and the widest mark it drew (the tests'
/// proof the picture was drawn and stayed narrow). `key` is the map and the
/// document's revision, so a map edit rebuilds the picture. `alpha` dims the
/// picture with an inaudible or dimmed track.
pub(crate) fn paint(
    painter: &Painter,
    key: MapKey<'_>,
    clip: &Arc<Clip>,
    body: Rect,
    time: TimeView,
    alpha: f32,
    picture: &mut Picture,
) -> (usize, usize, f32) {
    let map = key.map;
    let notes = match &clip.content {
        ClipContent::Midi { notes, .. } => notes.as_slice(),
        ClipContent::Audio { .. } => {
            picture.drawn = (0, 0);
            picture.widest = 0.0;
            return (0, 0, 0.0);
        }
    };
    // Only the visible part of the clip is a picture: at the deepest zoom the
    // body is tens of thousands of points wide (review H1).
    let visible = body.intersect(painter.clip_rect());
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        picture.drawn = (0, 0);
        picture.widest = 0.0;
        return (0, 0, 0.0);
    }
    picture.gather(clip, key, visible, body, time, notes);
    let rows = &picture.rows;
    if rows.is_empty() {
        picture.drawn = (0, 0);
        picture.widest = 0.0;
        return (0, 0, 0.0);
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
    let width = mark_width(bucket);
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
    picture.widest = width;
    (rows.len(), drawn, width)
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
        assert_eq!(cells.cells.len(), 2, "{cells:?}");
        assert_eq!(
            cells.cells[0].velocity, 120,
            "the loudest of the first column"
        );
        assert_eq!(cells.cells[0].x, 100.0);
        assert_eq!(cells.cells[1].x, 124.0, "a beat on at 24 points a beat");
        assert_eq!(cells.visited, 3, "each note offered once");
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
        let cells = cells(&clip, &rows, &map, body, body, time, true).cells;
        assert_eq!(cells.len(), 2, "two repeats: {cells:?}");
        // A repeat is a real hit, not a muted one (review M7).
        assert!(cells.iter().all(|cell| !cell.muted));
    }

    #[test]
    fn a_looped_clip_visits_only_the_repeats_the_view_shows() {
        // Review M2: a one-bar loop repeated for hours, seen at a deep zoom:
        // the gather visits the repeat in view and the notes in its window,
        // not every repeat and every note.
        let map = map();
        let kick = map.sounds[0].input;
        // 500 hits spread over one beat, the loop's length; the clip lasts
        // hours.
        let beat = Ticks::quarters(1).0;
        let notes: Vec<Note> = (0..500_i64)
            .map(|i| Note::new(Ticks(i * beat / 500), Ticks(4), kick, 100))
            .collect();
        let mut clip = (*clip(notes)).clone();
        clip.length = Ticks::quarters(4 * 3600);
        clip.loop_length = Some(Ticks(beat));
        let clip = Arc::new(clip);
        let time = TimeView {
            left: 0.0,
            zoom: 4_000.0,
        };
        let body = Rect::from_min_max(pos2(0.0, 0.0), pos2(1_411_200.0 * 4.0 * 3600.0, 40.0));
        let visible = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 40.0));
        let notes = match &clip.content {
            ClipContent::Midi { notes, .. } => notes.clone(),
            _ => unreachable!(),
        };
        let rows = rows(&map, &notes, body.height());
        let gathered = cells(&clip, &rows, &map, visible, body, time, true);
        assert!(!gathered.cells.is_empty());
        assert!(
            gathered.visited <= 60,
            "the view bounds the work: {} visited",
            gathered.visited
        );
    }

    #[test]
    fn the_ranged_loop_gather_places_hits_as_the_notes_do() {
        // The ranged loop path must draw what `ClipNotes::each_placed` places:
        // a phased loop carries only a note that sounds across the phase, a
        // note before the phase that has ended is not heard, and a note at the
        // loop's end belongs to no repeat.
        let map = map();
        let (kick, snare, hat) = (
            map.sounds[0].input,
            map.sounds[1].input,
            map.sounds[2].input,
        );
        let beat = Ticks::quarters(1).0;
        let notes = vec![
            // Before the phase, ended before it: not heard in the first pass.
            Note::new(Ticks(0), Ticks(beat / 8), snare, 70),
            // Before the phase, still sounding across it: carried to 0.
            Note::new(Ticks(beat / 4), Ticks(beat / 2), hat, 80),
            Note::new(Ticks(beat / 2), Ticks(beat / 8), kick, 100),
            Note::new(Ticks(3 * beat / 2), Ticks(beat / 8), snare, 110),
            // At the loop's end: in no repeat.
            Note::new(Ticks(2 * beat), Ticks(beat / 8), hat, 120),
        ];
        let mut clip = (*clip(notes)).clone();
        clip.length = Ticks::quarters(7);
        clip.loop_length = Some(Ticks(2 * beat));
        clip.loop_phase = Ticks(3 * beat / 8);
        let clip = Arc::new(clip);
        let time = TimeView {
            left: 0.0,
            zoom: 96.0,
        };
        let body = Rect::from_min_max(pos2(0.0, 0.0), pos2(7.0 * 96.0, 60.0));
        let notes = match &clip.content {
            ClipContent::Midi { notes, .. } => notes.clone(),
            _ => unreachable!(),
        };
        assert!(sorted(&notes));
        let rows = rows(&map, &notes, body.height());
        let key = |gathered: Gathered| {
            let mut cells: Vec<(i64, usize, u8)> = gathered
                .cells
                .iter()
                .map(|cell| ((cell.x * 100.0).round() as i64, cell.row, cell.velocity))
                .collect();
            cells.sort_unstable();
            cells
        };
        // The whole clip, and a window that starts mid-way.
        for visible in [
            body,
            Rect::from_min_max(pos2(2.5 * 96.0, 0.0), pos2(5.5 * 96.0, 60.0)),
        ] {
            let ranged = key(cells(&clip, &rows, &map, visible, body, time, true));
            let placed = key(cells(&clip, &rows, &map, visible, body, time, false));
            assert!(!ranged.is_empty());
            assert_eq!(ranged, placed, "in {visible:?}");
        }
    }

    #[test]
    fn a_clip_whose_notes_are_not_in_order_is_still_drawn() {
        // Review L1: the unsorted fallback (a loaded file) draws the same
        // picture, filtered by the view.
        let map = map();
        let kick = map.sounds[0].input;
        let notes = vec![
            Note::new(Ticks::quarters(1), Ticks(30), kick, 90),
            Note::new(Ticks::ZERO, Ticks(30), kick, 60),
            Note::new(Ticks::quarters(2), Ticks(30), kick, 120),
        ];
        let clip = clip(notes);
        let time = TimeView {
            left: 0.0,
            zoom: 24.0,
        };
        let body = Rect::from_min_max(pos2(0.0, 0.0), pos2(96.0, 40.0));
        let notes = match &clip.content {
            ClipContent::Midi { notes, .. } => notes.clone(),
            _ => unreachable!(),
        };
        assert!(!sorted(&notes), "the file's order is not kept");
        let rows = rows(&map, &notes, body.height());
        let gathered = cells(&clip, &rows, &map, body, body, time, false);
        assert_eq!(gathered.cells.len(), 3, "{gathered:?}");
        assert!(gathered.cells.iter().all(|cell| cell.x <= body.right()));
    }

    #[test]
    fn a_mark_is_narrow_at_every_zoom() {
        // Review L1: the widest a mark gets, zoomed out and at the deepest
        // zoom, not a tautology about the clamp.
        assert_eq!(mark_width(BUCKET), MARK_MIN, "a 2 pt column, zoomed out");
        assert!(
            (mark_width(1_000.0) - MARK_MAX).abs() < 1e-3,
            "deep in, a mark stays {MARK_MAX} pt: {}",
            mark_width(1_000.0)
        );
        assert!((mark_width(MARK_MAX + 1.2) - MARK_MAX).abs() < 1e-3);
    }

    #[test]
    fn the_visible_part_bounds_the_picture_and_the_marks_stay_narrow() {
        // A clip a hundred beats wide: its picture is gathered for the
        // visible part only, and a mark is never a wall (review H1).
        let map = map();
        let kick = map.sounds[0].input;
        let beat = Ticks::quarters(1).0;
        let notes: Vec<Note> = (0..1000_i64)
            .map(|i| Note::new(Ticks(i * 100 * beat / 1000), Ticks(60), kick, 100))
            .collect();
        let mut clip = (*clip(notes)).clone();
        clip.length = Ticks::quarters(100);
        let clip = Arc::new(clip);
        let time = TimeView {
            left: 0.0,
            zoom: 2_000.0,
        };
        let body = Rect::from_min_max(pos2(0.0, 0.0), pos2(200_000.0, 40.0));
        let visible = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 40.0));
        let notes = match &clip.content {
            ClipContent::Midi { notes, .. } => notes.clone(),
            _ => unreachable!(),
        };
        let rows = rows(&map, &notes, body.height());
        let gathered = cells(&clip, &rows, &map, visible, body, time, true);
        assert!(!gathered.cells.is_empty());
        assert!(gathered.cells.iter().all(|cell| cell.x <= visible.right()));
        assert!(
            gathered.visited < notes.len(),
            "the visible part bounds the gather: {} of {}",
            gathered.visited,
            notes.len()
        );
        // The gather is cached: the same clip, map, revision, part and view do
        // not gather again (a frame that changes nothing costs nothing).
        let mut picture = Picture::default();
        picture.gather(
            &clip,
            MapKey {
                map: &map,
                revision: 0,
            },
            visible,
            body,
            time,
            &notes,
        );
        let cells = picture.cells.len();
        picture.gather(
            &clip,
            MapKey {
                map: &map,
                revision: 0,
            },
            visible,
            body,
            time,
            &notes,
        );
        assert_eq!(picture.cells.len(), cells);
        assert_eq!(picture.visible, visible);
        // A map edit (a new revision) gathers again.
        picture.gather(
            &clip,
            MapKey {
                map: &map,
                revision: 1,
            },
            visible,
            body,
            time,
            &notes,
        );
        assert_eq!(picture.map, Some((map.id, 1)));
    }
}
