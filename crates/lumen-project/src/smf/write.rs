//! Writing a Standard MIDI File.

use std::fmt;

use lumen_core::time::{PPQ, TempoMap, Ticks};
use midly::num::{u4, u7, u15, u24, u28};
use midly::{
    Format, Header, MetaMessage, MidiMessage, PitchBend, Smf, Timing, TrackEvent, TrackEventKind,
};

use super::{FileTrack, MidiFile};
use crate::model::EventKind;

/// Ticks per quarter note in written files: the most widely read division.
/// Lumen's ticks round to the nearest of these (at most half a step, about
/// 0.26 ms at 120 BPM).
pub const DIVISION: u16 = 960;

/// How a file's parts are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Type 0: one track holding everything, each part on its channel.
    Single,
    /// Type 1: a conductor track with the tempo map and time signature,
    /// then a track per part, named after it.
    Parallel,
}

/// The order of events at one tick: names and tempo first, then note-offs
/// (so a note ending where the next begins does not cut it), then
/// controllers and programs, then note-ons.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Meta,
    Off,
    Control,
    On,
}

/// Why a file could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// Two events lie further apart than a MIDI file can say: its delta
    /// times are 28 bits, so at [`DIVISION`] about 78 hours. Projects may
    /// reach much further, and such a gap has no event that fills it.
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

/// One event before deltas: its tick at [`DIVISION`], its rank, the event.
type Timed<'a> = (u64, Rank, TrackEventKind<'a>);

/// A file's bytes, at [`DIVISION`] ticks per quarter note, or why they
/// cannot be made.
pub fn write(file: &MidiFile, layout: Layout) -> Result<Vec<u8>, WriteError> {
    let mut conductor: Vec<Timed<'static>> = Vec::new();
    // A file holds steps only: a ramp is written as a run of short steps.
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

/// A part's notes and events on its channel.
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

/// Events sorted into a track, with its end. The sort is stable, so events
/// of one rank at one tick keep their order (a bank select before its
/// program change). A gap too long for a delta time is an error.
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

/// A position at [`DIVISION`], rounded to the nearest; never before zero.
fn tick(at: Ticks) -> u64 {
    let step = PPQ / i64::from(DIVISION);
    let at = at.0.max(0);
    ((at + step / 2) / step) as u64
}
