//! Reading a Standard MIDI File.

use std::collections::BTreeMap;
use std::fmt;

use lumen_core::time::{MAX_BPM, MIN_BPM, PPQ, TempoPoint, Ticks};
use midly::{Format, MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

use super::{FileTrack, MidiFile};
use crate::model::{ClipEvent, EventKind, Note, TimeSignature};

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// Not a MIDI file, or damaged past what lenient reading accepts.
    Malformed(String),
    /// A type 2 file: its tracks are separate songs, not parts of one.
    Sequential,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Malformed(why) => write!(f, "not a readable MIDI file ({why})"),
            ReadError::Sequential => {
                f.write_str("a type 2 MIDI file (separate songs), which Lumen does not read")
            }
        }
    }
}

impl std::error::Error for ReadError {}

/// The tempo SMF assumes until a file sets one.
const DEFAULT_MICROS: u32 = 500_000;

/// Reads a `.mid` file's bytes. Ticks are rescaled to [`PPQ`]: the common
/// divisions (96 to 960 and others dividing 1,411,200) come in exactly,
/// others round to the nearest tick. SMPTE-timed files are converted
/// through their own tempo.
///
/// A note-on with velocity 0 is a note-off; a note-on over a sounding note
/// of the same key ends the first; notes still on at the end of their track
/// end there. A bank select (CC 0 and 32) at the same tick as a program
/// change on its channel becomes the change's bank.
pub fn read(bytes: &[u8]) -> Result<MidiFile, ReadError> {
    let smf = Smf::parse(bytes).map_err(|e| ReadError::Malformed(e.to_string()))?;
    if smf.header.format == Format::Sequential {
        return Err(ReadError::Sequential);
    }
    // Every event's absolute tick, per track.
    let tracks: Vec<Vec<(u64, TrackEventKind<'_>)>> = smf
        .tracks
        .iter()
        .map(|track| {
            let mut at = 0_u64;
            track
                .iter()
                .map(|e| {
                    at += u64::from(e.delta.as_int());
                    (at, e.kind)
                })
                .collect()
        })
        .collect();

    let mut changes: Vec<(u64, u32)> = Vec::new();
    let mut meter: Option<(u64, TimeSignature)> = None;
    for track in &tracks {
        for &(at, kind) in track {
            match kind {
                TrackEventKind::Meta(MetaMessage::Tempo(micros)) if micros.as_int() > 0 => {
                    changes.push((at, micros.as_int()));
                }
                TrackEventKind::Meta(MetaMessage::TimeSignature(n, d, _, _)) => {
                    if let Some(sig) = signature(n, d)
                        && meter.is_none_or(|(first, _)| at < first)
                    {
                        meter = Some((at, sig));
                    }
                }
                _ => {}
            }
        }
    }
    // Sorted by tick, the file's order kept at the same tick; the last wins.
    changes.sort_by_key(|&(at, _)| at);
    changes.dedup_by(|later, earlier| {
        if later.0 == earlier.0 {
            earlier.1 = later.1;
            true
        } else {
            false
        }
    });
    let base = Timebase::new(smf.header.timing, &changes)?;

    let mut tempo: Vec<TempoPoint> = changes
        .iter()
        .map(|&(at, micros)| TempoPoint {
            at: base.at(at),
            bpm: bpm(micros),
        })
        .collect();
    tempo.dedup_by(|later, earlier| {
        if later.at == earlier.at {
            earlier.bpm = later.bpm;
            true
        } else {
            false
        }
    });
    if tempo.first().is_some_and(|p| p.at > Ticks::ZERO) {
        tempo.insert(
            0,
            TempoPoint {
                at: Ticks::ZERO,
                bpm: bpm(DEFAULT_MICROS),
            },
        );
    }

    let mut parts = Vec::new();
    for (source, track) in tracks.iter().enumerate() {
        parts.extend(read_track(source, track, &base));
    }
    Ok(MidiFile {
        tracks: parts,
        tempo,
        meter: meter.map(|(_, sig)| sig),
    })
}

/// One file track's parts, one per channel used, in channel order.
fn read_track(
    source: usize,
    track: &[(u64, TrackEventKind<'_>)],
    base: &Timebase,
) -> Vec<FileTrack> {
    let mut name: Option<String> = None;
    let mut parts: BTreeMap<u8, Part> = BTreeMap::new();
    let end = track.last().map_or(0, |&(at, _)| at);
    for &(at, kind) in track {
        match kind {
            TrackEventKind::Meta(MetaMessage::TrackName(bytes)) if name.is_none() => {
                let text = String::from_utf8_lossy(bytes).trim().to_string();
                if !text.is_empty() {
                    name = Some(text);
                }
            }
            TrackEventKind::Midi { channel, message } => {
                parts
                    .entry(channel.as_int())
                    .or_default()
                    .take(at, message, base);
            }
            _ => {}
        }
    }
    parts
        .into_iter()
        .filter_map(|(channel, mut part)| {
            part.close(end, base);
            if part.notes.is_empty() && part.events.is_empty() {
                return None;
            }
            part.notes
                .sort_by_key(|n| (n.start(), n.pitch(), n.length()));
            Some(FileTrack {
                name: name.clone(),
                channel,
                source,
                notes: part.notes,
                events: part.events,
            })
        })
        .collect()
}

/// One channel of a track being read.
#[derive(Default)]
struct Part {
    /// Per key, the note sounding: where it began and its velocity.
    sounding: BTreeMap<u8, (u64, u8)>,
    notes: Vec<Note>,
    events: Vec<ClipEvent>,
    /// A bank select waiting for a program change at its tick: the tick,
    /// MSB, LSB, and where their controller events are in `events`.
    bank: Option<Bank>,
}

#[derive(Default, Clone, Copy)]
struct Bank {
    at: u64,
    msb: Option<(u8, usize)>,
    lsb: Option<(u8, usize)>,
}

impl Part {
    fn take(&mut self, at: u64, message: MidiMessage, base: &Timebase) {
        match message {
            MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                let key = key.as_int();
                self.end_note(key, at, base);
                self.sounding.insert(key, (at, vel.as_int()));
            }
            MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                self.end_note(key.as_int(), at, base);
            }
            MidiMessage::Controller { controller, value } => {
                let (number, value) = (controller.as_int(), value.as_int());
                let index = self.events.len();
                self.event(at, EventKind::Controller { number, value }, base);
                if number == 0 || number == 32 {
                    let mut bank = self.bank.filter(|b| b.at == at).unwrap_or(Bank {
                        at,
                        ..Bank::default()
                    });
                    if number == 0 {
                        bank.msb = Some((value, index));
                    } else {
                        bank.lsb = Some((value, index));
                    }
                    self.bank = Some(bank);
                }
            }
            MidiMessage::ProgramChange { program } => {
                let bank = self.bank.take().filter(|b| b.at == at).map(|b| {
                    let mut taken: Vec<usize> = [b.msb, b.lsb]
                        .into_iter()
                        .flatten()
                        .map(|(_, i)| i)
                        .collect();
                    taken.sort_unstable();
                    for i in taken.into_iter().rev() {
                        self.events.remove(i);
                    }
                    u16::from(b.msb.map_or(0, |(v, _)| v)) << 7
                        | u16::from(b.lsb.map_or(0, |(v, _)| v))
                });
                let program = program.as_int();
                self.event(at, EventKind::Program { program, bank }, base);
            }
            MidiMessage::PitchBend { bend } => {
                self.event(
                    at,
                    EventKind::PitchBend {
                        value: bend.as_int(),
                    },
                    base,
                );
            }
            MidiMessage::ChannelAftertouch { vel } => {
                self.event(
                    at,
                    EventKind::Pressure {
                        value: vel.as_int(),
                    },
                    base,
                );
            }
            // Polyphonic aftertouch has no place in the model.
            MidiMessage::Aftertouch { .. } => {}
        }
    }

    fn event(&mut self, at: u64, kind: EventKind, base: &Timebase) {
        self.events.push(ClipEvent {
            at: base.at(at),
            kind,
        });
    }

    /// Ends `key`'s sounding note, if any, at `at`.
    fn end_note(&mut self, key: u8, at: u64, base: &Timebase) {
        let Some((start, velocity)) = self.sounding.remove(&key) else {
            return;
        };
        let begin = base.at(start);
        // A note on and off at one tick is a trigger: it lasts one tick of
        // the file.
        let end = base.at(at.max(start + 1));
        let length = Ticks((end - begin).0.max(1));
        self.notes.push(Note::new(begin, length, key, velocity));
    }

    /// Ends every sounding note at the track's end.
    fn close(&mut self, end: u64, base: &Timebase) {
        let keys: Vec<u8> = self.sounding.keys().copied().collect();
        for key in keys {
            self.end_note(key, end, base);
        }
    }
}

/// A file's clock: its ticks to Lumen's.
enum Timebase {
    /// Ticks per quarter note.
    Beats(i64),
    /// Ticks per second, and the tempo in seconds: where each tempo starts,
    /// in seconds and in quarters, and its quarters per second.
    Seconds {
        per_second: f64,
        segments: Vec<(f64, f64, f64)>,
    },
}

impl Timebase {
    fn new(timing: Timing, changes: &[(u64, u32)]) -> Result<Self, ReadError> {
        match timing {
            Timing::Metrical(division) => {
                let division = i64::from(division.as_int());
                if division == 0 {
                    return Err(ReadError::Malformed("a division of 0 ticks".into()));
                }
                Ok(Timebase::Beats(division))
            }
            Timing::Timecode(fps, per_frame) => {
                let per_second = f64::from(fps.as_f32()) * f64::from(per_frame);
                if per_second <= 0.0 {
                    return Err(ReadError::Malformed("0 ticks per frame".into()));
                }
                let mut segments = vec![(0.0, 0.0, quarters_per_second(DEFAULT_MICROS))];
                for &(at, micros) in changes {
                    let seconds = at as f64 / per_second;
                    let &(from, quarters, rate) = segments.last().expect("one segment at least");
                    let here = quarters + (seconds - from) * rate;
                    if seconds == from {
                        segments.pop();
                    }
                    segments.push((seconds, here, quarters_per_second(micros)));
                }
                Ok(Timebase::Seconds {
                    per_second,
                    segments,
                })
            }
        }
    }

    /// Where a file tick is, in Lumen's ticks, rounded to the nearest.
    fn at(&self, tick: u64) -> Ticks {
        match self {
            Timebase::Beats(division) => {
                let (tick, division) = (i128::from(tick), i128::from(*division));
                let scaled = (2 * tick * i128::from(PPQ) + division) / (2 * division);
                Ticks(i64::try_from(scaled).unwrap_or(i64::MAX))
            }
            Timebase::Seconds {
                per_second,
                segments,
            } => {
                let seconds = tick as f64 / per_second;
                let &(from, quarters, rate) = segments
                    .iter()
                    .rev()
                    .find(|s| s.0 <= seconds)
                    .unwrap_or(&segments[0]);
                Ticks(((quarters + (seconds - from) * rate) * PPQ as f64).round() as i64)
            }
        }
    }
}

fn quarters_per_second(micros: u32) -> f64 {
    1_000_000.0 / f64::from(micros)
}

/// Beats per minute from microseconds per quarter, inside Lumen's range.
fn bpm(micros: u32) -> f64 {
    (60_000_000.0 / f64::from(micros)).clamp(MIN_BPM, MAX_BPM)
}

/// A time signature from its meta event (the denominator as a power of 2),
/// if Lumen can show it.
fn signature(numerator: u8, power: u8) -> Option<TimeSignature> {
    (numerator >= 1 && power <= 6).then(|| TimeSignature {
        numerator,
        denominator: 1 << power,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_divisions_come_in_exactly_and_others_round_to_the_nearest_tick() {
        for division in [24_i64, 48, 96, 120, 192, 240, 384, 480, 960, 1920] {
            let base = Timebase::Beats(division);
            for tick in [0_u64, 1, 7, division as u64 * 4 + 3] {
                let exact = Ticks(tick as i64 * PPQ / division);
                assert_eq!(PPQ % division, 0);
                assert_eq!(base.at(tick), exact, "{tick} at {division}");
            }
        }
        // 1,411,200 / 1000 = 1411.2: one tick rounds down, three up.
        let base = Timebase::Beats(1000);
        assert_eq!(base.at(1), Ticks(1411));
        assert_eq!(base.at(3), Ticks(4234));
        assert_eq!(base.at(1000), Ticks(PPQ));
    }

    #[test]
    fn tempo_stays_in_lumens_range_and_odd_meters_are_skipped() {
        assert_eq!(bpm(500_000), 120.0);
        assert_eq!(bpm(1), MAX_BPM);
        assert_eq!(bpm(0xFF_FFFF), MIN_BPM);
        assert_eq!(
            signature(6, 3),
            Some(TimeSignature {
                numerator: 6,
                denominator: 8
            })
        );
        assert_eq!(signature(0, 2), None);
        assert_eq!(signature(4, 7), None);
    }
}
