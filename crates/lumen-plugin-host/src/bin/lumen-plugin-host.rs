//! The plugin-host process: serves one plugin to Lumen over stdin and
//! stdout. Started by Lumen, never by hand.

use std::io::{self, BufReader, BufWriter};
use std::process::ExitCode;

use lumen_plugin_host::process;
use lumen_plugin_host::server::{Exit, serve};

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

    // The protocol owns the real stdout; whatever a plugin prints goes to
    // stderr, where it cannot corrupt a frame.
    let protocol = process::take_stdout();

    // `scan <path>`: describe one module and exit (the plugin browser's
    // discovery). Anything else is a host session over stdin and stdout.
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == "scan") {
        return scan(args.next(), protocol);
    }
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(protocol);
    match serve(&mut input, &mut output) {
        Ok(Exit::Quit | Exit::Closed) => ExitCode::SUCCESS,
        Ok(Exit::Malformed) => ExitCode::from(2),
        Err(e) => {
            eprintln!("lumen-plugin-host: {e}");
            ExitCode::from(1)
        }
    }
}
