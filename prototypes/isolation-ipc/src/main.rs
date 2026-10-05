//! P-ISO prototype: is a shared-memory transport to separate plugin-host
//! processes fast enough to be the default, and can crashes and hangs be
//! survived without stalling audio?
//!
//! ```text
//! isolation-ipc all                       the standard measurement matrix
//! isolation-ipc bench [--hosts N] [--spin] [--frames F] [--rate R] [--seconds S]
//! isolation-ipc fault crash|hang [--hosts N] [--seconds S]
//! ```

#[cfg(windows)]
mod ipc;
#[cfg(windows)]
mod scenarios;

lumen_rt::install_guarded_allocator!();

#[cfg(not(windows))]
fn main() {
    eprintln!("isolation-ipc only runs on Windows (POSIX transport is M6 work)");
}

#[cfg(windows)]
fn main() {
    use scenarios::{Config, Fault};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
    };
    let number =
        |name: &str, default: f64| value(name).and_then(|v| v.parse().ok()).unwrap_or(default);
    let base = Config {
        hosts: number("--hosts", 1.0) as usize,
        spin: args.iter().any(|a| a == "--spin"),
        frames: number("--frames", 128.0) as usize,
        rate: number("--rate", 48_000.0),
        seconds: number("--seconds", 20.0),
        deadline_fraction: number("--deadline", 0.5),
        fault: None,
    };
    // Each scenario gets a fresh thread, as a real audio thread would; MMCSS
    // registration is per thread.
    let run = |title: &str, config: &Config| {
        let owned = config.clone();
        let result = std::thread::spawn(move || scenarios::run(&owned))
            .join()
            .expect("scenario thread panicked");
        match result {
            Ok(report) => scenarios::print(title, config, &report),
            Err(e) => println!("\n== {title}\n   FAILED: {e}"),
        }
    };

    match args.first().map(String::as_str) {
        Some("child") => {
            let (Some(name), Some(mode)) = (args.get(1), args.get(2)) else {
                std::process::exit(2)
            };
            ipc::child_main(name, mode == "spin");
        }
        Some("bench") => run("bench", &base),
        Some("fault") => {
            let fault = match args.get(1).map(String::as_str) {
                Some("hang") => Fault::Hang,
                _ => Fault::Crash,
            };
            let config = Config {
                hosts: base.hosts.max(2),
                fault: Some((fault, 1_000)),
                ..base
            };
            run(
                &format!("{fault:?} injected into host 0 at block 1000"),
                &config,
            );
        }
        None | Some("all") => {
            let seconds = base.seconds;
            for (title, config) in [
                (
                    "1 host, events",
                    Config {
                        hosts: 1,
                        ..base.clone()
                    },
                ),
                (
                    "1 host, spinning (lower bound)",
                    Config {
                        hosts: 1,
                        spin: true,
                        ..base.clone()
                    },
                ),
                (
                    "4 hosts, events",
                    Config {
                        hosts: 4,
                        ..base.clone()
                    },
                ),
                (
                    "8 hosts, events",
                    Config {
                        hosts: 8,
                        ..base.clone()
                    },
                ),
                (
                    "16 hosts, events",
                    Config {
                        hosts: 16,
                        ..base.clone()
                    },
                ),
                (
                    "8 hosts, events, 64 frames",
                    Config {
                        hosts: 8,
                        frames: 64,
                        ..base.clone()
                    },
                ),
                (
                    "crash: host 0 of 2 dies at block 1000",
                    Config {
                        hosts: 2,
                        fault: Some((Fault::Crash, 1_000)),
                        seconds: seconds.min(10.0),
                        ..base.clone()
                    },
                ),
                (
                    "hang: host 0 of 2 stops responding at block 1000",
                    Config {
                        hosts: 2,
                        fault: Some((Fault::Hang, 1_000)),
                        seconds: seconds.min(10.0),
                        ..base.clone()
                    },
                ),
            ] {
                run(title, &config);
            }
        }
        Some(other) => {
            eprintln!("unknown command {other:?}; use all, bench or fault");
            std::process::exit(2);
        }
    }
}
