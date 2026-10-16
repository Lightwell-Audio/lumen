//! What scans learned, kept between runs. One entry per module, keyed by
//! its path and stamped with the library's size and modification time: an
//! entry whose stamp still matches is trusted and the module is not loaded
//! again. A module that was blocked is an entry too, with its reason, so it
//! is retried when its file changes and not before.
//!
//! The file is human-readable JSON. A cache that cannot be read, or that a
//! newer Lumen wrote, is discarded and rebuilt by scanning: it is only ever
//! a copy of what the plugins say, never anyone's work.

use serde::{Deserialize, Serialize};

use crate::stamp::Stamp;

/// Bumped when the file's shape changes. A cache of another version is
/// discarded.
pub const CACHE_VERSION: u32 = 1;

/// One class a module offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassRecord {
    /// The class id as 32 hexadecimal digits (see [`crate::catalog::class_id`]).
    pub id: String,
    pub name: String,
    pub category: String,
    pub vendor: String,
    pub subcategories: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub sdk_version: String,
}

/// Why a module is blocked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    /// Its process died, by a signal or exception or a failure code.
    Crashed { how: String, detail: String },
    /// It did not answer within `seconds`.
    TimedOut { seconds: u64 },
    /// It could not be loaded, or has no factory.
    Failed { message: String },
    /// It answered with something unreadable.
    Malformed { message: String },
}

/// What is known about one module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Result {
    Scanned {
        vendor: String,
        url: String,
        classes: Vec<ClassRecord>,
        load_ms: u32,
    },
    Blocked {
        reason: Reason,
        /// When it was blocked, in seconds since the Unix epoch.
        at: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The `.vst3` folder or file the user knows it by.
    pub path: String,
    /// The library that was loaded.
    pub binary: String,
    pub stamp: Stamp,
    pub result: Result,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
    version: u32,
    entries: Vec<Entry>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            entries: Vec::new(),
        }
    }
}

/// A cache read from bytes, and why it was thrown away if it was.
#[derive(Debug)]
pub struct Loaded {
    pub cache: Cache,
    pub discarded: Option<String>,
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads a cache. Anything unreadable gives an empty one and the reason;
    /// this never fails.
    pub fn load(bytes: &[u8]) -> Loaded {
        let discard = |why: String| Loaded {
            cache: Cache::new(),
            discarded: Some(why),
        };
        let value: serde_json::Value = match serde_json::from_slice(bytes) {
            Ok(value) => value,
            Err(e) => return discard(format!("the plugin cache is damaged ({e})")),
        };
        match value.get("version").and_then(serde_json::Value::as_u64) {
            Some(v) if v == u64::from(CACHE_VERSION) => {}
            Some(v) => {
                return discard(format!(
                    "the plugin cache is version {v}, this Lumen reads {CACHE_VERSION}"
                ));
            }
            None => return discard("the plugin cache has no version".into()),
        }
        match serde_json::from_value::<Cache>(value) {
            Ok(mut cache) => {
                cache.sort();
                Loaded {
                    cache,
                    discarded: None,
                }
            }
            Err(e) => discard(format!("the plugin cache is damaged ({e})")),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).expect("a cache is plain data and serializes")
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, path: &str) -> Option<&Entry> {
        self.entries
            .binary_search_by(|e| e.path.as_str().cmp(path))
            .ok()
            .map(|i| &self.entries[i])
    }

    /// Adds or replaces the entry for its path.
    pub fn set(&mut self, entry: Entry) {
        match self
            .entries
            .binary_search_by(|e| e.path.as_str().cmp(&entry.path))
        {
            Ok(i) => self.entries[i] = entry,
            Err(i) => self.entries.insert(i, entry),
        }
    }

    /// Forgets a module, so the next scan loads it again whatever its stamp:
    /// how Retry and "rescan this plugin" work. True if it was known.
    pub fn forget(&mut self, path: &str) -> bool {
        match self.entries.binary_search_by(|e| e.path.as_str().cmp(path)) {
            Ok(i) => {
                self.entries.remove(i);
                true
            }
            Err(_) => false,
        }
    }

    /// Keeps only entries for which `keep` is true; returns how many went.
    pub fn retain(&mut self, mut keep: impl FnMut(&Entry) -> bool) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| keep(e));
        before - self.entries.len()
    }

    /// The modules that are blocked.
    pub fn blocked(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.result, Result::Blocked { .. }))
    }

    fn sort(&mut self) {
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        self.entries.dedup_by(|a, b| a.path == b.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scanned(path: &str, len: u64) -> Entry {
        Entry {
            path: path.into(),
            binary: format!("{path}/bin"),
            stamp: Stamp {
                len,
                modified_ns: 7,
            },
            result: Result::Scanned {
                vendor: "Vendor".into(),
                url: String::new(),
                classes: vec![ClassRecord {
                    id: "00".repeat(16),
                    name: "Thing".into(),
                    category: "Audio Module Class".into(),
                    vendor: "Vendor".into(),
                    subcategories: "Fx".into(),
                    version: String::new(),
                    sdk_version: String::new(),
                }],
                load_ms: 12,
            },
        }
    }

    fn blocked(path: &str) -> Entry {
        Entry {
            result: Result::Blocked {
                reason: Reason::TimedOut { seconds: 30 },
                at: 1,
            },
            ..scanned(path, 1)
        }
    }

    #[test]
    fn a_cache_round_trips_and_stays_sorted_by_path() {
        let mut cache = Cache::new();
        cache.set(scanned("b", 1));
        cache.set(blocked("a"));
        cache.set(scanned("c", 3));
        let loaded = Cache::load(&cache.to_bytes());
        assert_eq!(loaded.discarded, None);
        assert_eq!(loaded.cache, cache);
        let paths: Vec<_> = loaded
            .cache
            .entries()
            .iter()
            .map(|e| e.path.as_str())
            .collect();
        assert_eq!(paths, ["a", "b", "c"]);
    }

    #[test]
    fn set_replaces_and_forget_removes() {
        let mut cache = Cache::new();
        cache.set(scanned("a", 1));
        cache.set(scanned("a", 2));
        assert_eq!(cache.entries().len(), 1);
        assert_eq!(cache.entry("a").unwrap().stamp.len, 2);
        assert!(cache.forget("a"));
        assert!(!cache.forget("a"));
        assert!(cache.entry("a").is_none());
    }

    #[test]
    fn blocked_lists_only_blocked_entries() {
        let mut cache = Cache::new();
        cache.set(scanned("a", 1));
        cache.set(blocked("b"));
        let names: Vec<_> = cache.blocked().map(|e| e.path.as_str()).collect();
        assert_eq!(names, ["b"]);
    }

    #[test]
    fn retain_drops_what_is_not_kept() {
        let mut cache = Cache::new();
        cache.set(scanned("a", 1));
        cache.set(scanned("b", 1));
        assert_eq!(cache.retain(|e| e.path == "a"), 1);
        assert_eq!(cache.entries().len(), 1);
    }

    #[test]
    fn damaged_empty_and_unversioned_caches_are_discarded_not_fatal() {
        for bytes in [&b""[..], b"{ not json", b"[]", b"{}", b"null"] {
            let loaded = Cache::load(bytes);
            assert!(loaded.cache.entries().is_empty());
            assert!(
                loaded.discarded.is_some(),
                "{:?}",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn a_cache_of_another_version_is_discarded_with_the_versions_named() {
        let bytes = br#"{ "version": 99, "entries": [] }"#;
        let loaded = Cache::load(bytes);
        assert!(loaded.cache.entries().is_empty());
        let why = loaded.discarded.unwrap();
        assert!(why.contains("99") && why.contains('1'), "{why}");
    }

    #[test]
    fn a_right_version_with_the_wrong_shape_is_discarded() {
        let bytes = br#"{ "version": 1, "entries": [ { "path": 3 } ] }"#;
        assert!(Cache::load(bytes).discarded.is_some());
    }

    #[test]
    fn duplicate_paths_in_a_file_collapse() {
        let mut cache = Cache::new();
        cache.set(scanned("a", 1));
        let mut value: serde_json::Value = serde_json::from_slice(&cache.to_bytes()).unwrap();
        let first = value["entries"][0].clone();
        value["entries"].as_array_mut().unwrap().push(first);
        let loaded = Cache::load(&serde_json::to_vec(&value).unwrap());
        assert_eq!(loaded.cache.entries().len(), 1);
    }
}
