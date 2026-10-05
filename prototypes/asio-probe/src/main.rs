//! P-ASIO prototype: can Lumen host ASIO drivers natively from Rust?
//!
//! ```text
//! asio-probe list                       registered drivers (registry only)
//! asio-probe query [--timeout SECS]     capabilities of every driver, each in a sandboxed child
//! asio-probe stream <name|index> [--seconds S] [--buffer N] [--engine-tracks N] [--tone]
//! ```
//! `stream` outputs silence unless `--tone` is given.

#[cfg(windows)]
mod driver;
#[cfg(windows)]
mod ffi;
#[cfg(windows)]
mod query;
#[cfg(windows)]
mod registry;
#[cfg(windows)]
mod stream;

lumen_rt::install_guarded_allocator!();

#[cfg(not(windows))]
fn main() {
    eprintln!("asio-probe only runs on Windows");
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
    };
    match args.first().map(String::as_str) {
        None | Some("list") => {
            for (i, d) in registry::installed_drivers().iter().enumerate() {
                println!(
                    "{i:>2}  {:<32} {}  {}",
                    d.key_name,
                    d.clsid,
                    d.description.as_deref().unwrap_or("")
                );
            }
        }
        Some("query") => {
            let timeout = flag("--timeout").and_then(|s| s.parse().ok()).unwrap_or(15);
            query::query_all(std::time::Duration::from_secs(timeout));
        }
        Some("query-one") => match args.get(1) {
            Some(clsid) => query::query_one_and_print(clsid),
            None => std::process::exit(2),
        },
        Some("stream") => {
            let Some(name) = args.get(1) else {
                eprintln!(
                    "usage: asio-probe stream <name|index> [--seconds S] [--buffer N] [--engine-tracks N] [--tone]"
                );
                std::process::exit(2);
            };
            let options = stream::StreamOptions {
                driver: name.clone(),
                seconds: flag("--seconds")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(10.0),
                tone: args.iter().any(|a| a == "--tone"),
                engine_tracks: flag("--engine-tracks")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                buffer_size: flag("--buffer").and_then(|s| s.parse().ok()),
                channels: 2,
            };
            if let Err(message) = stream::run(&options) {
                eprintln!("stream failed: {message}");
                std::process::exit(1);
            }
        }
        Some(other) => {
            eprintln!("unknown command {other:?}; use list, query or stream");
            std::process::exit(2);
        }
    }
}

/// NUL-terminated UTF-16 for Win32 calls.
#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
