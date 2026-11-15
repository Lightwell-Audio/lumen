use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub len: u64,
    pub modified_ns: u64,
}

impl Stamp {
    pub fn of(path: &Path) -> Option<Stamp> {
        let meta = std::fs::metadata(path).ok()?;
        let modified_ns = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
        Some(Stamp {
            len: meta.len(),
            modified_ns,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_changes_with_size_and_time_and_is_none_for_a_missing_file() {
        let dir = std::env::temp_dir().join(format!("lumen-stamp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.bin");
        std::fs::write(&file, b"one").unwrap();
        let first = Stamp::of(&file).unwrap();
        assert_eq!(first, Stamp::of(&file).unwrap());
        std::fs::write(&file, b"three").unwrap();
        assert_ne!(first, Stamp::of(&file).unwrap());
        assert_eq!(Stamp::of(&dir.join("missing")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
