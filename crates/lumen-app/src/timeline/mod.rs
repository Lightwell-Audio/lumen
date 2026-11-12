//! The arrangement's time axis and its drawing: the horizontal view
//! (`timeview`), the ruler's formats (`timefmt`), the position as read and
//! typed (`position`), the smooth playhead (`playhead`), ruler geometry
//! (`ruler`) and the canvases the renderer paints (`canvas`).
//!
//! Zooming, scrolling, fitting and formatting are pure functions, tested on
//! their own; the app applies them (sweep M11: one folder per concern).

pub(crate) mod canvas;
pub(crate) mod playhead;
pub(crate) mod position;
pub(crate) mod ruler;
pub(crate) mod timefmt;
pub(crate) mod timeview;
