//! Measures the audio path between Lumen and plugin-host processes: the
//! wake-up in each direction, how many hosts, how short the block, how busy
//! the machine. Run it on the machine whose answer matters (Windows, ASIO
//! buffer sizes) and keep the output with the milestone's records. See
//! `docs/manual-tests/M6.md`, MT-VST-0.

mod options;
mod platform;
mod scenario;
mod stats;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use options::{HELP, Options};
use scenario::{Report, Scenario, run};
use stats::Sorted;

lumen_rt::install_guarded_allocator!();

fn sibling(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join(name);
    path.is_file().then_some(path)
}

fn host_name() -> &'static str {
    if cfg!(windows) {
        "lumen-plugin-host.exe"
    } else {
        "lumen-plugin-host"
    }
}

fn plugin_name() -> &'static str {
    if cfg!(windows) {
        "lumen_test_plugin.dll"
    } else if cfg!(target_os = "macos") {
        "liblumen_test_plugin.dylib"
    } else {
        "liblumen_test_plugin.so"
    }
}

fn summary(report: &Report, period_us: f64, deadline_us: f64) -> Vec<String> {
    let mut lines = Vec::new();
    let all = Sorted::of(&report.block_ns);
    lines.push(format!(
        "block time (submit to all answers): {}",
        all.line()
    ));
    if !report.cold_ns.is_empty() {
        let cold = Sorted::of(&report.cold_ns);
        lines.push(format!(
            "  first {} blocks after the cold start: {}",
            report.cold_ns.len(),
            cold.line()
        ));
    }
    let sum = |v: &[u32]| v.iter().sum::<u32>();
    lines.push(format!(
        "deadline {deadline_us:.0} µs of {period_us:.0}: misses {} / busy skips {} / stale answers {} / failed {} / divergent {} / loop overruns {}",
        sum(&report.misses),
        sum(&report.busy),
        sum(&report.stale),
        sum(&report.failed),
        report.divergent,
        report.overruns
    ));
    lines.push(format!(
        "heap operations in Lumen's loop: {}",
        report.heap_ops
    ));
    if !report.idle_cpu.is_empty() {
        let mean = report.idle_cpu.iter().sum::<f64>() / report.idle_cpu.len() as f64;
        let worst = report.idle_cpu.iter().copied().fold(0.0, f64::max);
        lines.push(format!(
            "idle host CPU: mean {mean:.1}% / worst {worst:.1}% of a core each"
        ));
    }
    if let Some(after) = report.detected_after {
        lines.push(format!(
            "fault in host 0 seen after {after} block(s) ({:.1} ms): {}",
            after as f64 * period_us / 1e3,
            report.verdict.as_deref().unwrap_or("?")
        ));
        let others: u32 = report.misses.iter().skip(1).sum();
        lines.push(format!("misses in the other hosts: {others}"));
    }
    if let Some(ms) = report.restart_ms {
        lines.push(format!("a replacement host answering its first block: {ms:.0} ms (cold start of process, plugin, link)"));
    }
    if !report.mmcss {
        lines.push(
            "note: no audio scheduling class for this thread (Windows: MMCSS \"Pro Audio\")".into(),
        );
    }
    for note in &report.notes {
        lines.push(format!("note: {note}"));
    }
    lines
}

fn main() -> ExitCode {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(Some(o)) => o,
        Ok(None) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::from(2);
        }
    };
    let Some(host_exe) = options.host.clone().or_else(|| sibling(host_name())) else {
        eprintln!(
            "the host executable was not found beside this one; build it with `cargo build --release -p lumen-plugin-host` or pass --host"
        );
        return ExitCode::from(2);
    };
    let Some(plugin) = options.plugin.clone().or_else(|| sibling(plugin_name())) else {
        eprintln!(
            "no plugin: build the test plugin with `cargo build --release -p lumen-test-plugin` or pass --plugin"
        );
        return ExitCode::from(2);
    };

    println!(
        "lumen-host-bench: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!(
        "logical CPUs: {}",
        std::thread::available_parallelism().map_or(0, |n| n.get())
    );
    for line in platform::describe() {
        println!("{line}");
    }
    println!(
        "host: {}\nplugin: {}\n",
        host_exe.display(),
        plugin.display()
    );

    let mut csv = options.csv.as_ref().map(|path| {
        let mut file = std::fs::File::create(path).unwrap_or_else(|e| {
            eprintln!("{}: {e}", path.display());
            std::process::exit(2);
        });
        let _ = writeln!(
            file,
            "hosts,frames,rate,wake,load_us,contention,p50_us,p99_us,p999_us,max_us,misses,busy,stale,failed,divergent,overruns,heap_ops,idle_cpu_mean_pct"
        );
        file
    });

    let mut problems = 0;
    for &contention in &options.contention {
        for &load_us in &options.loads {
            for &rate in &options.rates {
                for &frames in &options.frames {
                    for &hosts in &options.hosts {
                        for (name, wake) in &options.wakes {
                            let period_us = f64::from(frames) / rate * 1e6;
                            let scenario = Scenario {
                                host_exe: &host_exe,
                                plugin: &plugin,
                                hosts,
                                frames,
                                rate,
                                wake: *wake,
                                load_us,
                                contention,
                                seconds: options.seconds,
                                warmup: options.warmup,
                                idle: options.idle,
                                cold: options.cold,
                                deadline: options.deadline,
                                fault: options.fault,
                            };
                            println!(
                                "== {hosts} host(s), {frames} frames @ {rate} Hz, wake {name}, plugin load {load_us} µs, {contention} busy thread(s)"
                            );
                            match run(&scenario) {
                                Ok(report) => {
                                    for line in
                                        summary(&report, period_us, period_us * options.deadline)
                                    {
                                        println!("   {line}");
                                    }
                                    if let Some(file) = csv.as_mut() {
                                        let s = Sorted::of(&report.block_ns);
                                        let sum = |v: &[u32]| v.iter().sum::<u32>();
                                        let idle = if report.idle_cpu.is_empty() {
                                            String::new()
                                        } else {
                                            format!(
                                                "{:.2}",
                                                report.idle_cpu.iter().sum::<f64>()
                                                    / report.idle_cpu.len() as f64
                                            )
                                        };
                                        let _ = writeln!(
                                            file,
                                            "{hosts},{frames},{rate},{name},{load_us},{contention},{:.1},{:.1},{:.1},{:.1},{},{},{},{},{},{},{},{idle}",
                                            s.at(0.5),
                                            s.at(0.99),
                                            s.at(0.999),
                                            s.at(1.0),
                                            sum(&report.misses),
                                            sum(&report.busy),
                                            sum(&report.stale),
                                            sum(&report.failed),
                                            report.divergent,
                                            report.overruns,
                                            report.heap_ops
                                        );
                                    }
                                    if report.heap_ops > 0 || report.divergent > 0 {
                                        problems += 1;
                                    }
                                }
                                Err(why) => {
                                    println!("   did not run: {why}");
                                    problems += 1;
                                }
                            }
                            println!();
                            std::thread::sleep(Duration::from_millis(200));
                        }
                    }
                }
            }
        }
    }
    if problems > 0 {
        println!(
            "{problems} scenario(s) did not run or broke an invariant (heap operations in the loop, or hosts disagreeing)"
        );
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
