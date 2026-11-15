use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Default)]
pub struct HostLog {
    lines: std::sync::Mutex<std::collections::VecDeque<String>>,
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

    pub fn added(&self) -> u32 {
        self.added.load(Ordering::Acquire)
    }

    pub fn lines(&self) -> Vec<String> {
        let lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        lines.iter().cloned().collect()
    }

    pub fn crash(&self) -> Option<String> {
        self.lines()
            .into_iter()
            .rev()
            .find_map(|l| l.strip_prefix("crash ").map(str::to_owned))
    }
}

pub(super) fn host_line<'a>(line: &'a str, mark: &str) -> Option<&'a str> {
    line.trim_end().strip_prefix(mark)
}
