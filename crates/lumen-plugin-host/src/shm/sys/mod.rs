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

/// A base for one link's names: this process, a counter, and random bits. The
/// names are only ever handed to the process that made the objects, but a
/// program that guessed one could open another link's objects and read or
/// write its audio path, so the base is not the plain pid and counter it once
/// was. The bits come from the standard library's hasher keys, mixed with the
/// clock and a stack address; 32 of them are enough, and keep the name inside
/// the 31-character limit above.
pub fn link_base() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hash, Hasher};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Instant;

    static LINKS: AtomicU32 = AtomicU32::new(0);
    let here = 0_u8;
    let mut hasher = RandomState::new().build_hasher();
    Instant::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    LINKS.fetch_add(1, Ordering::Relaxed).hash(&mut hasher);
    (std::ptr::from_ref(&here) as usize).hash(&mut hasher);
    format!("{:x}-{:08x}", std::process::id(), hasher.finish() as u32)
}
