//! The `lumen` binary: everything lives in the `lumen_app` library, so the
//! end-to-end tests in `tests/` can start the same app.

fn main() -> eframe::Result {
    lumen_app::run()
}
