//! The editor-focus spike of M6 slice 0 (plan §2, "Plugin editors freeze
//! focus or Lumen"; ADR-0005: editors are top-level windows the host process
//! makes, never windows parented across processes).
//!
//! One program, two roles:
//!
//! - `run` is a stand-in for Lumen: a window of its own that pumps messages
//!   on its main thread, starts an editor host as a child process, drives it
//!   and reports what happened. It measures the stand-in window's own stalls,
//!   which is the number that matters: does a plugin's editor ever freeze
//!   Lumen.
//! - `host` is the editor host, started by `run`: it loads a plugin, makes the
//!   editor window on its main thread (the one that must pump messages, and
//!   on macOS the only one allowed to) and takes commands on stdin.
//!
//! Windows only for now. See `docs/manual-tests/M6.md`, MT-VST-0 (editors).

#[cfg(windows)]
mod host;
#[cfg(windows)]
mod lead;
#[cfg(windows)]
mod link;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let role = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();
    match role.as_str() {
        "run" => lead::run(&rest),
        "host" => host::run(&rest),
        _ => {
            eprintln!("{HELP}");
            std::process::ExitCode::from(2)
        }
    }
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!(
        "lumen-editor-spike: Windows only for now (macOS and Linux editors come with slice 4)"
    );
    std::process::ExitCode::from(2)
}

#[cfg(windows)]
const HELP: &str = "\
lumen-editor-spike: the editor-focus spike (M6 slice 0).

  run   [--owner none|owned|top] [--plugin PATH] [--class NAME]
        [--manual] [--skip-destroy]
        A stand-in for Lumen: starts an editor host, runs the scenarios and
        prints results. --owner picks the window relationship:
          none   an unowned top-level window (option A)
          owned  owned by the stand-in's window across processes (option B)
          top    unowned and always on top (option C)
        --manual opens the editor and leaves it for you to use (type in it,
        press Space, drag it to another monitor); close the stand-in window
        to finish. --skip-destroy leaves out the last scenario, which
        destroys the stand-in's window while an owned editor is hung.
  host  (started by `run`; not for hand use)
";
