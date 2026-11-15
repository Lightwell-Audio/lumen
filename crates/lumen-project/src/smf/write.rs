use std::fmt;

use lumen_core::time::{PPQ, TempoMap, Ticks};
use midly::num::{u4, u7, u15, u24, u28};
use midly::{
    Format, Header, MetaMessage, MidiMessage, PitchBend, Smf, Timing, TrackEvent, TrackEventKind,
};

use super::{FileTrack, MidiFile};
use crate::model::EventKind;

pub const DIVISION: u16 = 960;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Single,
    Parallel,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Meta,
    Off,
    Control,
    On,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    TooSparse,
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::TooSparse => f.write_str(
                "two events are further apart than a MIDI file can hold (about 78 hours)",
            ),
        }
    }
}

impl std::error::Error for WriteError {}

type Timed<'a> = (u64, Rank, TrackEventKind<'a>);

pub fn write(file: &MidiFile, layout: Layout) -> Result<Vec<u8>, WriteError> {
    let mut conductor: Vec<Timed<'static>> = Vec::new();
    let steps = if file.tempo.iter().any(|p| p.ramp) {
        TempoMap::new(file.tempo.clone()).map_or_else(|_| file.tempo.clone(), |map| map.stepped())
    } else {
        file.tempo.clone()
    };
    for point in &steps {
        let micros = (60_000_000.0 / point.bpm)
            .round()
            .clamp(1.0, f64::from(0xFF_FFFF_u32));
        conductor.push((
            tick(point.at),
            Rank::Meta,
            TrackEventKind::Meta(MetaMessage::Tempo(u24::new(micros as u32))),
        ));
    }
    for point in &file.meter {
        let power = point.meter.denominator.max(1).trailing_zeros() as u8;
        let numerator = point.meter.numerator.min(u32::from(u8::MAX)) as u8;
        conductor.push((
            tick(point.at),
            Rank::Meta,
            TrackEventKind::Meta(MetaMessage::TimeSignature(numerator, power, 24, 8)),
        ));
    }
    let tracks: Vec<Vec<TrackEvent<'_>>> = match layout {
        Layout::Single => {
            let mut events = conductor;
            if let Some(name) = file.tracks.iter().find_map(|t| t.name.as_deref()) {
                events.push(name_event(name));
            }
            for part in &file.tracks {
                events.extend(part_events(part));
            }
            vec![finish(events)?]
        }
        Layout::Parallel => {
            let mut tracks = vec![finish(conductor)?];
            for part in &file.tracks {
                let mut events: Vec<Timed<'_>> =
                    part.name.as_deref().map(name_event).into_iter().collect();
                events.extend(part_events(part));
                tracks.push(finish(events)?);
            }
            tracks
        }
    };
    let format = match layout {
        Layout::Single => Format::SingleTrack,
        Layout::Parallel => Format::Parallel,
    };
    let smf = Smf {
        header: Header::new(format, Timing::Metrical(u15::new(DIVISION))),
        tracks,
    };
    let mut bytes = Vec::new();
    smf.write_std(&mut bytes)
        .expect("writing to a Vec cannot fail, and every count and length is in range");
    Ok(bytes)
}

fn name_event(name: &str) -> Timed<'_> {
    (
        0,
        Rank::Meta,
        TrackEventKind::Meta(MetaMessage::TrackName(name.as_bytes())),
    )
}

fn part_events(part: &FileTrack) -> Vec<Timed<'static>> {
    let channel = u4::new(part.channel);
    let midi = |message| TrackEventKind::Midi { channel, message };
    let mut out = Vec::with_capacity(part.notes.len() * 2 + part.events.len());
    for note in &part.notes {
        let key = u7::new(note.pitch());
        let on = tick(note.start());
        let off = tick(note.end()).max(on + 1);
        let vel = u7::new(note.velocity().max(1));
        out.push((on, Rank::On, midi(MidiMessage::NoteOn { key, vel })));
        out.push((
            off,
            Rank::Off,
            midi(MidiMessage::NoteOff {
                key,
                vel: u7::new(64),
            }),
        ));
    }
    for event in &part.events {
        let at = tick(event.at);
        match event.kind {
            EventKind::Controller { number, value } => {
                let (controller, value) = (u7::new(number), u7::new(value));
                out.push((
                    at,
                    Rank::Control,
                    midi(MidiMessage::Controller { controller, value }),
                ));
            }
            EventKind::PitchBend { value } => {
                let bend = PitchBend::from_int(value);
                out.push((at, Rank::Control, midi(MidiMessage::PitchBend { bend })));
            }
            EventKind::Pressure { value } => {
                let vel = u7::new(value);
                out.push((
                    at,
                    Rank::Control,
                    midi(MidiMessage::ChannelAftertouch { vel }),
                ));
            }
            EventKind::Program { program, bank } => {
                if let Some(bank) = bank {
                    for (number, value) in [(0, bank >> 7), (32, bank & 127)] {
                        let (controller, value) = (u7::new(number), u7::new(value as u8));
                        out.push((
                            at,
                            Rank::Control,
                            midi(MidiMessage::Controller { controller, value }),
                        ));
                    }
                }
                let program = u7::new(program);
                out.push((
                    at,
                    Rank::Control,
                    midi(MidiMessage::ProgramChange { program }),
                ));
            }
        }
    }
    out
}

fn finish(mut events: Vec<Timed<'_>>) -> Result<Vec<TrackEvent<'_>>, WriteError> {
    events.sort_by_key(|&(at, rank, _)| (at, rank));
    let mut last = 0;
    let mut track = Vec::with_capacity(events.len() + 1);
    for (at, _, kind) in events {
        let delta = u32::try_from(at - last)
            .ok()
            .and_then(u28::try_from)
            .ok_or(WriteError::TooSparse)?;
        last = at;
        track.push(TrackEvent { delta, kind });
    }
    track.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    Ok(track)
}

fn tick(at: Ticks) -> u64 {
    let step = PPQ / i64::from(DIVISION);
    let at = at.0.max(0);
    ((at + step / 2) / step) as u64
}
