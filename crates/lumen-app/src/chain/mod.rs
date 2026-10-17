//! The Devices tab (M6 slice 2d): a track's device chain as cards in a sunk
//! tray, joined by cables (the owner's C4, design system v3). `place` decides
//! where a chosen device goes, `art` makes a card's picture from its
//! identity, `geometry` lays the chain out, `card` paints one device and
//! `view` is the tab.

mod actions;
mod art;
mod card;
mod geometry;
pub(crate) mod place;
mod view;

use lumen_project::model::Id;

pub(crate) use geometry::panel_height;
pub(crate) use view::paint_drag_ghost;

/// What is being dragged from the browser: a device's catalog id.
pub(crate) struct Dragged(pub String);

/// The chain's own state: which card is chosen (the commands act on it) and
/// whether the tab holds the keyboard.
#[derive(Default)]
pub(crate) struct Chain {
    /// The chosen device; it belongs to the shown track.
    pub selected: Option<Id>,
    /// The Devices tab has the keyboard: Delete removes the chosen device.
    pub live: bool,
    /// The pointer was over the tray last frame.
    pub hovered: bool,
    /// The detail panel should be tall enough for a whole card at the next
    /// frame's layout (it only ever grows; the user can drag it back).
    pub grow: bool,
}
