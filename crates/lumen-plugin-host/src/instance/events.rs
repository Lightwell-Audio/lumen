use super::*;

pub(crate) struct NoteIds {
    held: Vec<i32>,
    next: i32,
}

impl NoteIds {
    const CHANNELS: usize = 16;
    const PITCHES: usize = 128;

    pub(crate) fn new() -> Self {
        Self {
            held: vec![-1; Self::CHANNELS * Self::PITCHES],
            next: 0,
        }
    }

    fn slot(channel: i16, pitch: i16) -> Option<usize> {
        let (c, p) = (usize::try_from(channel).ok()?, usize::try_from(pitch).ok()?);
        (c < Self::CHANNELS && p < Self::PITCHES).then_some(c * Self::PITCHES + p)
    }

    pub(crate) fn on(&mut self, channel: i16, pitch: i16) -> i32 {
        let id = self.next;
        self.next = self.next.wrapping_add(1).max(0);
        if let Some(slot) = Self::slot(channel, pitch) {
            self.held[slot] = id;
        }
        id
    }

    pub(crate) fn off(&mut self, channel: i16, pitch: i16) -> i32 {
        Self::slot(channel, pitch).map_or(-1, |slot| std::mem::replace(&mut self.held[slot], -1))
    }

    pub(crate) fn clear(&mut self) {
        self.held.fill(-1);
    }
}

pub(super) const MAP_CHANNELS: usize = 16;
pub(super) const MAP_NUMBERS: usize = 130;
pub(super) const MAP_UNKNOWN: i64 = -2;
pub(super) const MAP_NONE: i64 = -1;

pub(super) fn controller_value(kind: &EventKind) -> Option<(i16, i16, f64)> {
    match *kind {
        EventKind::Controller {
            channel,
            number,
            value,
        } => Some((channel, number, f64::from(value.clamp(0.0, 1.0)))),
        EventKind::Pressure { channel, value } => {
            Some((channel, 128, f64::from(value.clamp(0.0, 1.0))))
        }
        EventKind::PitchBend { channel, value } => {
            Some((channel, 129, f64::from(value.clamp(0.0, 1.0))))
        }
        _ => None,
    }
}

pub(super) fn vst_event(event: &Event, ids: &mut NoteIds) -> Option<VstEvent> {
    let (kind, body) = match event.kind {
        EventKind::NoteOn {
            channel,
            pitch,
            velocity,
        } => (
            kNoteOnEvent,
            Event__type0 {
                noteOn: NoteOnEvent {
                    channel,
                    pitch,
                    tuning: 0.0,
                    velocity,
                    length: 0,
                    noteId: ids.on(channel, pitch),
                },
            },
        ),
        EventKind::NoteOff {
            channel,
            pitch,
            velocity,
        } => (
            kNoteOffEvent,
            Event__type0 {
                noteOff: NoteOffEvent {
                    channel,
                    pitch,
                    velocity,
                    noteId: ids.off(channel, pitch),
                    tuning: 0.0,
                },
            },
        ),
        _ => return None,
    };
    Some(VstEvent {
        busIndex: 0,
        sampleOffset: event.offset as i32,
        ppqPosition: 0.0,
        flags: 0,
        r#type: kind as u16,
        __field0: body,
    })
}
