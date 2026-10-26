//! What a recording replaces (M9 slice R3, ADR-0104): one rule, fixed when
//! Record is pressed, for every part of the app that needs it.
//!
//! A take replaces what its track holds over the timeline it records: from
//! where recording began (or the loop's start, whichever is earlier) to the
//! loop's end with Loop on, or on until Stop without it, inside the punch
//! range when there is one. MIDI keeps no lead-in: with Loop on its take
//! begins at the loop's start, so the notes before the loop are not replaced.
//!
//! While recording, the old material over the whole span is silent from the
//! first sample, not only the part reached so far. That sounds the same: the
//! playhead only reaches a part of the span once the take is recording over
//! it (a loop's start is reached by wrapping, after which every pass replaces
//! the whole loop). And as the span never changes while recording, nothing
//! depends on noticing a wrap in time, which a frame-late UI cannot do.
//!
//! Once recording ends, what is still silent until the take is placed is
//! only what it recorded ([`Span::recorded`]).

use lumen_core::time::Ticks;

/// Ends at or past this are "no end" (an open punch-out).
const OPEN: Ticks = Ticks(i64::MAX / 8);

/// A stretch of the timeline a take replaces on its tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub from: Ticks,
    /// `None`: on until the recording stops.
    pub until: Option<Ticks>,
}

/// What kind of track records: an audio take keeps what it recorded before
/// the loop (a lead-in clip), a MIDI take does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Audio,
    Midi,
}

/// The span a recording that began at `start` replaces, with the loop
/// (`looped`, only when recording began before its end) and the punch range.
pub(crate) fn span(
    kind: Kind,
    start: Ticks,
    looped: Option<(Ticks, Ticks)>,
    punch: Option<(Ticks, Ticks)>,
) -> Span {
    let (mut from, mut until) = match (looped, kind) {
        (Some((a, b)), Kind::Audio) => (start.min(a), Some(b)),
        (Some((a, b)), Kind::Midi) => (a, Some(b)),
        (None, _) => (start, None),
    };
    if let Some((a, b)) = punch {
        from = from.max(a);
        if b < OPEN {
            until = Some(until.map_or(b, |u| u.min(b)));
        }
    }
    Span { from, until }
}

impl Span {
    /// What a recording of this span covered once it ended with the playhead
    /// at `head`, having `wrapped` round the loop or not: the whole span
    /// after a wrap, else from where it began to the playhead.
    pub(crate) fn recorded(self, start: Ticks, head: Ticks, wrapped: bool) -> Option<Span> {
        if wrapped {
            return Some(self);
        }
        let from = self.from.max(start);
        let until = self.until.map_or(head, |u| u.min(head));
        (until > from).then_some(Span {
            from,
            until: Some(until),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn s(from: i64, until: Option<i64>) -> Span {
        Span {
            from: q(from),
            until: until.map(q),
        }
    }

    const OPEN_PUNCH: Ticks = Ticks(i64::MAX / 4);

    #[test]
    fn without_loop_or_punch_a_take_replaces_from_where_it_began_on() {
        assert_eq!(span(Kind::Audio, q(3), None, None), s(3, None));
        assert_eq!(span(Kind::Midi, q(3), None, None), s(3, None));
    }

    #[test]
    fn begun_inside_the_loop_a_take_replaces_the_whole_loop_from_the_start() {
        // The owner's fifth run: began half-way into a loop over a clip.
        let looped = Some((q(4), q(8)));
        assert_eq!(span(Kind::Audio, q(6), looped, None), s(4, Some(8)));
        assert_eq!(span(Kind::Midi, q(6), looped, None), s(4, Some(8)));
    }

    #[test]
    fn begun_before_the_loop_audio_keeps_its_lead_in_and_midi_does_not() {
        let looped = Some((q(4), q(8)));
        assert_eq!(span(Kind::Audio, q(1), looped, None), s(1, Some(8)));
        assert_eq!(span(Kind::Midi, q(1), looped, None), s(4, Some(8)));
    }

    #[test]
    fn the_punch_range_bounds_the_span() {
        let looped = Some((q(4), q(8)));
        // Punch in at the loop's start, out left open.
        let punch = Some((q(4), OPEN_PUNCH));
        assert_eq!(span(Kind::Audio, q(1), looped, punch), s(4, Some(8)));
        // Punch out only, no loop: from where it began to the punch-out.
        let punch = Some((Ticks::ZERO, q(8)));
        assert_eq!(span(Kind::Audio, q(2), None, punch), s(2, Some(8)));
    }

    #[test]
    fn once_ended_only_what_was_recorded_stays_replaced() {
        let span = s(4, Some(8));
        // Wrapped: the whole loop was recorded over.
        assert_eq!(span.recorded(q(6), q(5), true), Some(span));
        // Stopped before the wrap: from where it began to the playhead.
        assert_eq!(span.recorded(q(6), q(7), false), Some(s(6, Some(7))));
        // Stopped before it reached anything.
        assert_eq!(span.recorded(q(6), q(6), false), None);
        // Without an end, the playhead is the end.
        assert_eq!(s(2, None).recorded(q(2), q(9), false), Some(s(2, Some(9))));
    }
}
