//! The messages between Lumen and a plugin-host process, and their framing.
//!
//! Every request gets exactly one response, in order. The one message that is
//! not an answer is [`Response::Key`]: a key pressed in the plugin's window
//! that Lumen asked for ([`Request::CaptureKeys`]), sent when it happens and
//! set apart by the reader. A `Process` request names the
//! configuration generation and a block sequence number, and its response
//! echoes both, so a response that arrives after its deadline (a late
//! plugin) is recognised as stale and dropped rather than written into a
//! block that has moved on. Everything read is bounded (see the limits).

mod codec;
mod frame;
mod types;

#[cfg(test)]
mod tests;

pub use frame::*;
pub use types::*;
