//! The project document and the `.lumen` file format (ADR-0004).
//!
//! A project is a folder holding `<name>.lumen` (pretty-printed UTF-8 JSON),
//! `Samples/` and `Backups/`. The document is wrapped in an envelope
//! `{ "format": "lumen-project", "version": N, "generator": ..., "project": ... }`.
//! Older versions are upgraded by [`migrate`]; newer ones are refused.

mod io;
mod json;
pub mod migrate;
pub mod model;
#[doc(hidden)]
pub mod testing;
mod validate;

pub use io::{
    FORMAT_ID, LoadError, MediaLocation, SaveError, SaveOptions, from_bytes, load, locate_media,
    media_path_for, save, to_bytes,
};
pub use validate::validate;
