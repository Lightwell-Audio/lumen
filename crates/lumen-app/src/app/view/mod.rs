//! The main window's content: the transport bar, the arrangement with its
//! ruler, playhead and drop overlay, and the device banner.
//!
//! By concern (audit M10): the notice card every banner uses (`notice`), the
//! transport bar (`transport`), the arrangement and what is dropped on it
//! (`arrangement`), the edge that sizes the track headers (`divider`), the
//! system file drag's pointer (`drag`), the audio device banner (`banner`)
//! and the `--bench` drivers (`bench`).

mod arrangement;
mod banner;
mod bench;
mod divider;
mod drag;
mod notice;
mod transport;

pub(crate) use notice::{notice_card, notice_card_with, notice_panel_frame};
