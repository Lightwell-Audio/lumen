//! The project document and the `.lumen` file format (ADR-0004).
//!
//! A project is a folder holding `<name>.lumen` (pretty-printed UTF-8 JSON),
//! `Samples/` and `Backups/`. The document is wrapped in an envelope
//! `{ "format": "lumen-project", "version": N, "generator": ..., "project": ... }`.
//! Older versions are upgraded by [`migrate`]; newer ones are refused.

pub mod automation;
pub mod edit;
pub mod groups;
pub mod history;
mod io;
mod json;
pub mod mapping;
pub mod migrate;
pub mod model;
pub mod routing;
pub mod smf;
pub mod solo;
#[doc(hidden)]
pub mod testing;
mod validate;

pub use history::History;
pub use io::{
    FORMAT_ID, LoadError, MediaLocation, SaveError, SaveOptions, from_bytes, load, locate_media,
    media_path_for, save, to_bytes, write_atomic,
};
pub use validate::validate;
