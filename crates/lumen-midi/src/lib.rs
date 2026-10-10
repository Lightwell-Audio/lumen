//! Lumen's MIDI device layer (ADR-0044).
//!
//! - [`types`]: ports, short messages and errors.
//! - [`backend`]: the interface a driver model implements; [`midir_host`]
//!   is the real one (WinMM, CoreMIDI, ALSA) and [`fake`] a scriptable one
//!   for tests.
//! - [`system`]: [`MidiSystem`], a worker thread that lists ports, notices
//!   plugging and unplugging, and opens and closes ports as the user's
//!   switches say, so enumeration and opening never touch the UI thread.

pub mod backend;
pub mod fake;
pub mod midir_host;
pub mod system;
pub mod types;

pub use backend::{Backend, Connection, InputSink, Output};
pub use system::{MidiSender, MidiSystem, PortEvent, PortState};
pub use types::{Direction, MidiError, PortId, PortInfo, Short, Timed, Use};
