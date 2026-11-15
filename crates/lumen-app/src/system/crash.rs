use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use lumen_core::product::PRODUCT_NAME;

const SEEN: &str = "seen.txt";

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

static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
static PANIC_REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn install(dir: PathBuf) {
    let _ = DIR.set(dir.clone());
    lumen_rt::on_allocation_failure(out_of_memory);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if PANIC_REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            previous(info);
            return;
        }
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
        let _ = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(dir.join(file_name(now())), report));
        previous(info);
    }));
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Test {
    Panic,
    Memory,
}

pub fn fail(test: Test) {
    match test {
        Test::Panic => panic!("--crash-test: a deliberate panic"),
        Test::Memory => {
            let huge = Vec::<u8>::with_capacity(isize::MAX as usize);
            std::hint::black_box(&huge);
        }
    }
}

pub struct Panic<'a> {
    pub message: &'a str,
    pub location: &'a str,
    pub thread: &'a str,
    pub backtrace: &'a str,
    pub seconds: u64,
}

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

fn file_name(seconds: u64) -> String {
    format!("crash-{seconds:012}.txt")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

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

pub fn unseen(dir: &Path) -> Option<PathBuf> {
    let seen = std::fs::read_to_string(dir.join(SEEN)).unwrap_or_default();
    let newest = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        .max()?;
    (newest.as_str() > seen.trim()).then(|| dir.join(newest))
}

pub fn mark_seen(report: &Path) {
    if let (Some(dir), Some(name)) = (report.parent(), report.file_name()) {
        let _ = std::fs::write(dir.join(SEEN), name.to_string_lossy().as_bytes());
    }
}

pub fn reveal(path: &Path) {
    let result = if cfg!(windows) {
        reveal_in_explorer(path)
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

#[cfg(windows)]
fn reveal_in_explorer(path: &Path) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    let shown = path.display().to_string().replace('/', "\\");
    let path = shown.strip_prefix(r"\\?\").unwrap_or(&shown);
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{path}\""))
        .spawn()
}

#[cfg(not(windows))]
fn reveal_in_explorer(_: &Path) -> std::io::Result<std::process::Child> {
    unreachable!("only called on Windows")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "opens a file manager window"]
    fn reveal_selects_a_file_in_a_folder_with_a_space() {
        let dir = std::env::temp_dir().join("lumen reveal check");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("picked.wav");
        std::fs::write(&file, b"x").unwrap();
        reveal(&file);
        std::thread::sleep(std::time::Duration::from_secs(4));
    }

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
