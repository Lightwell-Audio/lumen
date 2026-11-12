//! The pair that plays MIDI: [`MidiHandle`] on the UI side replaces the
//! track list, [`MidiPlayer`] on the audio thread reads it (a
//! [`TrackSource`](crate::mixer::TrackSource), played beside the audio clips).

mod carry;
mod handle;
mod out;
mod part;
mod play;
mod set;
mod source;

pub use handle::MidiHandle;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use super::capture::{self, Captured};
use super::clock::ClockOut;
use super::hold::MAX_EVENTS;
use super::live::LiveInput;
use super::{MidiTrack, Target};

pub(in crate::midi) use out::Out;
use set::MidiSet;

const QUEUE: usize = 4;

/// A live input on its way to the audio thread, or back to be freed.
pub(super) type Feed = Box<dyn LiveInput>;

/// Audio side: plays the MIDI clips. A
/// [`TrackSource`](crate::mixer::TrackSource).
pub struct MidiPlayer {
    pub(super) incoming: rtrb::Consumer<Box<MidiSet>>,
    pub(super) retired: rtrb::Producer<Box<MidiSet>>,
    /// A replaced list that did not fit in the retired queue yet.
    pub(super) stash: Option<Box<MidiSet>>,
    pub(super) current: Box<MidiSet>,
    pub(super) out: Out,
    /// The device frame at the end of the last part rendered: when to
    /// stamp messages sent outside a part.
    pub(super) clock: u64,
    pub(super) feeds: rtrb::Consumer<Feed>,
    pub(super) old_feeds: rtrb::Producer<Feed>,
    /// The live input, if the app gave one.
    pub(super) live: Option<Feed>,
    /// A replaced live input that did not fit in its queue back yet.
    pub(super) old_feed: Option<Feed>,
    /// Where recorded messages go.
    pub(super) capture: rtrb::Producer<Captured>,
    /// Messages lost because the app was not reading (a count, for tests).
    pub(super) lost: u32,
    /// How many times the loop has wrapped: the pass messages are kept in.
    pub(super) pass: u32,
    /// A loop has wrapped since the last play, locate, or reposition.
    /// Delayed live input before the loop start belongs to the previous pass
    /// only while this is true.
    pub(super) wrapped_since_locate: bool,
    /// The last part rendered had the transport recording.
    pub(super) recording: bool,
    /// The ports that get MIDI clock, as [`MidiHandle::set_sync`] set them.
    pub(super) sync: Arc<AtomicU64>,
    /// The ports the clock was last sent to.
    pub(super) synced: u64,
    pub(super) clock_out: ClockOut,
    /// Some track's instrument runs in a plugin host.
    pub(super) hosted: bool,
    /// A list with a hosted instrument is waiting (the handle sets it).
    pub(super) hosted_pending: Arc<AtomicBool>,
    /// [`TrackSource::begin`] has done the part's first half: the next
    /// `render` plays the tracks it gathered.
    pub(super) prepared: bool,
}

/// Creates the MIDI pair, starting with no tracks. Allocates, so call it
/// off the audio thread.
pub fn midi_player() -> (MidiHandle, MidiPlayer) {
    let (incoming_tx, incoming_rx) = rtrb::RingBuffer::new(QUEUE);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(QUEUE + 1);
    let (out_tx, out_rx) = rtrb::RingBuffer::new(out::OUT_QUEUE);
    let (feeds_tx, feeds_rx) = rtrb::RingBuffer::new(2);
    let (old_feeds_tx, old_feeds_rx) = rtrb::RingBuffer::new(3);
    let (capture_tx, capture_rx) = rtrb::RingBuffer::new(capture::QUEUE);
    let sync = Arc::new(AtomicU64::new(0));
    let hosted_pending = Arc::new(AtomicBool::new(false));
    (
        MidiHandle {
            incoming: incoming_tx,
            retired: retired_rx,
            output: Some(out_rx),
            feeds: feeds_tx,
            old_feeds: old_feeds_rx,
            captured: Some(capture_rx),
            sync: Arc::clone(&sync),
            hosted_pending: Arc::clone(&hosted_pending),
        },
        MidiPlayer {
            incoming: incoming_rx,
            retired: retired_tx,
            stash: None,
            current: Box::new(MidiSet::new(Vec::new())),
            out: Out {
                tx: out_tx,
                dropped: 0,
            },
            clock: 0,
            feeds: feeds_rx,
            old_feeds: old_feeds_tx,
            live: None,
            old_feed: None,
            capture: capture_tx,
            lost: 0,
            pass: 0,
            wrapped_since_locate: false,
            recording: false,
            sync,
            synced: 0,
            clock_out: ClockOut::default(),
            hosted: false,
            hosted_pending,
            prepared: false,
        },
    )
}

impl MidiPlayer {
    /// Events and notes dropped for lack of room so far (a full track
    /// queue, a full output queue). Zero in any sane project.
    pub fn dropped(&self) -> u32 {
        let tracks: u32 = self
            .current
            .tracks
            .iter()
            .map(|t| t.state.dropped.saturating_add(t.state.live.dropped))
            .sum();
        tracks
            .saturating_add(self.out.dropped)
            .saturating_add(self.lost)
    }

    /// Notes held on a track now (tests).
    pub fn held(&self, track: u64) -> usize {
        self.current
            .tracks
            .iter()
            .find(|t| t.id == track)
            .map_or(0, |t| t.state.held.len())
    }

    /// Live notes held on a track now (tests).
    pub fn held_live(&self, track: u64) -> usize {
        self.current
            .tracks
            .iter()
            .find(|t| t.id == track)
            .map_or(0, |t| t.state.live.len())
    }
}

impl MidiTrack {
    /// Whether anything is listening: an instrument, or hardware.
    pub(super) fn sounds(&self) -> bool {
        matches!(self.target, Target::Port { .. }) || self.instrument.is_some()
    }
}

const _: () = assert!(MAX_EVENTS >= 512, "a panic and a chase need room");
