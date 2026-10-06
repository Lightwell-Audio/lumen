//! Developer tracing: with `LUMEN_TRACE=1`, timestamped lines on stderr for
//! frames, input and transport commands. Used to measure UI latency.

use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Option<Instant>> = OnceLock::new();

/// Whether tracing is on (decided once, at first use).
pub fn enabled() -> bool {
    START
        .get_or_init(|| std::env::var_os("LUMEN_TRACE").map(|_| Instant::now()))
        .is_some()
}

/// Prints `message` with milliseconds since tracing started.
pub fn log(message: impl FnOnce() -> String) {
    if let Some(Some(start)) = START.get().or_else(|| {
        enabled();
        START.get()
    }) {
        eprintln!(
            "[{:>8.1} ms] {}",
            start.elapsed().as_secs_f64() * 1e3,
            message()
        );
    }
}
