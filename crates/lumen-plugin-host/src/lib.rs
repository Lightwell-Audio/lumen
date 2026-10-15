//! The plugin host. A plugin runs in a `lumen-plugin-host` process, never in
//! Lumen's own, so a crash or a hang costs one device and not the session
//! (ADR-0005). This crate holds both sides:
//!
//! - the host side: [`Module`] loads a VST3 module with the operating
//!   system's loader, [`Instance`] runs one plugin, [`server`] answers
//!   requests for it;
//! - the Lumen side: [`client::HostProcess`] starts a host process and talks
//!   to it with deadlines, and says how a host that stopped ended.
//!
//! [`protocol`] is the versioned, bounded message format between them.
//!
//! The transport here is a pair of pipes. It carries state, parameter and
//! fault traffic and proves the protocol; the audio path with shared memory
//! and per-block signalling follows the measurements in
//! `docs/milestones/M6.md`.

// The bindings' enum constants are `i32` on Windows and `u32` elsewhere.
#![allow(clippy::unnecessary_cast)]

mod com;
pub mod instance;
pub mod module;
pub mod process;
pub mod protocol;
pub mod server;
pub mod wire;

pub mod client;

pub use instance::{Instance, InstanceError};
pub use module::{Module, ModuleError};
