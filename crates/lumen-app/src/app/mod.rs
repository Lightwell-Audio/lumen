//! The application's behaviour, by concern. `App` itself, the frame loop and
//! start-up stay in `main.rs`; these modules hold its `impl` blocks (audit
//! M1).

pub(crate) mod clip_card;
pub(crate) mod clipboard_keys;
mod commands;
mod cue;
pub(crate) mod editing;
pub(crate) mod grid;
mod handles;
pub(crate) mod hardware;
pub(crate) mod history;
pub(crate) mod info;
pub(crate) mod locators;
mod mixer;
pub(crate) mod navigate;
pub(crate) mod palette;
pub(crate) mod panes;
pub(crate) mod preferences;
pub(crate) mod projects;
pub(crate) mod recovery;
pub(crate) mod regions;
pub(crate) mod rulers;
pub(crate) mod scrub;
pub(crate) mod shortcuts;
pub(crate) mod test_modes;
pub(crate) mod view;
