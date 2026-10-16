//! The scan child's work: load one module, say what it offers.

use std::path::Path;
use std::time::Instant;

use crate::module::Module;
use crate::scan::ScanReply;

/// Loads the module at `path` and describes it. A module that cannot be
/// loaded is a [`ScanReply::Failed`] with the loader's message; one that
/// crashes or hangs never gets to answer, which the parent classifies.
pub fn scan_module(path: &Path) -> ScanReply {
    let started = Instant::now();
    match Module::load(path) {
        Ok(module) => {
            let load_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
            ScanReply::Report(module.scan_report(load_ms))
        }
        Err(e) => ScanReply::Failed(e.to_string()),
    }
}
