//! The `lumen` binary: everything lives in the `lumen_app` library, so the
//! end-to-end tests in `tests/` can start the same app.

/// The system allocator, reporting a failed allocation as a crash before
/// the process ends (crash reports, `crash.rs`).
#[global_allocator]
static ALLOCATOR: lumen_rt::ReportingAllocator = lumen_rt::ReportingAllocator;

fn main() -> eframe::Result {
    lumen_app::run()
}
