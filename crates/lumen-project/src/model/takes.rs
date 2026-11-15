use serde::{Deserialize, Serialize};

use super::{Clip, ClipContent, ClipEvent, Note, Warp};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Takes {
    pub active: u32,
    pub count: u32,
    pub others: Vec<OtherTake>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OtherTake {
    pub take: u32,
    #[serde(flatten)]
    pub pass: Pass,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Pass {
    Audio {
        offset: i64,
        #[serde(default, skip_serializing_if = "is_zero")]
        audio_phase_seconds: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        warp: Option<Warp>,
    },
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
    pub fn of(clip: &Clip) -> Option<Pass> {
        match &clip.content {
            ClipContent::Audio { offset, .. } => Some(Pass::Audio {
                offset: *offset,
                audio_phase_seconds: clip.audio_phase_seconds,
                warp: clip.warp.clone(),
            }),
            ClipContent::Midi { notes, events } => Some(Pass::Midi {
                notes: notes.clone(),
                events: events.clone(),
            }),
        }
    }

    pub fn fits(&self, clip: &Clip) -> bool {
        matches!(
            (self, &clip.content),
            (Pass::Audio { .. }, ClipContent::Audio { .. })
                | (Pass::Midi { .. }, ClipContent::Midi { .. })
        )
    }

    pub fn put_on(self, clip: &mut Clip) -> bool {
        match (self, &mut clip.content) {
            (
                Pass::Audio {
                    offset,
                    audio_phase_seconds,
                    warp,
                },
                ClipContent::Audio { offset: own, .. },
            ) => {
                *own = offset;
                clip.audio_phase_seconds = audio_phase_seconds;
                clip.warp = warp;
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
    pub fn new(active: u32, count: u32, mut others: Vec<OtherTake>) -> Self {
        others.sort_by_key(|o| o.take);
        Takes {
            active,
            count,
            others,
        }
    }

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

    pub fn other(&self, take: u32) -> Option<&Pass> {
        self.others.iter().find(|o| o.take == take).map(|o| &o.pass)
    }
}
