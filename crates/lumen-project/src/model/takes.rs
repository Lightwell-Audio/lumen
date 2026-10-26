//! Takes (format v31, M9 slice R3): the passes of a loop recording that a
//! clip keeps besides the one it plays.
//!
//! A clip that was recorded with Loop on has one *active* pass, which is the
//! clip's own content, so editing it never leaves a stale copy. The other
//! passes are held here, by number. Playing another pass is a swap
//! (`edit::takes`): the clip's current pass goes into the slot of the number
//! it had, and the chosen one becomes the clip's content. Playback sees an
//! ordinary clip.
//!
//! Passes are numbered from 0 here and shown from 1.

use serde::{Deserialize, Serialize};

use super::{Clip, ClipContent, ClipEvent, Note};

/// The passes a clip keeps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Takes {
    /// The number of the pass the clip plays.
    pub active: u32,
    /// How many passes there are, the active one included.
    pub count: u32,
    /// The other passes, in order of number.
    pub others: Vec<OtherTake>,
}

/// One pass the clip does not play now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OtherTake {
    /// Its number, from 0.
    pub take: u32,
    #[serde(flatten)]
    pub pass: Pass,
}

/// What distinguishes one pass from another on a clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Pass {
    /// Where the clip's start falls in the file: the pass is a window on the
    /// same media. `offset` and `audio_phase_seconds` are the clip's own
    /// fields of those names; an earlier pass than the file's first frame
    /// has a negative `audio_phase_seconds`, which plays silence.
    Audio {
        offset: i64,
        #[serde(default, skip_serializing_if = "is_zero")]
        audio_phase_seconds: f64,
    },
    /// The notes and events of the pass, relative to the clip's start.
    Midi {
        notes: Vec<Note>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        events: Vec<ClipEvent>,
    },
}

fn is_zero(seconds: &f64) -> bool {
    *seconds == 0.0
}

impl Pass {
    /// The pass a clip plays now, or `None` for content that has no passes.
    pub fn of(clip: &Clip) -> Option<Pass> {
        match &clip.content {
            ClipContent::Audio { offset, .. } => Some(Pass::Audio {
                offset: *offset,
                audio_phase_seconds: clip.audio_phase_seconds,
            }),
            ClipContent::Midi { notes, events } => Some(Pass::Midi {
                notes: notes.clone(),
                events: events.clone(),
            }),
        }
    }

    /// Whether the pass is of the kind of content `clip` has.
    pub fn fits(&self, clip: &Clip) -> bool {
        matches!(
            (self, &clip.content),
            (Pass::Audio { .. }, ClipContent::Audio { .. })
                | (Pass::Midi { .. }, ClipContent::Midi { .. })
        )
    }

    /// Makes `clip` play this pass. Returns false, changing nothing, when the
    /// kind of pass does not match the clip.
    pub fn put_on(self, clip: &mut Clip) -> bool {
        match (self, &mut clip.content) {
            (
                Pass::Audio {
                    offset,
                    audio_phase_seconds,
                },
                ClipContent::Audio { offset: own, .. },
            ) => {
                *own = offset;
                clip.audio_phase_seconds = audio_phase_seconds;
                true
            }
            (
                Pass::Midi { notes, events },
                ClipContent::Midi {
                    notes: own_notes,
                    events: own_events,
                },
            ) => {
                *own_notes = notes;
                *own_events = events;
                true
            }
            _ => false,
        }
    }
}

impl Takes {
    /// A set of `count` passes, `active` of them played, holding the others.
    /// `others` need not be sorted.
    pub fn new(active: u32, count: u32, mut others: Vec<OtherTake>) -> Self {
        others.sort_by_key(|o| o.take);
        Takes {
            active,
            count,
            others,
        }
    }

    /// Whether the numbers describe a set: every number below `count`, each
    /// once, the active one not among the others, and a pass of one kind.
    pub fn is_sound(&self) -> bool {
        let mut seen = vec![false; self.count as usize];
        if self.active >= self.count || self.others.len() + 1 != self.count as usize {
            return false;
        }
        seen[self.active as usize] = true;
        let audio = self
            .others
            .first()
            .map(|o| matches!(o.pass, Pass::Audio { .. }));
        for other in &self.others {
            let Some(slot) = seen.get_mut(other.take as usize) else {
                return false;
            };
            if std::mem::replace(slot, true)
                || Some(matches!(other.pass, Pass::Audio { .. })) != audio
            {
                return false;
            }
        }
        true
    }

    /// The pass held under `take`.
    pub fn other(&self, take: u32) -> Option<&Pass> {
        self.others.iter().find(|o| o.take == take).map(|o| &o.pass)
    }
}
