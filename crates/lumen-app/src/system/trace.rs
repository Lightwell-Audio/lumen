use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

struct Sink {
    start: Instant,
    file: Option<Mutex<File>>,
}

static SINK: OnceLock<Option<Sink>> = OnceLock::new();

fn sink() -> Option<&'static Sink> {
    SINK.get_or_init(|| {
        let target = std::env::var_os("LUMEN_TRACE")?;
        let file = if target == "1" {
            None
        } else {
            Some(Mutex::new(File::create(&target).ok()?))
        };
        Some(Sink {
            start: Instant::now(),
            file,
        })
    })
    .as_ref()
}

pub fn enabled() -> bool {
    sink().is_some()
}

pub fn log(message: impl FnOnce() -> String) {
    let Some(sink) = sink() else {
        return;
    };
    let line = format!(
        "[{:>9.1} ms] {}\n",
        sink.start.elapsed().as_secs_f64() * 1e3,
        message()
    );
    match &sink.file {
        Some(file) => {
            if let Ok(mut file) = file.lock() {
                let _ = file.write_all(line.as_bytes());
            }
        }
        None => eprint!("{line}"),
    }
}
