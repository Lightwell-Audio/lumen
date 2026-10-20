//! The plugin-host process: serves one plugin to Lumen over stdin and
//! stdout. Started by Lumen, never by hand.

use std::io::{self, BufReader, BufWriter};
use std::process::ExitCode;

#[cfg(not(any(windows, target_os = "macos")))]
use lumen_plugin_host::server::Plain;
use lumen_plugin_host::server::{Exit, serve_pumped};
use lumen_plugin_host::{editor, process};

/// Writes one scan reply to `out`. A module that crashes or hangs never gets
/// here, which is how the parent knows.
fn scan(path: Option<std::ffi::OsString>, mut out: std::fs::File) -> ExitCode {
    use std::io::Write;
    let Some(path) = path else {
        eprintln!("lumen-plugin-host: scan needs a module path");
        return ExitCode::from(2);
    };
    let reply = lumen_plugin_host::scan::host::scan_module(std::path::Path::new(&path));
    match out.write_all(&reply.encode()).and_then(|()| out.flush()) {
        // The answer is out. Leave without letting the module run its
        // shutdown code: some hang there and some set a failure code, which
        // would block a plugin that answered perfectly well.
        Ok(()) => process::end_now(0),
        Err(e) => {
            eprintln!("lumen-plugin-host: {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    process::die_with_parent();
    // A crash must end this process, not park it behind an error dialog.
    #[cfg(windows)]
    process::quiet_crashes();
    // And say where a crash happened, for Lumen to show.
    #[cfg(windows)]
    process::report_crashes();
    // The thread that creates plugins gets COM, as a plugin expects.
    #[cfg(windows)]
    process::init_com(true);

    // The protocol owns the real stdout; whatever a plugin prints goes to
    // stderr, where it cannot corrupt a frame.
    let protocol = process::take_stdout();

    // `scan <path>`: describe one module and exit (the plugin browser's
    // discovery). Anything else is a host session over stdin and stdout.
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == "scan") {
        return scan(args.next(), protocol);
    }
    // A host session plays audio: it runs at full speed (a scan does not).
    #[cfg(windows)]
    process::run_at_full_speed();
    // Requests are read on a thread of their own; this one made the plugin and
    // runs what is asked of it, and shows its editor window.
    let input = BufReader::new(io::stdin());
    let output = BufWriter::new(protocol);
    #[cfg(windows)]
    let mut pump = editor::win::WindowPump::new();
    #[cfg(target_os = "macos")]
    let mut pump = editor::mac::WindowPump::new();
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut pump = Plain::default();
    let code = match serve_pumped(input, output, &mut pump, editor::platform()) {
        Ok(Exit::Quit | Exit::Closed) => 0,
        Ok(Exit::Malformed) => 2,
        Err(e) => {
            eprintln!("lumen-plugin-host: {e}");
            1
        }
    };
    // `serve_pumped` has already detached audio, closed the editor, dropped
    // the instance, and joined the request reader. End without running native
    // module-level destructors: VST3 modules stay loaded until process exit,
    // and their global shutdown code is outside the host's control.
    process::end_now(code)
}
