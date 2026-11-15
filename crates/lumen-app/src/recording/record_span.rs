use lumen_core::time::Ticks;

const OPEN: Ticks = Ticks(i64::MAX / 8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub from: Ticks,
    pub until: Option<Ticks>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Audio,
    Midi,
}

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
        let punch = Some((q(4), OPEN_PUNCH));
        assert_eq!(span(Kind::Audio, q(1), looped, punch), s(4, Some(8)));
        let punch = Some((Ticks::ZERO, q(8)));
        assert_eq!(span(Kind::Audio, q(2), None, punch), s(2, Some(8)));
    }

    #[test]
    fn once_ended_only_what_was_recorded_stays_replaced() {
        let span = s(4, Some(8));
        assert_eq!(span.recorded(q(6), q(5), true), Some(span));
        assert_eq!(span.recorded(q(6), q(7), false), Some(s(6, Some(7))));
        assert_eq!(span.recorded(q(6), q(6), false), None);
        assert_eq!(s(2, None).recorded(q(2), q(9), false), Some(s(2, Some(9))));
    }
}
