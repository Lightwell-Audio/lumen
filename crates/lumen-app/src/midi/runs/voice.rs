use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::drums;
use lumen_project::model::{self, DrumMap, Id};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Dest {
    Own,
    To(Id),
    All,
}

impl Dest {
    fn takes(self, route: model::Route) -> bool {
        match self {
            Dest::Own => route.sent_to().is_none(),
            Dest::To(track) => route.sent_to() == Some(track),
            Dest::All => true,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Member<'a> {
    pub clip: &'a Arc<model::Clip>,
    pub length: Ticks,
    pub map: Option<&'a DrumMap>,
    pub dest: Dest,
}

impl<'a> Member<'a> {
    pub(crate) fn own(clip: &'a Arc<model::Clip>, length: Ticks, map: Option<&'a DrumMap>) -> Self {
        Self {
            clip,
            length,
            map,
            dest: Dest::Own,
        }
    }

    pub(crate) fn sending(mut self, dest: Dest) -> Self {
        self.dest = dest;
        self
    }
}

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

    pub(super) fn played(self, note: model::Note) -> Option<(u8, Option<u8>, Ticks)> {
        match self.playing {
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
