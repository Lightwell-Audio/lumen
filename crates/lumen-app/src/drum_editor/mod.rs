//! The drum editor's lanes (M8 slice D2, the owner's DV A and DE A): a clip on
//! a track with a drum map, shown as one lane per sound.
//!
//! - [`geometry`]: where the lanes and their hits are on the screen, and only
//!   the hits in view, gathered per pixel so a clip of a hundred thousand
//!   notes costs what its picture costs;
//! - [`paint`]: how a lane, a hit and the names read (the specimen's diamond,
//!   filled from the foot up to the velocity);
//! - [`arrangement`]: the same picture at clip size, on the arrangement (DV
//!   A's rows in place of the mini piano roll).
//!
//! The editing itself is the piano roll's: the same selection, snapping, zoom
//! and gesture code, given a different row set (E5). This module is what is
//! different about drums — the rows are a map's sounds in its own order, the
//! hits are diamonds, and a vertical drag on a hit sets its velocity.

pub(crate) mod arrangement;
pub(crate) mod geometry;
pub(crate) mod paint;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use eframe::egui::{Color32, Rect};
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, DrumMap, Id, Note, Quantize};

use crate::design::icons::DrumGlyph;
use crate::note_editor::view::EditorView;
use crate::timeview::TimeView;

use geometry::Cell;

/// Which view of a drum clip the Clip tab shows (the owner's DV A): the lanes,
/// or the piano roll, whose key column D1 made the kit's (Keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum View {
    #[default]
    Lanes,
    Keys,
}

/// One lane: a sound of the map, and what the clip and the view say about it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Lane {
    /// The note a clip stores for this sound (the lane's row).
    pub input: u8,
    pub name: String,
    pub icon: DrumGlyph,
    pub color: Color32,
    /// Muted in the map.
    pub mute: bool,
    /// Soloed in the map.
    pub solo: bool,
    /// Muted in the map, or silenced by another sound's solo.
    pub silent: bool,
    /// A hit in the clip is this sound.
    pub used: bool,
    /// How long the sound plays, whatever a note says (the map's override).
    pub length: Option<Ticks>,
    /// The sound's own quantize grid.
    pub quantize: Option<Quantize>,
    /// The map's place of the sound, for a reorder.
    pub index: usize,
    /// How many hits of this sound the clip holds (its whole content, outside
    /// its window too): the name column's accessible name, counted once with
    /// the lanes (H2).
    pub hits: usize,
}

/// The lanes the open clip shows: the map's sounds in the map's order, less
/// the unused ones while `hide_unused` is on (a sound with no hit in the
/// clip's notes, outside its window too, so a trimmed-away hit is not
/// forgotten and then found). One pass over the notes counts each sound's
/// hits.
pub(crate) fn lanes(map: &DrumMap, notes: &[Note], hide_unused: bool) -> Vec<Lane> {
    let mut used = [0_usize; 128];
    for note in notes {
        if let Some(slot) = used.get_mut(usize::from(note.pitch())) {
            *slot = slot.saturating_add(1);
        }
    }
    map.sounds
        .iter()
        .enumerate()
        .filter(|(_, sound)| {
            !hide_unused || used.get(usize::from(sound.input)).copied().unwrap_or(0) > 0
        })
        .map(|(index, sound)| Lane {
            input: sound.input,
            name: sound.name.clone(),
            icon: sound.icon.into(),
            color: crate::theme::wave(crate::arrangement::color_to(sound.color)),
            mute: sound.mute,
            solo: sound.solo,
            silent: !map.heard(sound),
            used: used.get(usize::from(sound.input)).copied().unwrap_or(0) > 0,
            length: sound.length,
            quantize: sound.quantize,
            index,
            hits: used.get(usize::from(sound.input)).copied().unwrap_or(0),
        })
        .collect()
}

/// A lane being dragged to another place: the map it belongs to, the place it
/// came from and where it would land (itself until the pointer moves it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reorder {
    pub map: Id,
    pub from: usize,
    pub to: usize,
}

/// The drum editor's state: which view a track's clips open in, which tracks
/// hide their unused lanes, which lanes show their sounds' lengths, and a
/// reorder in progress. None of it is saved: it is a view, like the lane's
/// shown state and Hide unmapped keys (D2's ADR), and it lasts while the app
/// runs.
#[derive(Debug, Default)]
pub(crate) struct State {
    views: BTreeMap<Id, View>,
    hiding: BTreeSet<Id>,
    /// The lanes set to show their length, by the map they belong to.
    lengths: BTreeSet<u8>,
    lengths_map: Option<Id>,
    pub reorder: Option<Reorder>,
    /// The lanes last built, with what they were built from: a frame that
    /// changes none of it draws them again from here, so nothing scans the
    /// clip's notes twice (H2).
    lanes: Option<LanesCache>,
    /// The hits last gathered for the lanes, with what they were gathered
    /// from.
    cells: Option<CellsCache>,
}

/// The lanes and the sound counts they were made from.
#[derive(Debug)]
struct LanesCache {
    map: DrumMap,
    clip: Arc<Clip>,
    hide: bool,
    lanes: Arc<Vec<Lane>>,
}

/// The gathered hits and what they were gathered from.
#[derive(Debug)]
struct CellsCache {
    clip: Arc<Clip>,
    rows: crate::piano_roll::draw::Rows,
    picked: Vec<usize>,
    hidden: Vec<usize>,
    body: Rect,
    time: TimeView,
    scroll: f32,
    cells: Arc<Vec<Cell>>,
    /// Reused between rebuilds, so a frame allocates nothing (H2).
    seen: Vec<Option<Cell>>,
    scratch: Vec<Cell>,
}

impl State {
    /// The lanes for `clip`, rebuilt only when the map, the clip or Hide
    /// unused changed.
    pub fn lanes_for(
        &mut self,
        map: &DrumMap,
        clip: &Arc<Clip>,
        notes: &[Note],
        hide: bool,
    ) -> Arc<Vec<Lane>> {
        let fresh = self.lanes.as_ref().is_some_and(|cache| {
            cache.map == *map && cache.hide == hide && Arc::ptr_eq(&cache.clip, clip)
        });
        if !fresh {
            self.lanes = Some(LanesCache {
                map: map.clone(),
                clip: clip.clone(),
                hide,
                lanes: Arc::new(lanes(map, notes, hide)),
            });
        }
        let cache = self.lanes.as_ref().expect("just built");
        cache.lanes.clone()
    }

    /// The hits of `clip` in `view`, with the `picked` and `hidden` ones
    /// marked; gathered again only when the clip, the rows, the picks, a
    /// gesture's hidden notes or the view changed.
    pub fn cells_for(
        &mut self,
        view: &EditorView,
        clip: &Arc<Clip>,
        notes: &[Note],
        picked: &[usize],
        hidden: &[usize],
    ) -> Arc<Vec<Cell>> {
        let fresh = self.cells.as_ref().is_some_and(|cache| {
            Arc::ptr_eq(&cache.clip, clip)
                && cache.rows == view.rows
                && cache.picked == picked
                && cache.hidden == hidden
                && cache.body == view.body
                && cache.time == view.time
                && cache.scroll == view.scroll
        });
        if !fresh {
            let mut cache = self.cells.take().unwrap_or(CellsCache {
                clip: clip.clone(),
                rows: view.rows,
                picked: Vec::new(),
                hidden: Vec::new(),
                body: view.body,
                time: view.time,
                scroll: view.scroll,
                cells: Arc::new(Vec::new()),
                seen: Vec::new(),
                scratch: Vec::new(),
            });
            geometry::cells_into(
                view,
                notes,
                picked,
                hidden,
                &mut cache.seen,
                &mut cache.scratch,
            );
            cache.cells = Arc::new(std::mem::take(&mut cache.scratch));
            cache.clip = clip.clone();
            cache.rows = view.rows;
            cache.picked.clear();
            cache.picked.extend_from_slice(picked);
            cache.hidden.clear();
            cache.hidden.extend_from_slice(hidden);
            cache.body = view.body;
            cache.time = view.time;
            cache.scroll = view.scroll;
            self.cells = Some(cache);
        }
        let cache = self.cells.as_ref().expect("just built");
        cache.cells.clone()
    }

    /// The view a track's drum clips open in (Lanes unless Keys was chosen).
    pub fn view(&self, track: Id) -> View {
        self.views.get(&track).copied().unwrap_or_default()
    }

    pub fn set_view(&mut self, track: Id, view: View) {
        if view == View::Lanes {
            self.views.remove(&track);
        } else {
            self.views.insert(track, view);
        }
    }

    /// Whether `track` hides its unused lanes.
    pub fn hides_unused(&self, track: Id) -> bool {
        self.hiding.contains(&track)
    }

    /// Turns Hide unused on or off for `track`; answers the new state.
    pub fn toggle_unused(&mut self, track: Id) -> bool {
        if self.hiding.remove(&track) {
            false
        } else {
            self.hiding.insert(track);
            true
        }
    }

    /// Whether the lane of `input` shows its length (the sound has no length
    /// override, and its lane was set to show it). The set belongs to one map:
    /// another map's lanes start with their lengths hidden.
    pub fn shows_length(&self, map: Id, input: u8) -> bool {
        self.lengths_map == Some(map) && self.lengths.contains(&input)
    }

    /// Whether the open map's lane of `input` shows its length, without
    /// naming the map (the pointer, for a box over the bars).
    pub fn shows_length_own(&self, input: u8) -> bool {
        self.lengths_map.is_some() && self.lengths.contains(&input)
    }

    /// Turns a lane's length on or off; answers the new state.
    pub fn toggle_length(&mut self, map: Id, input: u8) -> bool {
        if self.lengths_map != Some(map) {
            self.lengths.clear();
            self.lengths_map = Some(map);
        }
        if self.lengths.remove(&input) {
            false
        } else {
            self.lengths.insert(input);
            true
        }
    }

    /// The map whose lanes are open: another map's length settings are
    /// dropped, so they never leak from one kit to another.
    pub fn open_map(&mut self, map: Id) {
        if self.lengths_map != Some(map) {
            self.lengths.clear();
            self.lengths_map = Some(map);
        }
    }
}

/// The drum editor's empty states (M8 D2 review M6): a map with no sounds, and
/// Hide unused with nothing used.
pub(crate) const NO_SOUNDS: &str = "This drum map has no sounds: add them in the map editor";
pub(crate) const UNUSED: &str = "This clip plays none of the map's sounds. Hide unused is on.";

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::drums::presets::Preset;

    fn map() -> DrumMap {
        Preset::EIGHT_OH_EIGHT.map(Id(1))
    }

    #[test]
    fn the_lanes_are_the_maps_order_and_hide_the_sounds_the_clip_does_not_play() {
        let map = map();
        let kick = map.sounds[0].input;
        let hat = map.sounds[2].input;
        let notes = vec![
            Note::new(Ticks::ZERO, Ticks(60), kick, 100),
            Note::new(Ticks(120), Ticks(60), hat, 90),
        ];
        let all = lanes(&map, &notes, false);
        assert_eq!(all.len(), map.sounds.len(), "every sound, in map order");
        assert_eq!(
            all.iter().map(|l| l.input).collect::<Vec<_>>(),
            map.sounds.iter().map(|s| s.input).collect::<Vec<_>>()
        );
        let used: Vec<u8> = all.iter().filter(|l| l.used).map(|l| l.input).collect();
        assert_eq!(used, vec![kick, hat]);
        let few = lanes(&map, &notes, true);
        assert_eq!(few.len(), 2);
        assert_eq!(few.iter().map(|l| l.input).collect::<Vec<_>>(), [kick, hat]);
        // A note outside the clip's window still counts: trimming over a hit
        // must not lose its lane.
        let outside = vec![Note::new(Ticks(90_000), Ticks(60), kick, 100)];
        assert_eq!(lanes(&map, &outside, true).len(), 1);
        // Nothing played: no lanes at all when hiding the unused.
        assert!(lanes(&map, &[], true).is_empty());
    }

    #[test]
    fn a_random_run_of_lane_edits_is_the_edit_the_piano_roll_makes() {
        // The lanes commit through the same named document methods the roll
        // does (E5): the same edits in the same order leave bit-identical
        // notes and the same undo steps, whichever editor asked.
        use crate::document::Document;
        use lumen_project::edit::quantize::{Mode, Settings};
        use lumen_project::model::{Clip, ClipContent, Color, Track, TrackKind};
        use std::sync::Arc;

        let map = map();
        let mut next = 0x9e37_79b9_u64;
        let mut roll = move || {
            next ^= next << 13;
            next ^= next >> 7;
            next ^= next << 17;
            next
        };
        let notes: Vec<Note> = (0..40)
            .map(|_| {
                let sound = &map.sounds[(roll() % map.sounds.len() as u64) as usize];
                Note::new(
                    Ticks((roll() % 32) as i64 * 176_400),
                    Ticks(30 + (roll() % 300) as i64),
                    sound.input,
                    20 + (roll() % 100) as u8,
                )
            })
            .collect();
        let clip = Clip {
            id: Id(2),
            name: "Beat".into(),
            start: Ticks::ZERO,
            length: Ticks::quarters(8),
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
                notes: notes.clone(),
                events: Vec::new(),
            },
        };
        let build = || {
            let mut project = crate::empty_project();
            let mut track = Track::new(Id(1), "Drums", TrackKind::Midi, Color(0));
            track.drum_map = Some(map.id);
            track.clips = vec![Arc::new(clip.clone())];
            project.tracks.push(Arc::new(track));
            project.drum_maps = vec![map.clone()];
            Document::new(project)
        };
        let mut lanes = build();
        let mut piano = build();
        let settings = Settings {
            step: Ticks(176_400),
            mode: Mode::Start,
            strength: 0.75,
            swing: 0.2,
            window: 0.5,
        };
        for round in 0..30 {
            let picked: Vec<usize> = (0..notes.len()).filter(|_| roll() % 5 == 0).collect();
            match roll() % 5 {
                0 => {
                    let time = Ticks(((roll() % 9) as i64 - 4) * 176_400);
                    let pitch = (roll() % 5) as i32 - 2;
                    let base = lanes.project().clone();
                    lanes.move_notes(&base, clip.id, &picked, time, pitch, None);
                    let base = piano.project().clone();
                    piano.move_notes(&base, clip.id, &picked, time, pitch, None);
                }
                1 => {
                    let changes: Vec<(usize, u8)> = picked
                        .iter()
                        .map(|&i| (i, 1 + (roll() % 127) as u8))
                        .collect();
                    lanes.set_velocities_each(None, clip.id, &changes, None);
                    piano.set_velocities_each(None, clip.id, &changes, None);
                }
                2 => {
                    let note = Note::new(
                        Ticks((roll() % 30) as i64 * 176_400),
                        Ticks(240),
                        map.sounds[(roll() % map.sounds.len() as u64) as usize].input,
                        1 + (roll() % 127) as u8,
                    );
                    lanes.add_note(None, clip.id, note, None);
                    piano.add_note(None, clip.id, note, None);
                }
                3 => {
                    let by = Ticks(((roll() % 5) as i64 - 2) * 176_400);
                    lanes.nudge_notes(clip.id, &picked, by);
                    piano.nudge_notes(clip.id, &picked, by);
                }
                _ => {
                    lanes.quantize_notes(clip.id, &picked, &settings);
                    piano.quantize_notes(clip.id, &picked, &settings);
                }
            }
            let held = |doc: &Document| {
                doc.project()
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .find(|c| c.id == clip.id)
                    .and_then(|c| match &c.content {
                        ClipContent::Midi { notes, .. } => Some(notes.clone()),
                        _ => None,
                    })
                    .expect("the clip")
            };
            assert_eq!(held(&lanes), held(&piano), "round {round}");
            assert_eq!(
                lanes.history().steps().0.last(),
                piano.history().steps().0.last(),
                "round {round}: the same undo step"
            );
        }
    }

    #[test]
    fn the_state_keeps_a_view_and_hide_unused_per_track_and_lengths_per_map() {
        let mut state = State::default();
        let (a, b) = (Id(10), Id(11));
        assert_eq!(state.view(a), View::Lanes, "a drum clip opens in Lanes");
        state.set_view(a, View::Keys);
        assert_eq!(state.view(a), View::Keys);
        assert_eq!(state.view(b), View::Lanes, "another track is its own");
        state.set_view(a, View::Lanes);
        assert_eq!(state.view(a), View::Lanes);
        assert!(!state.hides_unused(a));
        assert!(state.toggle_unused(a));
        assert!(state.hides_unused(a) && !state.hides_unused(b));
        assert!(!state.toggle_unused(a));
        // The length of a lane belongs to its map.
        assert!(!state.shows_length(Id(1), 36));
        assert!(state.toggle_length(Id(1), 36));
        assert!(state.shows_length(Id(1), 36));
        assert!(!state.shows_length(Id(2), 36), "another map starts hidden");
        state.toggle_length(Id(2), 38);
        assert!(!state.shows_length(Id(1), 36), "the new map took over");
        assert!(state.shows_length(Id(2), 38));
    }
}
