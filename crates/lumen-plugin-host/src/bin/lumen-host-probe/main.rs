//! Runs installed VST3 plugins through the stages a host needs from them
//! (scan, load, buses, configure, process, link, state, parameters, editor,
//! shutdown), each in a host process of its own, so a plugin that crashes
//! or hangs is a result and not the end of the run. The compatibility table
//! (`docs/PLUGIN_COMPATIBILITY.md`) is written from its output. See
//! `docs/manual-tests/M6.md`, MT-VST-7.

mod report;
mod stages;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use lumen_plugin_host::scan::{ScanOutcome, ScanReport, run_scan};
use report::{Found, Kind, Stage, Verdict, table_header};
use stages::{Setup, probe_class};

const HELP: &str = "\
Usage: lumen-host-probe [options] <module or folder>...

Runs every audio class of each VST3 module through the stages and prints
what happened. A folder is searched for .vst3 modules. Exits with 1 when a
plugin did not pass every stage.

Options:
  --host <path>      the plugin host program (default: beside this program)
  --rate <hz>        sample rate (default 48000)
  --frames <n>       block size (default 128)
  --blocks <n>       blocks rendered by the process and link stages (default 200)
  --deadline <secs>  how long any one call may take (default 20)
  --no-editor        do not open the plugin's window
  --json <file>      add one line of JSON per class to this file
  --table            also print the rows of the compatibility table
";

struct Options {
    setup: Setup,
    modules: Vec<PathBuf>,
    json: Option<PathBuf>,
    table: bool,
}

fn host_name() -> &'static str {
    if cfg!(windows) {
        "lumen-plugin-host.exe"
    } else {
        "lumen-plugin-host"
    }
}

fn parse(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(host_name())));
    let mut setup = Setup {
        host: beside.unwrap_or_else(|| PathBuf::from(host_name())),
        rate: 48_000.0,
        frames: 128,
        deadline: Duration::from_secs(20),
        blocks: 200,
        editor: true,
    };
    let (mut modules, mut json, mut table) = (Vec::new(), None, false);
    let mut args = args;
    while let Some(arg) = args.next() {
        let mut value = |what: &str| args.next().ok_or_else(|| format!("{arg} needs {what}"));
        match arg.as_str() {
            "--host" => setup.host = value("a path")?.into(),
            "--rate" => setup.rate = number(&arg, &value("a number")?)?,
            "--frames" => setup.frames = number(&arg, &value("a number")?)?,
            "--blocks" => setup.blocks = number(&arg, &value("a number")?)?,
            "--deadline" => {
                setup.deadline = Duration::from_secs_f64(number(&arg, &value("a number")?)?)
            }
            "--no-editor" => setup.editor = false,
            "--json" => json = Some(PathBuf::from(value("a file")?)),
            "--table" => table = true,
            "-h" | "--help" => return Err(String::new()),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path => modules.push(PathBuf::from(path)),
        }
    }
    if modules.is_empty() {
        return Err("no module given".into());
    }
    if !(8_000.0..=384_000.0).contains(&setup.rate) || !(16..=8192).contains(&setup.frames) {
        return Err("the rate or the block size is out of range".into());
    }
    Ok(Options {
        setup,
        modules,
        json,
        table,
    })
}

fn number<T: std::str::FromStr>(flag: &str, text: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("{flag}: `{text}` is not a number"))
}

/// The `.vst3` modules in `folder`, looking inside folders that are not
/// modules themselves (vendor folders), a few levels down.
fn find_modules(folder: &Path, depth: u32, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let module = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("vst3"));
        if module {
            found.push(path);
        } else if path.is_dir() && depth > 0 {
            find_modules(&path, depth - 1, found);
        }
    }
}

fn is_module(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("vst3"))
}

/// A module that could not be scanned, as one result.
fn unscanned(module: &Path, verdict: Verdict, detail: String, ms: u32) -> Found {
    let mut stages = vec![Stage {
        name: "scan",
        verdict,
        detail,
        ms,
    }];
    for &name in &Found::STAGES[1..] {
        stages.push(Stage {
            name,
            verdict: Verdict::Skipped,
            detail: "the module did not scan".into(),
            ms: 0,
        });
    }
    Found {
        module: module.display().to_string(),
        name: module
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
        vendor: String::new(),
        subcategories: String::new(),
        version: String::new(),
        kind: Kind::Effect,
        stages,
    }
}

fn probe_module(setup: &Setup, module: &Path) -> Vec<Found> {
    let began = std::time::Instant::now();
    let cancel = AtomicBool::new(false);
    let outcome = run_scan(&setup.host, module, setup.deadline, &[], &cancel);
    let ms = u32::try_from(began.elapsed().as_millis()).unwrap_or(u32::MAX);
    let report: ScanReport = match outcome {
        ScanOutcome::Scanned(report) => report,
        ScanOutcome::Failed(why) => return vec![unscanned(module, Verdict::Fail, why, ms)],
        ScanOutcome::Crashed { how, detail } => {
            return vec![unscanned(
                module,
                Verdict::Crashed,
                format!("{how} {detail}").trim().to_owned(),
                ms,
            )];
        }
        ScanOutcome::TimedOut => {
            return vec![unscanned(
                module,
                Verdict::TimedOut,
                "no answer by the deadline".into(),
                ms,
            )];
        }
        ScanOutcome::Cancelled => {
            return vec![unscanned(module, Verdict::Fail, "cancelled".into(), ms)];
        }
        ScanOutcome::Malformed(why) | ScanOutcome::Unstarted(why) => {
            return vec![unscanned(module, Verdict::Fail, why, ms)];
        }
    };
    let audio: Vec<_> = report
        .classes
        .iter()
        .filter(|c| c.category == "Audio Module Class")
        .collect();
    if audio.is_empty() {
        return vec![unscanned(
            module,
            Verdict::Fail,
            format!("{} classes, none an audio module", report.classes.len()),
            ms,
        )];
    }
    audio
        .into_iter()
        .map(|class| {
            let kind = if class.subcategories.contains("Instrument") {
                Kind::Instrument
            } else {
                Kind::Effect
            };
            let mut stages = vec![Stage {
                name: "scan",
                verdict: Verdict::Pass,
                detail: format!(
                    "{} classes, loaded in {} ms (SDK {})",
                    report.classes.len(),
                    report.load_ms,
                    class.sdk_version
                ),
                ms,
            }];
            stages.extend(probe_class(setup, module, class, kind));
            Found {
                module: module.display().to_string(),
                name: class.name.clone(),
                vendor: if class.vendor.is_empty() {
                    report.vendor.clone()
                } else {
                    class.vendor.clone()
                },
                subcategories: class.subcategories.clone(),
                version: class.version.clone(),
                kind,
                stages,
            }
        })
        .collect()
}

fn main() -> ExitCode {
    let options = match parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            if message.is_empty() {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            eprintln!("lumen-host-probe: {message}\n\n{HELP}");
            return ExitCode::from(2);
        }
    };
    if let Err(why) = lumen_plugin_host::scan::check_host(&options.setup.host) {
        eprintln!("lumen-host-probe: {why}");
        return ExitCode::from(2);
    }
    let mut modules = Vec::new();
    for path in &options.modules {
        if path.is_dir() && !is_module(path) {
            find_modules(path, 3, &mut modules);
        } else {
            modules.push(path.clone());
        }
    }
    let mut json = options.json.as_ref().map(|path| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap_or_else(|e| {
                eprintln!("lumen-host-probe: {}: {e}", path.display());
                std::process::exit(2)
            })
    });
    let mut everything = Vec::new();
    for module in &modules {
        for found in probe_module(&options.setup, module) {
            print!("{}", found.lines());
            if let Some(file) = &mut json {
                let _ = writeln!(file, "{}", found.json());
            }
            everything.push(found);
        }
    }
    println!(
        "\n{} of {} classes passed every stage",
        everything.iter().filter(|f| f.compatible()).count(),
        everything.len()
    );
    if options.table && !everything.is_empty() {
        println!("\n{}", table_header());
        for (i, found) in everything.iter().enumerate() {
            println!("{}", found.row(&format!("Probe {}", i + 1)));
        }
    }
    if everything.iter().all(Found::compatible) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
