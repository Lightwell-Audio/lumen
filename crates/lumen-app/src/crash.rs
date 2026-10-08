//! Crash reports (CLAUDE.md §7, "crash report file"; owner, M3 slice 6c,
//! to find a crash at deep zoom). A panic anywhere in Lumen (the UI, the
//! audio thread, a loader) writes a report before the process ends: what
//! failed, where, the backtrace, the version, the system and the thread.
//! The next start offers the newest report the owner has not seen.
//!
//! Reports are plain text in `%LOCALAPPDATA%\Lumen\Crashes` (Windows),
//! `~/Library/Logs/Lumen` (macOS) or `$XDG_STATE_HOME/lumen/crashes`
//! (Linux). Nothing is sent anywhere (CLAUDE.md §4, privacy).
//!
//! A failed allocation is not a panic: the app's allocator
//! (`lumen_rt::ReportingAllocator`) reports it before Rust ends the
//! process. A failure raised by native code itself (a driver's fail-fast)
//! ends the process before any Rust code can run, so it leaves no report.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use lumen_core::product::PRODUCT_NAME;

/// The marker naming the newest report already shown.
const SEEN: &str = "seen.txt";

/// The default folder for crash reports.
pub fn default_dir() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join(PRODUCT_NAME)
            .join("Crashes")
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?)
            .join("Library/Logs")
            .join(PRODUCT_NAME)
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/state")))?
            .join(PRODUCT_NAME.to_lowercase())
            .join("crashes")
    };
    Some(dir)
}

/// Where reports go, for the allocation-failure hook (a plain function).
static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Writes a report to `dir` whenever anything panics or an allocation
/// fails, then lets the usual message print. Installed once, by the real
/// binary only.
pub fn install(dir: PathBuf) {
    let _ = DIR.set(dir.clone());
    lumen_rt::on_allocation_failure(out_of_memory);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_owned());
        let location = info.location().map_or_else(
            || "unknown".to_owned(),
            |l| format!("{}:{}:{}", l.file(), l.line(), l.column()),
        );
        let thread = std::thread::current();
        let report = report(&Panic {
            message: &message,
            location: &location,
            thread: thread.name().unwrap_or("unnamed"),
            backtrace: &std::backtrace::Backtrace::force_capture().to_string(),
            seconds: now(),
        });
        // Best effort: a report that cannot be written must not hide the
        // panic message below.
        let _ = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(dir.join(file_name(now())), report));
        previous(info);
    }));
}

/// An allocation failed (`ReportingAllocator`): not a panic, so this is
/// the only chance to say where. Small allocations still work, so the
/// report is written as usual.
fn out_of_memory(size: usize) {
    let Some(dir) = DIR.get() else {
        return;
    };
    let thread = std::thread::current();
    let report = report(&Panic {
        message: &format!("memory allocation of {size} bytes failed"),
        location: "see the backtrace (the first lumen_ frame)",
        thread: thread.name().unwrap_or("unnamed"),
        backtrace: &std::backtrace::Backtrace::force_capture().to_string(),
        seconds: now(),
    });
    let _ = std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(dir.join(file_name(now())), report));
}

/// A deliberate failure, to check that reports are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Test {
    Panic,
    Memory,
}

/// Fails as `test` says.
pub fn fail(test: Test) {
    match test {
        Test::Panic => panic!("--crash-test: a deliberate panic"),
        Test::Memory => {
            // 64 TiB: no machine has it, so the allocation fails.
            let huge = vec![0_u8; 1 << 46];
            std::hint::black_box(&huge);
        }
    }
}

/// What a report says.
pub struct Panic<'a> {
    pub message: &'a str,
    pub location: &'a str,
    pub thread: &'a str,
    pub backtrace: &'a str,
    /// Seconds since 1970 (UTC).
    pub seconds: u64,
}

/// The text of a crash report.
pub fn report(p: &Panic<'_>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{PRODUCT_NAME} crash report");
    let _ = writeln!(out, "Time:     {} UTC", utc(p.seconds));
    let _ = writeln!(out, "Version:  {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "System:   {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(out, "Thread:   {}", p.thread);
    let _ = writeln!(out, "Where:    {}", p.location);
    let _ = writeln!(out, "What:     {}", p.message);
    let _ = writeln!(out, "\nBacktrace:\n{}", p.backtrace);
    out
}

/// `crash-<seconds since 1970>.txt`: sorts by time.
fn file_name(seconds: u64) -> String {
    format!("crash-{seconds:012}.txt")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `2026-10-08 14:03:07` from seconds since 1970 (UTC; civil-from-days).
fn utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rest / 3_600,
        rest / 60 % 60,
        rest % 60
    )
}

/// The newest report in `dir` that has not been shown yet.
pub fn unseen(dir: &Path) -> Option<PathBuf> {
    let seen = std::fs::read_to_string(dir.join(SEEN)).unwrap_or_default();
    let newest = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        .max()?;
    (newest.as_str() > seen.trim()).then(|| dir.join(newest))
}

/// Remembers that `report` was shown, so it is not offered again.
pub fn mark_seen(report: &Path) {
    if let (Some(dir), Some(name)) = (report.parent(), report.file_name()) {
        let _ = std::fs::write(dir.join(SEEN), name.to_string_lossy().as_bytes());
    }
}

/// Shows `path` in the system's file browser.
pub fn reveal(path: &Path) {
    let result = if cfg!(windows) {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
    } else {
        std::process::Command::new("xdg-open")
            .arg(path.parent().unwrap_or(path))
            .spawn()
    };
    if let Err(e) = result {
        crate::trace::log(|| format!("crash: could not reveal {}: {e}", path.display()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_says_what_where_when_and_how() {
        let text = report(&Panic {
            message: "index out of bounds",
            location: "src/arrangement/samples.rs:120:5",
            thread: "main",
            backtrace: "0: lumen_app::arrangement::samples::Strips::update",
            seconds: 1_791_468_187,
        });
        assert!(text.contains("Time:     2026-10-08 14:03:07 UTC"), "{text}");
        assert!(text.contains("What:     index out of bounds"));
        assert!(text.contains("Where:    src/arrangement/samples.rs:120:5"));
        assert!(text.contains("Thread:   main"));
        assert!(text.contains("Strips::update"));
        assert_eq!(utc(0), "1970-01-01 00:00:00");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00:00", "a leap day");
    }

    #[test]
    fn the_newest_report_is_offered_once() {
        let dir = std::env::temp_dir().join(format!("lumen-crash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unseen(&dir), None);
        std::fs::write(dir.join(file_name(100)), "old").unwrap();
        std::fs::write(dir.join(file_name(200)), "new").unwrap();
        let newest = unseen(&dir).unwrap();
        assert!(newest.ends_with(file_name(200)));
        mark_seen(&newest);
        assert_eq!(unseen(&dir), None, "shown once");
        std::fs::write(dir.join(file_name(300)), "newer").unwrap();
        assert!(unseen(&dir).unwrap().ends_with(file_name(300)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
