#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{Mapping, Signal, audio_thread_priority, remove_names};
#[cfg(windows)]
pub use windows::{Job, Mapping, Signal, audio_thread_priority, remove_names};

pub struct Names {
    pub region: String,
    pub go: String,
    pub done: String,
}

impl Names {
    pub fn new(base: &str) -> Self {
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
