//! What the host says about itself on stderr, kept for whoever shows it.

use std::sync::atomic::{AtomicU32, Ordering};

/// What the host process said about itself on stderr: its own lines, marked
/// with the nonce this process chose (see [`crate::process::NONCE_ENV`]);
/// anything else a plugin prints is read and dropped. Shared between the
/// reader thread and whoever shows it.
#[derive(Default)]
pub struct HostLog {
    lines: std::sync::Mutex<std::collections::VecDeque<String>>,
    /// Counts lines added, so a watcher can tell when to look.
    added: AtomicU32,
}

impl HostLog {
    const MOST_LINES: usize = 40;
    const MOST_BYTES: usize = 400;

    pub(super) fn push(&self, line: &str) {
        let mut line = line.to_owned();
        line.truncate(Self::MOST_BYTES.min(line.floor_char_boundary(Self::MOST_BYTES)));
        let mut lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        if lines.len() == Self::MOST_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
        self.added.fetch_add(1, Ordering::Release);
    }

    /// How many lines have been said in all.
    pub fn added(&self) -> u32 {
        self.added.load(Ordering::Acquire)
    }

    /// What the host said, oldest first.
    pub fn lines(&self) -> Vec<String> {
        let lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        lines.iter().cloned().collect()
    }

    /// The crash the host reported, if it did: where the plugin died.
    pub fn crash(&self) -> Option<String> {
        self.lines()
            .into_iter()
            .rev()
            .find_map(|l| l.strip_prefix("crash ").map(str::to_owned))
    }
}

/// The host's own words in one line of its stderr, if the line is the host's.
/// A plugin writes to the same pipe, so a line without this process's nonce is
/// not the host's, however much it looks like one.
pub(super) fn host_line<'a>(line: &'a str, mark: &str) -> Option<&'a str> {
    line.trim_end().strip_prefix(mark)
}
