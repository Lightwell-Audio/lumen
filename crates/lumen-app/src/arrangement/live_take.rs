//! The take as it grows (M5 slice 6, after MT-MIDI-7): drawn on every frame
//! from what the recorder has captured, not from the clips the document
//! gets about once a second. The clip being recorded reaches the playhead
//! smoothly and its notes appear as they are played (a held note grows),
//! and nothing under it is redrawn when the document takes a commit.
//!
//! The arrangement draws it over the lanes, after the clips: in Replace its
//! body covers whatever is under it, in Overdub only the gaps between the
//! clips under it (the clips there keep their own bodies, with the new notes
//! drawn on them).

use lumen_core::time::Ticks;
use lumen_project::model::Id;

/// A note of the take, held or finished. A held note ends where the
/// playhead is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiveNote {
    pub start: Ticks,
    pub end: Ticks,
    pub pitch: u8,
    pub velocity: u8,
}

/// One recording track's part of the take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveTrack {
    pub track: Id,
    pub notes: Vec<LiveNote>,
    /// Overdub: where the take is over no clip, and so is a new clip. Replace
    /// leaves it empty; its take is one clip over the whole range.
    pub gaps: Vec<(Ticks, Ticks)>,
    /// The pitch range the track's notes are laid out to: its clips and the
    /// take's notes, as the track will have once the take is in
    /// (`piano_roll::mini::Fit`). None before a note is played.
    pub range: Option<(u8, u8)>,
}

/// The take under way: where it begins, how far it has reached, and what
/// each recording track has played.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveTake {
    pub from: Ticks,
    pub to: Ticks,
    pub overdub: bool,
    pub tracks: Vec<LiveTrack>,
}

impl LiveTake {
    pub(crate) fn track(&self, id: Id) -> Option<&LiveTrack> {
        self.tracks.iter().find(|t| t.track == id)
    }

    /// The stretches of `track`'s lane the take's body is drawn over: all of
    /// the take in Replace, the gaps in Overdub.
    pub(crate) fn bodies(&self, track: &LiveTrack) -> Vec<(Ticks, Ticks)> {
        if self.overdub {
            track.gaps.clone()
        } else if self.to > self.from {
            vec![(self.from, self.to)]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(start: i64, end: i64, pitch: u8) -> LiveNote {
        LiveNote {
            start: Ticks(start),
            end: Ticks(end),
            pitch,
            velocity: 100,
        }
    }

    fn take(overdub: bool) -> LiveTake {
        LiveTake {
            from: Ticks(100),
            to: Ticks(900),
            overdub,
            tracks: vec![LiveTrack {
                track: Id(3),
                notes: vec![note(100, 200, 64), note(150, 900, 48), note(300, 310, 90)],
                gaps: vec![(Ticks(100), Ticks(400))],
                range: Some((48, 90)),
            }],
        }
    }

    #[test]
    fn replace_covers_the_whole_take_and_overdub_only_its_gaps() {
        let replace = take(false);
        assert_eq!(
            replace.bodies(&replace.tracks[0]),
            vec![(Ticks(100), Ticks(900))]
        );
        let overdub = take(true);
        assert_eq!(
            overdub.bodies(&overdub.tracks[0]),
            vec![(Ticks(100), Ticks(400))]
        );
    }

    #[test]
    fn a_take_that_has_not_moved_has_no_body() {
        let mut t = take(false);
        t.to = t.from;
        assert!(t.bodies(&t.tracks[0]).is_empty());
    }

    #[test]
    fn tracks_are_found_by_id() {
        let t = take(false);
        assert!(t.track(Id(3)).is_some());
        assert!(t.track(Id(4)).is_none());
    }
}
