//! Finding out what a module offers without risking Lumen.
//!
//! A scan loads a plugin module, which runs the module's own code: it can
//! crash, hang or damage memory. So it never happens in Lumen. [`host`] is
//! the child's side (the `lumen-plugin-host scan <path>` mode); [`run`] is
//! Lumen's: start a child for one module, wait with a deadline, and say how
//! it went. [`report`] is the reply between them.

pub mod host;
pub mod report;
pub mod run;

pub use report::{MAX_REPLY, ScanClass, ScanReply, ScanReport};
pub use run::{ScanOutcome, check_host, run_scan};
