//! The plugin-host process: serves one plugin to Lumen over stdin and
//! stdout. Started by Lumen, never by hand.

use std::io::{self, BufReader, BufWriter};
use std::process::ExitCode;

use lumen_plugin_host::process;
use lumen_plugin_host::server::{Exit, serve};

fn main() -> ExitCode {
    process::die_with_parent();
    // A crash must end this process, not park it behind an error dialog.
    #[cfg(windows)]
    process::quiet_crashes();

    // The protocol owns the real stdout; whatever a plugin prints goes to
    // stderr, where it cannot corrupt a frame.
    let protocol = process::take_stdout();
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
