//! A plugin instrument: the notes of a track as a host hears them.

use super::*;

/// The most events one hosted instrument takes to its host in a block; a
/// storm past it is dropped and counted.
pub(super) const EVENTS: usize = 2048;

/// Whether a message may make the plugin sound: everything but the one that
/// stops every sound at once.
fn wakes(msg: Msg) -> bool {
    !matches!(
        msg.kind(),
        Kind::Controller {
            number: cc::ALL_SOUND_OFF,
            ..
        }
    )
}

/// The notes held down and the channels with the sustain pedal down, which
/// say whether an instrument still sounds without a word being said. A held
/// note keeps the host given blocks even while its output is quiet (a slow
/// attack, a swell).
struct Holding {
    /// A bit for each channel and key.
    notes: [u64; 32],
    /// A bit for each channel with the pedal down.
    pedals: u16,
    /// How many notes are down: asked every block, so not counted from the
    /// bits each time.
    down: u32,
}

impl Holding {
    const NONE: Self = Self {
        notes: [0; 32],
        pedals: 0,
        down: 0,
    };

    fn any(&self) -> bool {
        self.down != 0 || self.pedals != 0
    }

    fn clear(&mut self) {
        *self = Self::NONE;
    }

    /// Lets go of one channel's notes and pedal: its 128 bits are two words.
    fn clear_channel(&mut self, channel: u8) {
        let first = usize::from(channel & 15) * 2;
        for word in &mut self.notes[first..first + 2] {
            self.down -= word.count_ones();
            *word = 0;
        }
        self.pedals &= !(1 << (channel & 15));
    }

    fn set(&mut self, channel: u8, key: u8, down: bool) {
        let bit = usize::from(channel & 15) * 128 + usize::from(key & 127);
        let (word, mask) = (bit / 64, 1_u64 << (bit % 64));
        let was = self.notes[word] & mask != 0;
        if down && !was {
            self.notes[word] |= mask;
            self.down += 1;
        } else if !down && was {
            self.notes[word] &= !mask;
            self.down -= 1;
        }
    }

    fn note(&mut self, msg: Msg) {
        let channel = msg.channel();
        match msg.kind() {
            Kind::NoteOn { key, .. } => self.set(channel, key, true),
            Kind::NoteOff { key, .. } => self.set(channel, key, false),
            Kind::Controller { number, value } => match number {
                cc::SUSTAIN if value >= 64 => self.pedals |= 1 << channel,
                cc::SUSTAIN => self.pedals &= !(1 << channel),
                // The plugin lets go of what that channel holds, pedal or not.
                cc::ALL_SOUND_OFF | cc::ALL_NOTES_OFF => self.clear_channel(channel),
                _ => {}
            },
            _ => {}
        }
    }
}

/// A plugin instrument in a host, played as an [`Instrument`] (M6 slice 2b).
/// The app builds one when it attaches a host, and a [`placeholder`] in every
/// later track list: the engine carries a track's instrument across lists by
/// its kind, so the real one stays and the placeholder is dropped.
///
/// [`placeholder`]: HostedInstrument::placeholder
pub struct HostedInstrument {
    pub(super) guard: Option<Guard>,
    /// Messages that arrived with no block to ride on, for the next block.
    pub(super) pending: Vec<Event>,
    /// The block's events, pending ones first.
    block: Vec<Event>,
    begun: bool,
    /// The early `begin` of this block was refused and its events are in
    /// `pending`: `render` retries the block without them.
    refused: bool,
    rate: u32,
    holding: Holding,
    dropped: u32,
}

impl HostedInstrument {
    /// Allocates; call off the audio thread.
    pub fn new(guard: Guard) -> Self {
        Self {
            guard: Some(guard),
            pending: Vec::with_capacity(EVENTS),
            block: Vec::with_capacity(EVENTS),
            begun: false,
            refused: false,
            rate: 0,
            holding: Holding::NONE,
            dropped: 0,
        }
    }

    /// A stand-in with no host, for a track list whose instrument is already
    /// attached. Allocates nothing.
    pub fn placeholder() -> Self {
        Self {
            guard: None,
            pending: Vec::new(),
            block: Vec::new(),
            begun: false,
            refused: false,
            rate: 0,
            holding: Holding::NONE,
            dropped: 0,
        }
    }

    /// Hands the host a block, with `events` and what is pending. `retry`:
    /// the block was refused by an early [`Instrument::begin`], which put its
    /// events in `pending`, so they are not added again.
    fn offer(&mut self, rate: u32, frames: usize, events: &[Event], retry: bool) -> bool {
        self.begun = false;
        if rate > 0 {
            self.rate = rate;
        }
        let rate = self.rate;
        let Some(guard) = self.guard.as_mut() else {
            return false;
        };
        // The other outputs are those of the block now begun, if any: not
        // those of the last one, when this one is refused or skipped.
        guard.forget_block();
        // A block refused early has its events in `pending`: they are not
        // said again.
        let events = if retry { &[] } else { events };
        let held = self.holding.any();
        if rate == 0
            || (events.is_empty() && self.pending.is_empty() && !held && !guard.sounding(rate))
        {
            // Nothing sounds and nothing is said: the host is left alone.
            return false;
        }
        self.block.clear();
        self.block.extend_from_slice(&self.pending);
        self.pending.clear();
        let room = EVENTS - self.block.len();
        self.block.extend(events.iter().take(room));
        self.dropped += events.len().saturating_sub(room) as u32;
        let sent = if retry {
            guard.begin_again(frames, &[], &self.block)
        } else {
            guard.begin(frames, &[], &self.block)
        };
        self.refused = !sent && !guard.failed();
        if self.refused {
            // The host is still on an earlier block: what was said goes with
            // the next one, at its start, rather than being lost (a lost
            // note-off would leave a note hanging).
            let said = self.block.iter().map(|e| Event {
                offset: 0,
                msg: e.msg,
            });
            self.pending.extend(said);
        }
        let mut woken = false;
        for event in &self.block {
            self.holding.note(event.msg);
            woken |= wakes(event.msg);
        }
        // What was said, and held notes and a held pedal, keep it sounding
        // block after block; after that, its output decides.
        if woken || self.holding.any() {
            guard.wake();
        }
        self.begun = sent;
        sent
    }

    /// Events dropped because a block had more than the host takes.
    pub fn dropped_events(&self) -> u32 {
        self.dropped
    }
}

impl Instrument for HostedInstrument {
    fn hosted(&self) -> bool {
        self.guard.is_some()
    }

    fn begin(&mut self, rate: u32, frames: usize, events: &[Event]) -> bool {
        self.offer(rate, frames, events, false)
    }

    fn output(&mut self, pair: usize, left: &mut [f32], right: &mut [f32]) -> bool {
        self.guard
            .as_mut()
            .is_some_and(|guard| guard.output(pair, left, right))
    }

    fn finish(&mut self, deadline: Instant, left: &mut [f32], right: &mut [f32]) {
        if !std::mem::take(&mut self.begun) {
            return;
        }
        if let Some(guard) = self.guard.as_mut() {
            guard.finish(deadline, left, right);
        }
    }

    fn render(&mut self, rate: u32, events: &[Event], left: &mut [f32], right: &mut [f32]) {
        self.render_by(rate, events, left, right, None);
    }

    fn render_by(
        &mut self,
        rate: u32,
        events: &[Event],
        left: &mut [f32],
        right: &mut [f32],
        deadline: Option<Instant>,
    ) {
        let frames = left.len();
        if self.guard.is_none() {
            return;
        }
        if frames == 0 {
            // Messages only (a track let go of its notes): they go with the
            // next block.
            for event in events {
                if self.pending.len() < EVENTS {
                    self.pending.push(Event {
                        offset: 0,
                        msg: event.msg,
                    });
                } else {
                    self.dropped += 1;
                }
            }
            return;
        }
        let retry = std::mem::take(&mut self.refused);
        if self.offer(rate, frames, events, retry) {
            let deadline = deadline.unwrap_or_else(|| {
                let rate = u64::from(self.rate.max(1));
                Instant::now() + Duration::from_micros(frames as u64 * 750_000 / rate)
            });
            self.finish(deadline, left, right);
        }
    }

    fn active(&self) -> bool {
        // Messages waiting for a block count: a note-off must reach the host
        // even when the project has stopped.
        self.guard.as_ref().is_some_and(|g| {
            !g.failed() && (!self.pending.is_empty() || self.holding.any() || g.sounding(self.rate))
        })
    }

    fn silence(&mut self) {
        self.holding.clear();
        let Some(guard) = self.guard.as_mut() else {
            return;
        };
        guard.hush();
        if self.pending.len() < EVENTS {
            self.pending.push(Event {
                offset: 0,
                msg: Msg::controller(0, cc::ALL_SOUND_OFF, 0),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_notes_off_on_one_channel_lets_go_of_that_channels_notes_only() {
        let mut held = Holding::NONE;
        held.note(Msg::note_on(0, 60, 100));
        held.note(Msg::note_on(0, 64, 100));
        held.note(Msg::note_on(5, 60, 100));
        held.note(Msg::controller(5, cc::SUSTAIN, 127));
        held.note(Msg::controller(0, cc::SUSTAIN, 127));
        assert_eq!(held.down, 3);
        held.note(Msg::controller(0, cc::ALL_NOTES_OFF, 0));
        assert_eq!(held.down, 1, "channel 5's note is still down");
        assert_eq!(held.pedals, 1 << 5, "and its pedal");
        assert!(held.any());
        held.note(Msg::controller(5, cc::ALL_SOUND_OFF, 0));
        assert_eq!(held.down, 0);
        assert!(!held.any());
        // Letting go of what is not held changes nothing.
        held.note(Msg::controller(9, cc::ALL_NOTES_OFF, 0));
        assert_eq!(held.down, 0);
    }
}
