use std::io::{self, BufReader, BufWriter};
use std::process::ExitCode;

#[cfg(not(any(windows, target_os = "macos")))]
use lumen_plugin_host::server::Plain;
use lumen_plugin_host::server::{Exit, serve_pumped};
use lumen_plugin_host::{editor, process};

fn scan(path: Option<std::ffi::OsString>, mut out: std::fs::File) -> ExitCode {
    use std::io::Write;
    let Some(path) = path else {
        eprintln!("lumen-plugin-host: scan needs a module path");
        return ExitCode::from(2);
    };
    let reply = lumen_plugin_host::scan::host::scan_module(std::path::Path::new(&path));
    match out.write_all(&reply.encode()).and_then(|()| out.flush()) {
        Ok(()) => process::end_now(0),
        Err(e) => {
            eprintln!("lumen-plugin-host: {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    process::die_with_parent();
    #[cfg(windows)]
    process::quiet_crashes();
    #[cfg(windows)]
    process::report_crashes();
    #[cfg(windows)]
    process::init_com(true);

    let protocol = process::take_stdout();

    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == "scan") {
        return scan(args.next(), protocol);
    }
    #[cfg(windows)]
    process::run_at_full_speed();
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
    process::end_now(code)
}
