use std::collections::HashSet;
use std::path::PathBuf;

use crate::blocklist::display_name;
use crate::cache::{Cache, Reason, Result};

const AUDIO_CLASS: &str = "Audio Module Class";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    Instruments,
    Dynamics,
    Eq,
    Time,
    Utility,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::Instruments,
        Group::Dynamics,
        Group::Eq,
        Group::Time,
        Group::Utility,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::Instruments => "Instruments",
            Group::Dynamics => "Dynamics",
            Group::Eq => "EQ",
            Group::Time => "Time",
            Group::Utility => "Utility",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Group::Instruments => "instruments",
            Group::Dynamics => "dynamics",
            Group::Eq => "eq",
            Group::Time => "time",
            Group::Utility => "utility",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colour {
    Synth,
    Utility,
    Eq,
    Delay,
    Reverb,
    Gravity,
    Horizon,
    Shutter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Builtin {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    pub group: Group,
    pub tag: &'static str,
    pub blurb: &'static str,
    pub tip: &'static str,
    pub colour: Colour,
}

pub const BUILTINS: &[Builtin] = &[
    Builtin {
        id: "builtin:synth",
        name: "Flare",
        kind: Kind::Instrument,
        group: Group::Instruments,
        tag: "Polysynth",
        blurb: "Two oscillators, filter, 16 voices",
        tip: "A warm polysynth: two oscillators, a filter and up to 16 voices. \
              Five factory presets.",
        colour: Colour::Synth,
    },
    Builtin {
        id: "builtin:sampler",
        name: "Shutter",
        kind: Kind::Instrument,
        group: Group::Instruments,
        tag: "Sampler",
        blurb: "One sample, zones, or drum pads",
        tip: "A sampler: one sample across the keys, key and velocity zones, \
              or a pad per sound of the track's drum map.",
        colour: Colour::Shutter,
    },
    Builtin {
        id: "builtin:compressor",
        name: "Gravity",
        kind: Kind::Effect,
        group: Group::Dynamics,
        tag: "Compressor",
        blurb: "Pulls peaks down; sidechain-ready",
        tip: "A compressor that pulls peaks into a well. Sidechain from any track.",
        colour: Colour::Gravity,
    },
    Builtin {
        id: "builtin:limiter",
        name: "Horizon",
        kind: Kind::Effect,
        group: Group::Dynamics,
        tag: "Limiter",
        blurb: "A ceiling the peaks cannot pass",
        tip: "A limiter: the ceiling is the horizon. Look-ahead, with its latency reported.",
        colour: Colour::Horizon,
    },
    Builtin {
        id: "builtin:eq",
        name: "Iris",
        kind: Kind::Effect,
        group: Group::Eq,
        tag: "Parametric EQ",
        blurb: "Eight bands and a live analyzer",
        tip: "Eight bands (cuts, shelves, bells, notches) with a live analyzer.",
        colour: Colour::Eq,
    },
    Builtin {
        id: "builtin:delay",
        name: "Parallax",
        kind: Kind::Effect,
        group: Group::Time,
        tag: "Stereo delay",
        blurb: "Stereo echoes in notes or ms",
        tip: "A stereo delay in notes that follow the tempo, or in milliseconds. Ping-pong.",
        colour: Colour::Delay,
    },
    Builtin {
        id: "builtin:reverb",
        name: "Afterglow",
        kind: Kind::Effect,
        group: Group::Time,
        tag: "Reverb",
        blurb: "A tail that lingers and fades",
        tip: "A reverb: pre-delay, size, and a decay the tail really takes.",
        colour: Colour::Reverb,
    },
    Builtin {
        id: "builtin:utility",
        name: "Prism",
        kind: Kind::Effect,
        group: Group::Utility,
        tag: "Utility",
        blurb: "Gain, pan, width and polarity",
        tip: "Gain, pan, width, mono below a frequency, polarity, swap and a DC block.",
        colour: Colour::Utility,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub kind: Kind,
    pub source: Source,
    pub categories: Vec<String>,
    pub version: String,
    pub module: Option<PathBuf>,
    pub group: Option<Group>,
    pub tag: &'static str,
    pub blurb: &'static str,
    pub tip: &'static str,
    pub colour: Option<Colour>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked {
    pub name: String,
    pub path: PathBuf,
    pub reason: Reason,
    pub at: u64,
}

pub fn builtin_devices() -> Vec<Device> {
    BUILTINS
        .iter()
        .map(|builtin| Device {
            id: builtin.id.into(),
            name: builtin.name.into(),
            vendor: "Lumen".into(),
            kind: builtin.kind,
            source: Source::Builtin,
            categories: vec![builtin.group.name().into()],
            version: String::new(),
            module: None,
            group: Some(builtin.group),
            tag: builtin.tag,
            blurb: builtin.blurb,
            tip: builtin.tip,
            colour: Some(builtin.colour),
        })
        .collect()
}

pub fn builtin(id: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|builtin| builtin.id == id)
}

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

#[derive(Debug, Default, Clone, Copy)]
pub struct Query<'a> {
    pub text: &'a str,
    pub kind: Option<Kind>,
    pub only: Option<&'a HashSet<String>>,
    pub source: Option<Source>,
}

#[derive(Debug, Default, Clone)]
pub struct Catalog {
    devices: Vec<Device>,
    blocked: Vec<Blocked>,
}

impl Catalog {
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
                            group: None,
                            tag: kind.name(),
                            blurb: "",
                            tip: "",
                            colour: None,
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
            .filter(|d| query.source.is_none_or(|s| d.source == s))
            .filter(|d| query.only.is_none_or(|only| only.contains(&d.id)))
            .filter_map(|d| rank(d, &words).map(|score| (score, d)))
            .collect();
        hits.sort_by_key(|(score, _)| *score);
        hits.into_iter().map(|(_, d)| d).collect()
    }

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

fn rank(device: &Device, words: &[String]) -> Option<u32> {
    let name = device.name.to_lowercase();
    let vendor = device.vendor.to_lowercase();
    let categories = device.categories.join(" ").to_lowercase();
    let tag = device.tag.to_lowercase();
    let blurb = device.blurb.to_lowercase();
    let kind = match device.kind {
        Kind::Instrument => "instrument",
        Kind::Effect => "effect fx",
    };
    let starts = |text: &str, word: &str| {
        text.split(|c: char| !c.is_alphanumeric())
            .any(|w| w.starts_with(word))
    };
    let mut total = 0;
    for word in words {
        let score = if name.starts_with(word.as_str()) {
            0
        } else if starts(&name, word) {
            1
        } else if name.contains(word.as_str()) {
            2
        } else if tag.starts_with(word.as_str()) || starts(&tag, word) {
            3
        } else if vendor.contains(word.as_str())
            || categories.contains(word.as_str())
            || tag.contains(word.as_str())
        {
            4
        } else if blurb.contains(word.as_str()) {
            5
        } else if kind.contains(word.as_str()) {
            6
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
            [
                "Afterglow",
                "Bus Compressor",
                "Flare",
                "Gravity",
                "Horizon",
                "Iris",
                "Parallax",
                "Plate Reverb",
                "Prism",
                "Shutter",
                "Warm Synth"
            ]
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
    fn every_built_in_carries_its_kind_group_description_and_colour() {
        let builtins = builtin_devices();
        for d in &builtins {
            assert_eq!(d.source, Source::Builtin);
            assert_eq!(d.vendor, "Lumen");
            assert!(!d.tag.is_empty(), "{} has no kind tag", d.name);
            assert!(!d.blurb.is_empty(), "{} has no description", d.name);
            assert!(!d.tip.is_empty(), "{} has no tip", d.name);
            assert!(d.colour.is_some(), "{} has no colour", d.name);
            assert_eq!(d.categories, [d.group.unwrap().name()]);
        }
        assert_eq!(builtins.len(), 8);
        let found = |name: &str| builtins.iter().find(|d| d.name == name).unwrap();
        assert_eq!(found("Flare").kind, Kind::Instrument);
        assert_eq!(found("Shutter").kind, Kind::Instrument);
        assert_eq!(found("Shutter").group, Some(Group::Instruments));
        assert_eq!(found("Shutter").tag, "Sampler");
        assert_eq!(found("Flare").group, Some(Group::Instruments));
        assert_eq!(found("Gravity").tag, "Compressor");
        assert_eq!(found("Gravity").group, Some(Group::Dynamics));
        assert_eq!(found("Horizon").group, Some(Group::Dynamics));
        assert_eq!(found("Iris").tag, "Parametric EQ");
        assert_eq!(found("Iris").group, Some(Group::Eq));
        assert_eq!(found("Parallax").group, Some(Group::Time));
        assert_eq!(found("Afterglow").tag, "Reverb");
        assert_eq!(found("Prism").group, Some(Group::Utility));
        assert_eq!(
            Group::ALL.map(Group::name),
            ["Instruments", "Dynamics", "EQ", "Time", "Utility"]
        );
        assert_eq!(
            BUILTINS.iter().map(|b| b.id).collect::<Vec<_>>(),
            [
                "builtin:synth",
                "builtin:sampler",
                "builtin:compressor",
                "builtin:limiter",
                "builtin:eq",
                "builtin:delay",
                "builtin:reverb",
                "builtin:utility"
            ]
        );
        assert_eq!(builtin("builtin:eq").map(|b| b.name), Some("Iris"));
        assert!(builtin("builtin:nothing").is_none());
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
            [
                "Afterglow",
                "Bus Compressor",
                "Flare",
                "Gravity",
                "Horizon",
                "Iris",
                "Parallax",
                "Plate Reverb",
                "Prism",
                "Shutter",
                "Warm Synth"
            ]
        );
        assert_eq!(find("warm"), ["Warm Synth"]);
        assert_eq!(find("REVERB"), ["Plate Reverb", "Afterglow"]);
        assert_eq!(
            find("lumen"),
            [
                "Afterglow",
                "Flare",
                "Gravity",
                "Horizon",
                "Iris",
                "Parallax",
                "Prism",
                "Shutter"
            ]
        );
        assert_eq!(find("vendor compressor"), ["Bus Compressor"]);
        assert_eq!(find("dynamics"), ["Bus Compressor", "Gravity", "Horizon"]);
        assert_eq!(find("plate warm"), Vec::<&str>::new());
        assert_eq!(find("zzz"), Vec::<&str>::new());
    }

    #[test]
    fn a_built_in_is_found_by_its_kind_tag() {
        let catalog = Catalog::build(&Cache::new(), builtin_devices());
        let find = |text: &str| {
            names(catalog.search(&Query {
                text,
                ..Query::default()
            }))
        };
        assert_eq!(find("comp"), ["Gravity"]);
        assert_eq!(find("reverb"), ["Afterglow"]);
        assert_eq!(find("eq"), ["Iris"]);
        assert_eq!(find("limit"), ["Horizon"]);
        assert_eq!(find("delay"), ["Parallax"]);
        assert_eq!(find("poly"), ["Flare"]);
        assert_eq!(find("sidechain"), ["Gravity"]);
        assert_eq!(find("analyzer"), ["Iris"]);
        assert_eq!(find("comp reverb"), Vec::<&str>::new());
    }

    #[test]
    fn the_source_filter_shows_only_the_built_ins_or_only_the_plugins() {
        let catalog = sample();
        let source = |source: Source| {
            names(catalog.search(&Query {
                source: Some(source),
                ..Query::default()
            }))
        };
        assert_eq!(source(Source::Builtin).len(), 8);
        assert_eq!(
            source(Source::Vst3),
            ["Bus Compressor", "Plate Reverb", "Warm Synth"]
        );
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
        assert_eq!(instruments, ["Flare", "Shutter", "Warm Synth"]);
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
                ("Lumen".to_string(), 8),
                ("Vendor A".to_string(), 2),
                ("Vendor B".to_string(), 1)
            ]
        );
    }
}
