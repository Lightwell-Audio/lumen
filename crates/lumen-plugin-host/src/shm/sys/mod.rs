//! The operating system's side of the audio path: a named block of shared
//! memory, a named wake-up signal, and the job that ends a host with Lumen.
//!
//! Both objects are created by Lumen and opened by name in the host. A name
//! is only needed until both sides have opened it; on Unix Lumen removes the
//! names then, so a crash cannot leave them behind. On Windows a name goes
//! with its last handle.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{Mapping, Signal, audio_thread_priority, remove_names};
#[cfg(windows)]
pub use windows::{Job, Mapping, Signal, audio_thread_priority, remove_names};

/// The names of the three objects of one link, from a base that is unique to
/// the Lumen process and the link.
pub struct Names {
    pub region: String,
    pub go: String,
    pub done: String,
}

impl Names {
    pub fn new(base: &str) -> Self {
        // macOS limits a POSIX object name to 31 characters, the leading
        // slash included, so these stay short.
        #[cfg(unix)]
        let head = "/lm";
        #[cfg(windows)]
        let head = "Local\\lumen-";
        Self {
            region: format!("{head}{base}-m"),
            go: format!("{head}{base}-g"),
            done: format!("{head}{base}-d"),
        }
    }
}
