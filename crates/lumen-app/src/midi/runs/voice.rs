//! What a note is, read for one instrument (M8 slice D4): the key, channel
//! and length its drum map gives it, and which destination plays it.

use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::drums;
use lumen_project::model::{self, DrumMap, Id};

/// Which instrument plays the notes a clip is read for (M8 slice D4).
///
/// A track reads its clips twice when one of its sounds is sent to another
/// track: once for itself, leaving out the sounds it sends, and once for
/// each track that plays one, keeping only those. A sound on an output pair
/// is not forwarded: it stays on its own track and the instrument routes it
/// inside itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Dest {
    /// The track the clips are on.
    Own,
    /// Another instrument track, by id: only the sounds sent to it.
    To(Id),
    /// Every sound the map plays, wherever it is heard: what Export MIDI
    /// writes, so a sound sent to another track is not lost from its part.
    All,
}

impl Dest {
    /// Whether a sound playing `route` is read for this destination.
    fn takes(self, route: model::Route) -> bool {
        match self {
            Dest::Own => route.sent_to().is_none(),
            Dest::To(track) => route.sent_to() == Some(track),
            Dest::All => true,
        }
    }
}

/// A clip in a run, and how much of it plays (less than its length when a
/// Replace take is recording over its end).
///
/// `map` is the drum map the track reads its notes through (M8 D0), which
/// changes neither the clip nor its notes: the notes are given their sound's
/// note, channel and length here, at play time. `dest` is which instrument
/// the run is read for (M8 D4): the track's own, or one its sounds are sent
/// to.
#[derive(Clone, Copy)]
pub(crate) struct Member<'a> {
    pub clip: &'a Arc<model::Clip>,
    pub length: Ticks,
    pub map: Option<&'a DrumMap>,
    pub dest: Dest,
}

impl<'a> Member<'a> {
    /// The member read for the track it is on.
    pub(crate) fn own(clip: &'a Arc<model::Clip>, length: Ticks, map: Option<&'a DrumMap>) -> Self {
        Self {
            clip,
            length,
            map,
            dest: Dest::Own,
        }
    }

    /// The same member, read for the instrument of `dest`.
    pub(crate) fn sending(mut self, dest: Dest) -> Self {
        self.dest = dest;
        self
    }
}

/// How a track's drum map plays its notes: the map, if any, read once for the
/// whole run so a clip's notes cost O(notes) (M8 D0 review L7), and the
/// destination the notes are read for. A clip note with no map plays as
/// written.
#[derive(Clone, Copy)]
pub(crate) struct Voice<'a> {
    playing: Option<drums::Playing<'a>>,
    dest: Dest,
}

impl<'a> Voice<'a> {
    pub(super) fn new(map: Option<&'a DrumMap>, dest: Dest) -> Self {
        Self {
            playing: map.map(drums::Playing::new),
            dest,
        }
    }

    /// The note as played: the key, the channel (the track's when none) and
    /// the length (a sound's override, else the note's); none for a muted
    /// sound, or one no solo lets through, and none for a sound another
    /// instrument plays than the one this read is for (M8 D4).
    pub(super) fn played(self, note: model::Note) -> Option<(u8, Option<u8>, Ticks)> {
        match self.playing {
            // No map: every note plays as written, wherever it is heard.
            None => Some((note.pitch(), None, note.length())),
            Some(playing) => {
                let played = playing.played(note.pitch())?;
                self.dest
                    .takes(played.route)
                    .then(|| (played.key, played.channel, played.length_of(note.length())))
            }
        }
    }
}
