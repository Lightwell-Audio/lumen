//! Runs one scenario by name; `perf-gate` starts it under Callgrind.

fn main() {
    let Some(name) = std::env::args().nth(1) else {
        eprintln!("usage: perf-scenario <name>");
        for (name, what) in lumen_perf::SCENARIOS {
            eprintln!("  {name:<20} {what}");
        }
        std::process::exit(2);
    };
    if !lumen_perf::run(&name) {
        eprintln!("unknown scenario {name:?}");
        std::process::exit(2);
    }
}
