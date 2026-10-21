//! The tempo and meter lanes (M7 slice A6): two slim lanes between the bar
//! ruler and the tracks that show where the song's tempo and time signature
//! change, and edit them. The model is `Project.tempo` and `Project.meter`;
//! every edit is a `Document` method and one undo step (a drag is one in
//! all). Which lanes are shown is not saved.
//!
//! The tempo lane draws each tempo as a step and a point; a click adds a
//! change, a drag moves one in time and value, a double-click types its
//! tempo. The meter lane draws a chip at each change; a click on empty lane
//! asks for a time signature at the nearest bar line, a drag moves a chip to
//! another bar line, a double-click types its signature. The pointer
//! handling is `app/song_lanes.rs`; the look is `paint.rs`; where things are
//! is `scale.rs`, which is pure and tested.

mod paint;
pub(crate) mod scale;

use eframe::egui::{Pos2, Rect};
use lumen_core::time::Ticks;

use crate::theme;
pub(crate) use paint::{Look, draw};
pub(crate) use scale::{Plane, Scale};

/// Which lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Which {
    Tempo,
    Meter,
}

/// A change picked or under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Picked {
    pub which: Which,
    pub index: usize,
}

/// What the pointer is doing with a change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Gesture {
    /// Moving tempo change `index`, on the scale the press saw, so the lane
    /// does not rescale under the pointer.
    Tempo { index: usize, scale: Scale },
    /// Moving time-signature change `index` to another bar line.
    Meter { index: usize, grab: f32 },
}

/// A value being typed where it sits.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Typing {
    pub which: Which,
    /// The change typed over; `None` for a time signature to add at `at`.
    pub index: Option<usize>,
    /// Where a new change goes (the nearest bar line is found on commit).
    pub at: Ticks,
    pub text: String,
    /// Top left of the box.
    pub anchor: Pos2,
}

/// The lanes' state: not saved, not undone.
#[derive(Debug, Default)]
pub(crate) struct SongLanes {
    pub shown: bool,
    pub picked: Option<Picked>,
    /// A change was pressed last: Delete means it.
    pub focus: bool,
    pub gesture: Option<Gesture>,
    pub typing: Option<Typing>,
    pub hover: Option<Picked>,
    /// Where the lane was right-clicked, for its menu.
    pub menu: Option<MenuSpot>,
    /// A box a command asked for, opened once the lanes are on screen.
    pub ask: Option<(Which, Option<usize>, Ticks)>,
    /// Where the lanes were drawn last and on what scale, for the tests and
    /// for boxes a command asks for.
    pub drawn: Option<(Band, Scale)>,
}

/// Where a right-click on a lane was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MenuSpot {
    pub which: Which,
    pub index: Option<usize>,
    pub at: Ticks,
}

impl SongLanes {
    /// The band's height when it is fully shown.
    pub fn full_height() -> f32 {
        theme::size::SONG_TEMPO + theme::size::SONG_METER
    }

    /// The change picked, if it is still there.
    pub fn picked_on(&self, tempo: usize, meter: usize) -> Option<Picked> {
        self.picked.filter(|p| {
            p.index
                < match p.which {
                    Which::Tempo => tempo,
                    Which::Meter => meter,
                }
        })
    }
}

/// Where the band's parts are on the screen.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Band {
    /// The name column over the track headers.
    pub corner: Rect,
    pub tempo: Plane,
    pub meter: Plane,
}
