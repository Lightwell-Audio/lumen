pub mod host;
pub mod report;
pub mod run;

pub use report::{MAX_REPLY, ScanClass, ScanReply, ScanReport};
pub use run::{ScanOutcome, check_host, run_scan};
