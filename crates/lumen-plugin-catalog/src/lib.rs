//! The plugin catalog: what plugins are installed, found without ever
//! loading one in Lumen's own process (ADR-0005, ADR-0068).
//!
//! - [`discover`] finds modules in the standard folders and the user's;
//! - [`scan`] describes each one in a child process with a deadline, a pool
//!   of them at a time, and says how every scan ended;
//! - [`cache`] remembers the answers by file stamp, so an unchanged module
//!   is not loaded again, and keeps the reason a module was blocked;
//! - [`blocklist`] words those reasons for the person who sees them;
//! - [`catalog`] merges the cache with Lumen's own devices and answers the
//!   browser's questions (search, kind, vendor).
//!
//! Nothing here draws, touches the project, or writes a file: the app owns
//! the thread, the disk and the interface.

pub mod blocklist;
pub mod cache;
pub mod catalog;
pub mod discover;
pub mod scan;
pub mod stamp;
