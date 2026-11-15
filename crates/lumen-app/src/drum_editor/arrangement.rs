use std::sync::Arc;

use eframe::egui::{Painter, Rect, Stroke, pos2};
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, ClipContent, DrumMap, Id, Note};

use crate::piano_roll::notes::{ClipNotes, Placed};
use crate::theme;
use crate::timeview::TimeView;

pub(crate) const MIN_ROW: f32 = theme::size::DRUM_ROW_MIN;
const MAX_CELLS: usize = 4000;
const BUCKET: f32 = 2.0;
const MARK_MAX: f32 = 3.0;
const MARK_MIN: f32 = 1.5;

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

#[derive(Debug, Clone, Copy, PartialEq)]
struct Cell {
    row: usize,
    x: f32,
    velocity: u8,
    muted: bool,
}

fn bucket(room: f32, rows: usize) -> f32 {
    let rows = rows.max(1) as f32;
    if (room / BUCKET).ceil().max(1.0) * rows <= MAX_CELLS as f32 {
        BUCKET
    } else {
        (room * rows / MAX_CELLS as f32).ceil().max(BUCKET)
    }
}

fn sorted(notes: &[Note]) -> bool {
    notes
        .windows(2)
        .all(|pair| pair[0].start() <= pair[1].start())
}

fn mark_width(bucket: f32) -> f32 {
    (bucket - 1.2).clamp(MARK_MIN, MARK_MAX)
}

#[derive(Debug, Default)]
struct Gathered {
    cells: Vec<Cell>,
    visited: usize,
}

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

#[derive(Debug)]
pub(crate) struct Picture {
    clip: Option<Arc<Clip>>,
    map: Option<(Id, u64)>,
    visible: Rect,
    body: Rect,
    time: TimeView,
    rows: Vec<usize>,
    cells: Vec<Cell>,
    sorted: bool,
    pub visited: usize,
    pub drawn: (usize, usize),
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct MapKey<'a> {
    pub map: &'a DrumMap,
    pub revision: u64,
}

impl Picture {
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
        assert_eq!(rows(&map, &notes, 60.0), [0, 1, 2]);
        let one = rows(&map, &notes, MIN_ROW + 1.0);
        assert_eq!(one.len(), 1);
        assert_eq!(map.sounds[one[0]].input, kick);
        assert!(rows(&map, &notes, MIN_ROW - 1.0).is_empty());
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
        assert!(cells.iter().all(|cell| !cell.muted));
    }

    #[test]
    fn a_looped_clip_visits_only_the_repeats_the_view_shows() {
        let map = map();
        let kick = map.sounds[0].input;
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
        let map = map();
        let (kick, snare, hat) = (
            map.sounds[0].input,
            map.sounds[1].input,
            map.sounds[2].input,
        );
        let beat = Ticks::quarters(1).0;
        let notes = vec![
            Note::new(Ticks(0), Ticks(beat / 8), snare, 70),
            Note::new(Ticks(beat / 4), Ticks(beat / 2), hat, 80),
            Note::new(Ticks(beat / 2), Ticks(beat / 8), kick, 100),
            Note::new(Ticks(3 * beat / 2), Ticks(beat / 8), snare, 110),
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
