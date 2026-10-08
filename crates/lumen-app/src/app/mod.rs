//! The application's behaviour, by concern. `App` itself, the frame loop and
//! start-up stay in `main.rs`; these modules hold its `impl` blocks (audit
//! M1).

pub(crate) mod grid;
pub(crate) mod history;
pub(crate) mod navigate;
pub(crate) mod preferences;
pub(crate) mod projects;
pub(crate) mod recovery;
pub(crate) mod view;
