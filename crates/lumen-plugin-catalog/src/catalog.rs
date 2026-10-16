//! The list the browser shows: every device a track can hold, from the
//! cache and from Lumen itself, and the questions asked of it.
//!
//! A device's id is what a project will store to find it again:
//! `builtin:<name>` for Lumen's own, `vst3:<class id>` for a plugin, the
//! class id being 32 hexadecimal digits of the factory's 16 bytes.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::blocklist::display_name;
use crate::cache::{Cache, Reason, Result};

/// The factory category of a class that processes audio.
const AUDIO_CLASS: &str = "Audio Module Class";

/// The class id as 32 uppercase hexadecimal digits.
pub fn class_id(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02X}")).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Instrument,
    Effect,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Instrument => "Instrument",
            Kind::Effect => "Effect",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Builtin,
    Vst3,
}

/// One thing that can go on a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub kind: Kind,
    pub source: Source,
    /// What it is for (`Synth`, `Reverb`...), without `Fx` or `Instrument`.
    pub categories: Vec<String>,
    pub version: String,
    /// The module a plugin lives in; none for a built-in device.
    pub module: Option<PathBuf>,
}

/// A module Lumen will not load, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked {
    pub name: String,
    pub path: PathBuf,
    pub reason: Reason,
    pub at: u64,
}

/// Lumen's own devices. The synth is the only one until M8.
pub fn builtin_devices() -> Vec<Device> {
    vec![Device {
        id: "builtin:synth".into(),
        name: "Synth".into(),
        vendor: "Lumen".into(),
        kind: Kind::Instrument,
        source: Source::Builtin,
        categories: vec!["Synth".into()],
        version: String::new(),
        module: None,
    }]
}

/// Kind and categories from a factory's `Instrument|Synth` style text.
fn classify(subcategories: &str) -> (Kind, Vec<String>) {
    let parts: Vec<&str> = subcategories
        .split('|')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let kind = if parts.iter().any(|p| p.eq_ignore_ascii_case("Instrument")) {
        Kind::Instrument
    } else {
        Kind::Effect
    };
    let mut categories: Vec<String> = Vec::new();
    for part in parts {
        let generic = part.eq_ignore_ascii_case("Fx") || part.eq_ignore_ascii_case("Instrument");
        if !generic && !categories.iter().any(|c| c.eq_ignore_ascii_case(part)) {
            categories.push(part.to_string());
        }
    }
    (kind, categories)
}

/// What to ask the catalog.
#[derive(Debug, Default, Clone, Copy)]
pub struct Query<'a> {
    /// Words to find; every word must match somewhere. Empty matches all.
    pub text: &'a str,
    pub kind: Option<Kind>,
    /// Only these ids (favorites), when set.
    pub only: Option<&'a HashSet<String>>,
}

#[derive(Debug, Default, Clone)]
pub struct Catalog {
    devices: Vec<Device>,
    blocked: Vec<Blocked>,
}

impl Catalog {
    /// The devices in `cache` and in `builtins`, and the blocked modules.
    pub fn build(cache: &Cache, builtins: Vec<Device>) -> Self {
        let mut devices = builtins;
        let mut seen: HashSet<String> = devices.iter().map(|d| d.id.clone()).collect();
        let mut blocked = Vec::new();
        for entry in cache.entries() {
            match &entry.result {
                Result::Scanned {
                    vendor, classes, ..
                } => {
                    for class in classes.iter().filter(|c| c.category == AUDIO_CLASS) {
                        let id = format!("vst3:{}", class.id);
                        // The same plugin installed twice is one device.
                        if !seen.insert(id.clone()) {
                            continue;
                        }
                        let (kind, categories) = classify(&class.subcategories);
                        devices.push(Device {
                            id,
                            name: class.name.clone(),
                            vendor: if class.vendor.is_empty() {
                                vendor.clone()
                            } else {
                                class.vendor.clone()
                            },
                            kind,
                            source: Source::Vst3,
                            categories,
                            version: class.version.clone(),
                            module: Some(PathBuf::from(&entry.path)),
                        });
                    }
                }
                Result::Blocked { reason, at } => blocked.push(Blocked {
                    name: display_name(entry),
                    path: PathBuf::from(&entry.path),
                    reason: reason.clone(),
                    at: *at,
                }),
            }
        }
        devices.sort_by_cached_key(|d| (d.name.to_lowercase(), d.id.clone()));
        blocked.sort_by_cached_key(|b| b.name.to_lowercase());
        Self { devices, blocked }
    }

    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    pub fn blocked(&self) -> &[Blocked] {
        &self.blocked
    }

    pub fn find(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    /// The devices that match, best first and then by name.
    pub fn search(&self, query: &Query<'_>) -> Vec<&Device> {
        let words: Vec<String> = query
            .text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let mut hits: Vec<(u32, &Device)> = self
            .devices
            .iter()
            .filter(|d| query.kind.is_none_or(|k| d.kind == k))
            .filter(|d| query.only.is_none_or(|only| only.contains(&d.id)))
            .filter_map(|d| rank(d, &words).map(|score| (score, d)))
            .collect();
        // The device list is already by name, and the sort is stable.
        hits.sort_by_key(|(score, _)| *score);
        hits.into_iter().map(|(_, d)| d).collect()
    }

    /// The vendors present, by name, with how many devices each has.
    pub fn vendors(&self) -> Vec<(String, usize)> {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for device in &self.devices {
            match counts
                .iter_mut()
                .find(|(v, _)| v.eq_ignore_ascii_case(&device.vendor))
            {
                Some((_, n)) => *n += 1,
                None => counts.push((device.vendor.clone(), 1)),
            }
        }
        counts.sort_by_cached_key(|(v, _)| v.to_lowercase());
        counts
    }
}

/// How well a device matches every word (lower is better), or `None`.
fn rank(device: &Device, words: &[String]) -> Option<u32> {
    let name = device.name.to_lowercase();
    let vendor = device.vendor.to_lowercase();
    let categories = device.categories.join(" ").to_lowercase();
    let kind = match device.kind {
        Kind::Instrument => "instrument",
        Kind::Effect => "effect fx",
    };
    let mut total = 0;
    for word in words {
        let score = if name.starts_with(word.as_str()) {
            0
        } else if name
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w.starts_with(word.as_str()))
        {
            1
        } else if name.contains(word.as_str()) {
            2
        } else if vendor.contains(word.as_str()) || categories.contains(word.as_str()) {
            3
        } else if kind.contains(word.as_str()) {
            4
        } else {
            return None;
        };
        total += score;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{ClassRecord, Entry};
    use crate::stamp::Stamp;

    fn class(id: u8, name: &str, category: &str, sub: &str, vendor: &str) -> ClassRecord {
        ClassRecord {
            id: class_id(&[id; 16]),
            name: name.into(),
            category: category.into(),
            vendor: vendor.into(),
            subcategories: sub.into(),
            version: "1.0".into(),
            sdk_version: String::new(),
        }
    }

    fn module(path: &str, vendor: &str, classes: Vec<ClassRecord>) -> Entry {
        Entry {
            path: path.into(),
            binary: path.into(),
            stamp: Stamp {
                len: 1,
                modified_ns: 1,
            },
            result: Result::Scanned {
                vendor: vendor.into(),
                url: String::new(),
                classes,
                load_ms: 1,
            },
        }
    }

    fn sample() -> Catalog {
        let mut cache = Cache::new();
        cache.set(module(
            "/p/A.vst3",
            "Vendor A",
            vec![
                class(1, "Warm Synth", AUDIO_CLASS, "Instrument|Synth", "Vendor A"),
                class(2, "Plate Reverb", AUDIO_CLASS, "Fx|Reverb", "Vendor A"),
                class(
                    3,
                    "Warm Synth",
                    "Component Controller Class",
                    "",
                    "Vendor A",
                ),
            ],
        ));
        cache.set(module(
            "/p/B.vst3",
            "Vendor B",
            vec![class(
                4,
                "Bus Compressor",
                AUDIO_CLASS,
                "Fx|Dynamics|Fx",
                "",
            )],
        ));
        cache.set(Entry {
            path: "/p/Bad.vst3".into(),
            binary: "/p/Bad.vst3".into(),
            stamp: Stamp {
                len: 1,
                modified_ns: 1,
            },
            result: Result::Blocked {
                reason: Reason::TimedOut { seconds: 30 },
                at: 9,
            },
        });
        Catalog::build(&cache, builtin_devices())
    }

    fn names(devices: Vec<&Device>) -> Vec<&str> {
        devices.into_iter().map(|d| d.name.as_str()).collect()
    }

    #[test]
    fn class_ids_are_32_uppercase_hex_digits() {
        let mut id = [0u8; 16];
        id[0] = 0xAB;
        id[15] = 0x01;
        assert_eq!(class_id(&id), "AB000000000000000000000000000001");
    }

    #[test]
    fn only_audio_classes_become_devices_and_builtins_come_with_them() {
        let catalog = sample();
        assert_eq!(
            names(catalog.devices().iter().collect()),
            ["Bus Compressor", "Plate Reverb", "Synth", "Warm Synth"]
        );
        assert!(catalog.find("builtin:synth").is_some());
        assert!(
            catalog
                .find(&format!("vst3:{}", class_id(&[1; 16])))
                .is_some()
        );
        assert!(
            catalog
                .find(&format!("vst3:{}", class_id(&[3; 16])))
                .is_none()
        );
    }

    #[test]
    fn kind_and_categories_come_from_the_sub_categories() {
        let catalog = sample();
        let synth = catalog
            .find(&format!("vst3:{}", class_id(&[1; 16])))
            .unwrap();
        assert_eq!(
            (synth.kind, synth.categories.as_slice()),
            (Kind::Instrument, &["Synth".to_string()][..])
        );
        let comp = catalog
            .find(&format!("vst3:{}", class_id(&[4; 16])))
            .unwrap();
        assert_eq!(comp.kind, Kind::Effect);
        assert_eq!(comp.categories, ["Dynamics"]);
        // The class had no vendor of its own: the factory's stands in.
        assert_eq!(comp.vendor, "Vendor B");
    }

    #[test]
    fn an_unlabelled_audio_class_is_an_effect() {
        assert_eq!(classify("").0, Kind::Effect);
        assert_eq!(classify("Spatial|Analyzer").0, Kind::Effect);
    }

    #[test]
    fn a_blocked_module_is_listed_apart_with_its_reason() {
        let catalog = sample();
        assert_eq!(catalog.blocked().len(), 1);
        assert_eq!(catalog.blocked()[0].name, "Bad");
        assert_eq!(
            catalog.blocked()[0].reason,
            Reason::TimedOut { seconds: 30 }
        );
    }

    #[test]
    fn the_same_plugin_in_two_modules_is_one_device() {
        let mut cache = Cache::new();
        let c = class(1, "Thing", AUDIO_CLASS, "Fx", "V");
        cache.set(module("/a/Thing.vst3", "V", vec![c.clone()]));
        cache.set(module("/b/Thing.vst3", "V", vec![c]));
        assert_eq!(Catalog::build(&cache, vec![]).devices().len(), 1);
    }

    #[test]
    fn search_matches_every_word_in_name_vendor_or_category() {
        let catalog = sample();
        let find = |text: &str| {
            names(catalog.search(&Query {
                text,
                ..Query::default()
            }))
        };
        assert_eq!(
            find(""),
            ["Bus Compressor", "Plate Reverb", "Synth", "Warm Synth"]
        );
        assert_eq!(find("warm"), ["Warm Synth"]);
        assert_eq!(find("REVERB"), ["Plate Reverb"]);
        assert_eq!(find("lumen"), ["Synth"]);
        assert_eq!(find("vendor compressor"), ["Bus Compressor"]);
        assert_eq!(find("dynamics"), ["Bus Compressor"]);
        assert_eq!(find("plate warm"), Vec::<&str>::new());
        assert_eq!(find("zzz"), Vec::<&str>::new());
    }

    #[test]
    fn a_name_that_starts_with_the_word_ranks_before_one_that_contains_it() {
        let mut cache = Cache::new();
        cache.set(module(
            "/p/x.vst3",
            "V",
            vec![
                class(1, "Big Synth", AUDIO_CLASS, "Instrument", "V"),
                class(2, "Synthwave", AUDIO_CLASS, "Instrument", "V"),
                class(3, "Basic Rhodes", AUDIO_CLASS, "Instrument|Synth", "V"),
            ],
        ));
        let catalog = Catalog::build(&cache, vec![]);
        let hits = names(catalog.search(&Query {
            text: "synth",
            ..Query::default()
        }));
        assert_eq!(hits, ["Synthwave", "Big Synth", "Basic Rhodes"]);
    }

    #[test]
    fn kind_and_favorites_narrow_the_result() {
        let catalog = sample();
        let instruments = names(catalog.search(&Query {
            kind: Some(Kind::Instrument),
            ..Query::default()
        }));
        assert_eq!(instruments, ["Synth", "Warm Synth"]);
        let favorites: HashSet<String> = [format!("vst3:{}", class_id(&[2; 16]))].into();
        let only = names(catalog.search(&Query {
            only: Some(&favorites),
            ..Query::default()
        }));
        assert_eq!(only, ["Plate Reverb"]);
        assert!(
            catalog
                .search(&Query {
                    only: Some(&HashSet::new()),
                    ..Query::default()
                })
                .is_empty()
        );
    }

    #[test]
    fn vendors_are_counted_and_sorted() {
        assert_eq!(
            sample().vendors(),
            [
                ("Lumen".to_string(), 1),
                ("Vendor A".to_string(), 2),
                ("Vendor B".to_string(), 1)
            ]
        );
    }
}
