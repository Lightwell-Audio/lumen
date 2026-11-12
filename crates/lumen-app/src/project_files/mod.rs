//! A project on disk: saving and opening it (`files`), the autosave and
//! crash recovery that keep work a crash cannot cost (`autosave`), and
//! relinking media that went missing (`relink`).
//!
//! All of it runs on a worker and comes back on a channel; the UI never
//! waits for the disk (sweep M11: one folder per concern).

pub(crate) mod autosave;
pub(crate) mod files;
pub(crate) mod relink;
