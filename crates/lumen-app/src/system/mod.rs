//! The application around the project: how Lumen was started (`launch`), the
//! window icon (`brand`), the window as the owner of native dialogs (`owner`),
//! developer tracing (`trace`) and crash reports (`crash`).
//!
//! These are the concerns of the process and its window, not of a project
//! (sweep M11: one folder per concern).

pub(crate) mod brand;
pub(crate) mod crash;
pub(crate) mod launch;
pub(crate) mod owner;
pub(crate) mod trace;
