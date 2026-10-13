//! A MIDI clip's notes as GPU box instances for the piano roll: one box
//! per note on the row of its pitch (the layer's row table puts pitch `p`
//! at `127 - p` rows from the top), in the track's colour, shaded by
//! velocity. Built only when the clip's content or the selection changes;
//! panning and zooming only move the view (ADR-0026).

use std::collections::BTreeSet;

use eframe::egui::Color32;
use lumen_core::time::Ticks;
use lumen_project::model::Note;
use lumen_render::{BoxInstance, beats, rgba};

use crate::theme;

/// Most boxes one clip builds, loop repeats included: a clip looped by a
/// tiny period over a long length must not ask for gigabytes.
pub(crate) const MAX_BOXES: usize = 600_000;
/// Most loop repeats walked, for the same reason.
const MAX_REPEATS: i64 = 100_000;

/// The opacity of a note at velocity 0; velocity adds the rest, so a note
/// is about 0.62 + 0.38 × velocity / 127 opaque (the owner's look A).
const VELOCITY_FLOOR: f32 = 0.62;
/// Repeats of a looped clip are drawn this much fainter than the content.
const GHOST: f32 = 0.5;
/// A selected note is this much brighter.
const SELECTED_LIGHTER: f32 = 0.3;
/// The 1-px top edge is this much lighter than the body.
const EDGE_LIGHTER: f32 = 0.4;

/// What the piano roll needs of a MIDI clip.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClipNotes<'a> {
    pub start: Ticks,
    pub length: Ticks,
    pub loop_length: Option<Ticks>,
    pub notes: &'a [Note],
    /// Notes (by index) that carry on into the clip across a seam with the
    /// clip before it (`edit::seams`): they start before the clip and are
    /// drawn from its start, as they sound.
    pub carried: &'a [usize],
}

/// One note placed on the timeline: `index` into the clip's notes, `rep`
/// the loop repeat it is in (0 is the content itself), and where it sits,
/// in quarter notes from the project's start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
    pub index: usize,
    pub rep: u32,
    pub start: f64,
    pub end: f64,
}

impl ClipNotes<'_> {
    /// The ticks one repeat lasts: the loop's length, or the whole clip.
    fn period(&self) -> i64 {
        self.loop_length
            .map(|p| p.0)
            .filter(|&p| p > 0)
            .unwrap_or(self.length.0)
            .max(1)
    }

    /// Whether note `index` is muted.
    pub fn is_muted(&self, index: usize) -> bool {
        self.notes.get(index).is_some_and(|n| n.muted())
    }

    /// Every note as it plays, the way the engine reads the clip
    /// (`midi/hold.rs`): each repeat plays the notes that start inside it,
    /// cut at the repeat's end, and at least a tick long; notes carried on
    /// into the clip play from its start. Calls `f` for each, repeat by
    /// repeat, until [`MAX_BOXES`].
    pub fn each_placed(&self, mut f: impl FnMut(Placed, Note)) {
        if self.length.0 <= 0 {
            return;
        }
        let period = self.period();
        let count = ((self.length.0 + period - 1) / period).min(MAX_REPEATS);
        let mut made = 0;
        for &index in self.carried {
            let Some(&note) = self.notes.get(index) else {
                continue;
            };
            let end = note.end().0.min(period.min(self.length.0)).max(1);
            f(
                Placed {
                    index,
                    rep: 0,
                    start: self.start.as_quarters(),
                    end: Ticks(self.start.0 + end).as_quarters(),
                },
                note,
            );
            made += 1;
        }
        for rep in 0..count {
            let base = self.start.0 + rep * period;
            let span = period.min(self.length.0 - rep * period);
            for (index, &note) in self.notes.iter().enumerate() {
                let at = note.start().0;
                if at < 0 || at >= span {
                    continue;
                }
                let end = (at + note.length().0).min(span).max(at + 1);
                f(
                    Placed {
                        index,
                        rep: rep as u32,
                        start: Ticks(base + at).as_quarters(),
                        end: Ticks(base + end).as_quarters(),
                    },
                    note,
                );
                made += 1;
                if made >= MAX_BOXES {
                    return;
                }
            }
        }
    }
}

/// The colours notes are drawn in, from the theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NoteColors {
    /// The track's (or clip's) colour.
    pub track: Color32,
    /// Muted notes.
    pub muted: Color32,
    /// The outline of a selected note.
    pub outline: Color32,
}

impl NoteColors {
    /// The colours for notes in `track` colour.
    pub fn new(track: Color32) -> Self {
        Self {
            track,
            muted: theme::CLIP_MUTED,
            outline: theme::p().text,
        }
    }
}

/// A colour with straight alpha `alpha` (0 to 1), as the GPU takes it.
pub(crate) fn pack(c: Color32, alpha: f32) -> u32 {
    let [r, g, b, _] = c.to_array();
    rgba(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The opacity of a note struck at `velocity`.
pub(crate) fn velocity_alpha(velocity: u8) -> f32 {
    VELOCITY_FLOOR + (1.0 - VELOCITY_FLOOR) * f32::from(velocity.min(127)) / 127.0
}

fn lighter(c: Color32, by: f32) -> Color32 {
    c.lerp_to_gamma(Color32::WHITE, by)
}

/// One note's box. `row` is the pitch.
fn note_box(
    placed: Placed,
    note: Note,
    selected: bool,
    muted: bool,
    colors: &NoteColors,
) -> BoxInstance {
    let base = if muted { colors.muted } else { colors.track };
    let ghost = if placed.rep > 0 { GHOST } else { 1.0 };
    let (body, alpha) = if selected {
        // Brighter: lighter on the dark theme; on the light one, where
        // lighter would wash out, the colour at full strength.
        let lit = if theme::is_light() {
            base
        } else {
            lighter(base, SELECTED_LIGHTER)
        };
        (lit, 1.0)
    } else {
        (base, velocity_alpha(note.velocity()))
    };
    BoxInstance {
        start: beats(placed.start),
        end: beats(placed.end),
        row: u32::from(note.pitch().min(127)),
        body: pack(body, alpha * ghost),
        // The 1-px lighter top edge (the layer's title bar is 1 px).
        bar: pack(lighter(body, EDGE_LIGHTER), (alpha + 0.2).min(1.0) * ghost),
        border: if selected {
            pack(colors.outline, 1.0)
        } else {
            0
        },
        fill: 0,
        progress: 0.0,
    }
}

/// The boxes of a clip's notes: repeats first (fainter), then the
/// content, then the selected notes last so their outlines lie on top.
/// `selected` holds note indices.
pub(crate) fn note_instances(
    clip: &ClipNotes<'_>,
    selected: &BTreeSet<usize>,
    colors: &NoteColors,
) -> Vec<BoxInstance> {
    let mut ghosts = Vec::new();
    let mut content = Vec::with_capacity(clip.notes.len());
    let mut chosen = Vec::new();
    clip.each_placed(|placed, note| {
        let is_selected = placed.rep == 0 && selected.contains(&placed.index);
        let instance = note_box(
            placed,
            note,
            is_selected,
            clip.is_muted(placed.index),
            colors,
        );
        match (placed.rep, is_selected) {
            (0, true) => chosen.push(instance),
            (0, false) => content.push(instance),
            _ => ghosts.push(instance),
        }
    });
    ghosts.append(&mut content);
    ghosts.append(&mut chosen);
    ghosts
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::PPQ;

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn clip(notes: &[Note]) -> ClipNotes<'_> {
        ClipNotes {
            start: q(8),
            length: q(4),
            loop_length: None,
            notes,
            carried: &[],
        }
    }

    fn colors() -> NoteColors {
        NoteColors::new(Color32::from_rgb(0x4F, 0x81, 0xBD))
    }

    fn start_of(b: &BoxInstance) -> f64 {
        f64::from(b.start[0]) + f64::from(b.start[1])
    }
    fn end_of(b: &BoxInstance) -> f64 {
        f64::from(b.end[0]) + f64::from(b.end[1])
    }
    fn alpha(b: &BoxInstance) -> u32 {
        b.body >> 24
    }

    #[test]
    fn notes_sit_at_the_clip_start_on_their_pitch_row() {
        let notes = [
            Note::new(q(1), q(2), 60, 100),
            Note::new(q(0), Ticks(PPQ / 2), 72, 127),
        ];
        let built = note_instances(&clip(&notes), &BTreeSet::new(), &colors());
        assert_eq!(built.len(), 2);
        assert_eq!((start_of(&built[0]), end_of(&built[0])), (9.0, 11.0));
        assert_eq!(built[0].row, 60);
        assert_eq!((start_of(&built[1]), end_of(&built[1])), (8.0, 8.5));
        assert_eq!(built[1].row, 72);
    }

    #[test]
    fn notes_are_cut_at_the_clip_end_and_dropped_after_it() {
        let notes = [
            Note::new(q(3), q(4), 60, 100),
            Note::new(q(4), q(1), 61, 100),
            Note::new(q(5), q(1), 62, 100),
        ];
        let built = note_instances(&clip(&notes), &BTreeSet::new(), &colors());
        assert_eq!(built.len(), 1, "notes from the clip's end on do not play");
        assert_eq!(end_of(&built[0]), 12.0);
    }

    #[test]
    fn a_zero_length_note_is_a_tick_long() {
        let notes = [Note::new(q(1), Ticks(0), 60, 100)];
        let built = note_instances(&clip(&notes), &BTreeSet::new(), &colors());
        assert!(end_of(&built[0]) > start_of(&built[0]));
    }

    #[test]
    fn loops_repeat_the_content_fainter_and_cut_the_last_repeat() {
        let notes = [
            Note::new(q(0), q(1), 60, 127),
            Note::new(q(1), q(2), 64, 127),
        ];
        let mut c = clip(&notes);
        c.length = q(5);
        c.loop_length = Some(q(2));
        let built = note_instances(&c, &BTreeSet::new(), &colors());
        // Repeats 0 and 1 hold both notes (the second cut at the period);
        // repeat 2 lasts one beat: only the first note starts in it.
        assert_eq!(built.len(), 5);
        // The ghosts come first, fainter than the content.
        assert!(alpha(&built[0]) < alpha(built.last().unwrap()));
        let last_repeat: Vec<_> = built.iter().filter(|b| start_of(b) >= 12.0).collect();
        assert_eq!(last_repeat.len(), 1);
        assert_eq!(end_of(last_repeat[0]), 13.0);
        let second_cut = built
            .iter()
            .find(|b| b.row == 64 && start_of(b) == 11.0)
            .unwrap();
        assert_eq!(end_of(second_cut), 12.0, "cut at the end of its repeat");
    }

    #[test]
    fn selected_notes_come_last_outlined_and_brighter() {
        let notes = [Note::new(q(0), q(1), 60, 64), Note::new(q(1), q(1), 61, 64)];
        let selected: BTreeSet<usize> = [0].into();
        let built = note_instances(&clip(&notes), &selected, &colors());
        assert_eq!(built[0].row, 61);
        assert_eq!(built[0].border, 0, "unselected notes have no outline");
        assert_eq!(built[1].row, 60);
        assert_ne!(built[1].border, 0);
        assert_eq!(alpha(&built[1]), 255, "selected notes are fully opaque");
        assert_ne!(built[1].body, built[0].body);
    }

    #[test]
    fn repeats_are_never_selected() {
        let notes = [Note::new(q(0), q(1), 60, 64)];
        let mut c = clip(&notes);
        c.loop_length = Some(q(2));
        let selected: BTreeSet<usize> = [0].into();
        let built = note_instances(&c, &selected, &colors());
        assert_eq!(built.len(), 2);
        assert_eq!(built.iter().filter(|b| b.border != 0).count(), 1);
    }

    #[test]
    fn muted_notes_are_grey_and_velocity_shades_the_rest() {
        let notes = [
            Note::new(q(0), q(1), 60, 127),
            Note(q(1), q(1), 61, 127, true),
            Note::new(q(2), q(1), 62, 0),
        ];
        let built = note_instances(&clip(&notes), &BTreeSet::new(), &colors());
        let [r, g, b, _] = theme::CLIP_MUTED.to_array();
        assert_eq!(built[1].body & 0x00FF_FFFF, rgba(r, g, b, 0));
        assert_ne!(built[0].body & 0x00FF_FFFF, built[1].body & 0x00FF_FFFF);
        assert_eq!(alpha(&built[0]), 255);
        assert!((alpha(&built[2]) as f32 / 255.0 - VELOCITY_FLOOR).abs() < 0.01);
    }

    #[test]
    fn a_huge_loop_count_is_capped() {
        let notes = [Note::new(q(0), Ticks(1), 60, 64)];
        let mut c = clip(&notes);
        c.length = Ticks(i64::MAX / 4);
        c.loop_length = Some(Ticks(1));
        let built = note_instances(&c, &BTreeSet::new(), &colors());
        assert_eq!(built.len(), MAX_REPEATS as usize);
    }

    #[test]
    fn a_hundred_thousand_notes_build_quickly() {
        let notes: Vec<Note> = (0..100_000)
            .map(|i| Note::new(Ticks(i * 1000), Ticks(900), (i % 128) as u8, 90))
            .collect();
        let mut c = clip(&notes);
        c.length = Ticks(100_000 * 1000);
        let t = std::time::Instant::now();
        let built = note_instances(&c, &BTreeSet::new(), &colors());
        assert_eq!(built.len(), 100_000);
        assert!(t.elapsed().as_millis() < 2_000);
    }
}
