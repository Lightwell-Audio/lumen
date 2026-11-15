use std::collections::HashSet;

use lumen_plugin_catalog::catalog::{Catalog, Device, Group, Query, Source};

use super::view::{Chip, Selection, View};

pub(crate) const MAX_CHIPS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Row {
    Section {
        name: &'static str,
        count: usize,
    },
    Group {
        key: String,
        name: &'static str,
        count: usize,
        folded: bool,
    },
    Vendor {
        name: String,
        count: usize,
        folded: bool,
    },
    Device(usize),
    BlockedHeader {
        count: usize,
        folded: bool,
    },
    Blocked(usize),
    BlockedNote(usize),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Rows {
    pub list: Vec<Row>,
    pub chips: Vec<(String, usize)>,
    pub matching: usize,
    pub total: usize,
}

pub(crate) fn group_key(group: Group) -> String {
    format!("lumen:{}", group.key())
}

pub(crate) fn rows(
    catalog: &Catalog,
    view: &View,
    favorites: &HashSet<String>,
    recents: &[String],
) -> Rows {
    let words = view.text.trim();
    let query = Query {
        text: words,
        kind: view.kind.kind(),
        only: None,
        source: view.lumen_only.then_some(Source::Builtin),
    };
    let fitting: Vec<&Device> = catalog.search(&query);

    let mut counts: Vec<(String, usize)> = Vec::new();
    for device in &fitting {
        for category in &device.categories {
            match counts
                .iter_mut()
                .find(|(c, _)| c.eq_ignore_ascii_case(category))
            {
                Some((_, n)) => *n += 1,
                None => counts.push((category.clone(), 1)),
            }
        }
    }
    counts.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    counts.truncate(MAX_CHIPS);

    let shown: Vec<&Device> = match &view.chip {
        Chip::All => fitting,
        Chip::Favorites => fitting
            .into_iter()
            .filter(|d| favorites.contains(&d.id))
            .collect(),
        Chip::Recent => recents
            .iter()
            .filter_map(|id| fitting.iter().copied().find(|d| &d.id == id))
            .collect(),
        Chip::Category(wanted) => fitting
            .into_iter()
            .filter(|d| d.categories.iter().any(|c| c.eq_ignore_ascii_case(wanted)))
            .collect(),
    };

    let index = |device: &Device| {
        catalog
            .devices()
            .iter()
            .position(|d| d.id == device.id)
            .expect("a device from the catalog is in the catalog")
    };

    let mut list = Vec::new();
    if matches!(view.chip, Chip::Recent) {
        list.extend(shown.iter().map(|d| Row::Device(index(d))));
    } else {
        let (builtins, plugins): (Vec<&Device>, Vec<&Device>) =
            shown.iter().partition(|d| d.source == Source::Builtin);
        if !builtins.is_empty() {
            list.push(Row::Section {
                name: lumen_core::product_name!(),
                count: builtins.len(),
            });
            for group in Group::ALL {
                let mut members: Vec<&Device> = builtins
                    .iter()
                    .copied()
                    .filter(|d| d.group == Some(group))
                    .collect();
                if members.is_empty() {
                    continue;
                }
                members.sort_by_cached_key(|d| d.name.to_lowercase());
                let key = group_key(group);
                let folded = words.is_empty() && view.is_folded(&key);
                list.push(Row::Group {
                    key,
                    name: group.name(),
                    count: members.len(),
                    folded,
                });
                if !folded {
                    list.extend(members.iter().map(|d| Row::Device(index(d))));
                }
            }
        }
        if !plugins.is_empty() {
            list.push(Row::Section {
                name: "Plugins",
                count: plugins.len(),
            });
            let mut groups: Vec<(String, Vec<&Device>)> = Vec::new();
            for device in &plugins {
                match groups
                    .iter_mut()
                    .find(|(v, _)| v.eq_ignore_ascii_case(&device.vendor))
                {
                    Some((_, members)) => members.push(device),
                    None => groups.push((device.vendor.clone(), vec![device])),
                }
            }
            groups.sort_by_key(|a| a.0.to_lowercase());
            let spelled = catalog.vendors();
            for (vendor, members) in groups {
                let vendor = spelled
                    .iter()
                    .find(|(v, _)| v.eq_ignore_ascii_case(&vendor))
                    .map_or(vendor, |(v, _)| v.clone());
                let folded = words.is_empty() && view.is_folded(&vendor);
                list.push(Row::Vendor {
                    name: vendor,
                    count: members.len(),
                    folded,
                });
                if !folded {
                    list.extend(members.iter().map(|d| Row::Device(index(d))));
                }
            }
        }
    }

    if view.chip == Chip::All && view.kind == super::view::TypeFilter::All && !view.lumen_only {
        let needle = words.to_lowercase();
        let blocked: Vec<usize> = catalog
            .blocked()
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                needle.is_empty()
                    || needle
                        .split_whitespace()
                        .all(|w| b.name.to_lowercase().contains(w))
            })
            .map(|(i, _)| i)
            .collect();
        if !blocked.is_empty() {
            let folded = words.is_empty() && view.blocked_folded;
            list.push(Row::BlockedHeader {
                count: blocked.len(),
                folded,
            });
            if !folded {
                for i in blocked {
                    list.push(Row::Blocked(i));
                    if view.selected
                        == Some(Selection::Blocked(
                            catalog.blocked()[i].path.to_string_lossy().into_owned(),
                        ))
                    {
                        list.push(Row::BlockedNote(i));
                    }
                }
            }
        }
    }

    Rows {
        list,
        chips: counts,
        matching: shown.len(),
        total: catalog.devices().len(),
    }
}

pub(crate) fn none_found(catalog: &Catalog) -> bool {
    catalog.blocked().is_empty()
        && catalog
            .devices()
            .iter()
            .all(|d| d.source == Source::Builtin)
}

pub(crate) fn selection_of(catalog: &Catalog, row: &Row) -> Option<Selection> {
    match row {
        Row::Section { .. } => None,
        Row::Group { key, .. } => Some(Selection::Group(key.clone())),
        Row::Vendor { name, .. } => Some(Selection::Vendor(name.to_lowercase())),
        Row::Device(i) => Some(Selection::Device(catalog.devices()[*i].id.clone())),
        Row::BlockedHeader { .. } => Some(Selection::BlockedHeader),
        Row::Blocked(i) => Some(Selection::Blocked(
            catalog.blocked()[*i].path.to_string_lossy().into_owned(),
        )),
        Row::BlockedNote(_) => None,
    }
}

pub(crate) fn neighbour(
    catalog: &Catalog,
    list: &[Row],
    from: Option<&Selection>,
    delta: isize,
) -> Option<Selection> {
    let all: Vec<Selection> = list
        .iter()
        .filter_map(|r| selection_of(catalog, r))
        .collect();
    if all.is_empty() {
        return None;
    }
    let last = all.len() as isize - 1;
    let at = from.and_then(|s| all.iter().position(|a| a == s));
    let to = match at {
        Some(at) => (at as isize + delta).clamp(0, last),
        None if delta < 0 => last,
        None => 0,
    };
    Some(all[to as usize].clone())
}

#[cfg(test)]
mod tests {
    use lumen_plugin_catalog::cache::{Cache, ClassRecord, Entry, Reason, Result as Scanned};
    use lumen_plugin_catalog::catalog::{builtin_devices, class_id};
    use lumen_plugin_catalog::stamp::Stamp;

    use super::super::view::TypeFilter;
    use super::*;

    const AUDIO: &str = "Audio Module Class";

    fn class(id: u8, name: &str, sub: &str, vendor: &str) -> ClassRecord {
        ClassRecord {
            id: class_id(&[id; 16]),
            name: name.into(),
            category: AUDIO.into(),
            vendor: vendor.into(),
            subcategories: sub.into(),
            version: String::new(),
            sdk_version: String::new(),
        }
    }

    fn catalog() -> Catalog {
        let mut cache = Cache::new();
        cache.set(Entry {
            path: "/p/A.vst3".into(),
            binary: "/p/A.vst3".into(),
            stamp: Stamp {
                len: 1,
                modified_ns: 1,
            },
            result: Scanned::Scanned {
                vendor: "Zed".into(),
                url: String::new(),
                load_ms: 1,
                classes: vec![
                    class(1, "Warm Synth", "Instrument|Synth", "Zed"),
                    class(2, "Plate Reverb", "Fx|Reverb", "Zed"),
                    class(3, "Deep Bass", "Instrument|Bass", "Alpha"),
                    class(4, "Room Reverb", "Fx|Reverb", "Alpha"),
                    class(5, "Tape EQ", "Fx|EQ", "alpha"),
                ],
            },
        });
        cache.set(Entry {
            path: "/p/Bad.vst3".into(),
            binary: "/p/Bad.vst3".into(),
            stamp: Stamp {
                len: 1,
                modified_ns: 1,
            },
            result: Scanned::Blocked {
                reason: Reason::TimedOut { seconds: 30 },
                at: 1,
            },
        });
        Catalog::build(&cache, builtin_devices())
    }

    fn view() -> View {
        View::default()
    }

    fn names(c: &Catalog, rows: &Rows) -> Vec<String> {
        rows.list
            .iter()
            .map(|r| match r {
                Row::Section { name, count } => format!("={name} {count}"),
                Row::Group {
                    name,
                    count,
                    folded,
                    ..
                } => format!("[{name} {count}{}]", if *folded { " folded" } else { "" }),
                Row::Vendor {
                    name,
                    count,
                    folded,
                } => {
                    format!("[{name} {count}{}]", if *folded { " folded" } else { "" })
                }
                Row::Device(i) => c.devices()[*i].name.clone(),
                Row::BlockedHeader { count, folded } => {
                    format!("[blocked {count}{}]", if *folded { " folded" } else { "" })
                }
                Row::Blocked(i) => format!("!{}", c.blocked()[*i].name),
                Row::BlockedNote(i) => format!("note {}", c.blocked()[*i].name),
            })
            .collect()
    }

    #[test]
    fn built_ins_come_first_by_group_by_name_and_plugins_by_vendor_with_blocked_last() {
        let c = catalog();
        let r = rows(&c, &view(), &HashSet::new(), &[]);
        assert_eq!(
            names(&c, &r),
            [
                "=Lumen 8",
                "[Instruments 2]",
                "Flare",
                "Shutter",
                "[Dynamics 2]",
                "Gravity",
                "Horizon",
                "[EQ 1]",
                "Iris",
                "[Time 2]",
                "Afterglow",
                "Parallax",
                "[Utility 1]",
                "Prism",
                "=Plugins 5",
                "[Alpha 3]",
                "Deep Bass",
                "Room Reverb",
                "Tape EQ",
                "[Zed 2]",
                "Plate Reverb",
                "Warm Synth",
                "[blocked 1]",
                "!Bad",
            ]
        );
        assert_eq!((r.matching, r.total), (12, 12));
    }

    #[test]
    fn a_folded_vendor_hides_its_devices_but_a_search_opens_every_group() {
        let c = catalog();
        let mut v = view();
        v.toggle_fold("Alpha");
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert!(names(&c, &r).contains(&"[Alpha 3 folded]".to_string()));
        assert!(!names(&c, &r).contains(&"Deep Bass".to_string()));
        assert_eq!(r.matching, 13, "folded devices still match");

        v.text = "bass".into();
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(names(&c, &r), ["=Plugins 1", "[Alpha 1]", "Deep Bass"]);
    }

    #[test]
    fn a_folded_kind_group_keeps_its_header_and_a_search_opens_it() {
        let c = catalog();
        let mut v = view();
        let key = group_key(Group::Dynamics);
        v.toggle_fold(&key);
        let r = rows(&c, &v, &HashSet::new(), &[]);
        let list = names(&c, &r);
        assert!(list.contains(&"[Dynamics 2 folded]".to_string()));
        assert!(!list.contains(&"Gravity".to_string()));
        assert!(list.contains(&"[Instruments 2]".to_string()));

        v.text = "gravity".into();
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(names(&c, &r), ["=Lumen 1", "[Dynamics 1]", "Gravity"]);
    }

    #[test]
    fn the_lumen_filter_shows_only_the_built_ins_in_their_group_order() {
        let c = catalog();
        let mut v = view();
        v.lumen_only = true;
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(
            names(&c, &r),
            [
                "=Lumen 8",
                "[Instruments 2]",
                "Flare",
                "Shutter",
                "[Dynamics 2]",
                "Gravity",
                "Horizon",
                "[EQ 1]",
                "Iris",
                "[Time 2]",
                "Afterglow",
                "Parallax",
                "[Utility 1]",
                "Prism",
            ]
        );
        assert_eq!(r.matching, 7);
        assert!(
            !r.list.iter().any(|row| matches!(
                row,
                Row::Blocked(_) | Row::BlockedHeader { .. } | Row::Vendor { .. }
            )),
            "no plugins and no blocked modules"
        );
        assert_eq!(
            r.chips[0],
            ("Dynamics".to_string(), 2),
            "the chips count the built-ins that remain"
        );
    }

    #[test]
    fn the_lumen_filter_combines_with_the_type_the_category_and_the_words() {
        let c = catalog();
        let mut v = view();
        v.lumen_only = true;
        v.kind = TypeFilter::Effects;
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert!(!names(&c, &r).contains(&"Flare".to_string()));
        assert_eq!(r.matching, 6);

        v.kind = TypeFilter::All;
        v.chip = Chip::Category("dynamics".into());
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(
            names(&c, &r),
            ["=Lumen 2", "[Dynamics 2]", "Gravity", "Horizon"]
        );

        v.chip = Chip::All;
        v.text = "reverb".into();
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(names(&c, &r), ["=Lumen 1", "[Time 1]", "Afterglow"]);
    }

    #[test]
    fn a_search_for_a_kind_tag_finds_the_built_in_that_carries_it() {
        let c = catalog();
        let mut v = view();
        v.text = "comp".into();
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(names(&c, &r), ["=Lumen 1", "[Dynamics 1]", "Gravity"]);

        v.text = "eq".into();
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert!(names(&c, &r).contains(&"Iris".to_string()));

        v.text = "utility".into();
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert!(names(&c, &r).contains(&"Prism".to_string()));
    }

    #[test]
    fn the_type_filter_shows_one_kind_and_hides_the_blocked_section() {
        let c = catalog();
        let mut v = view();
        v.kind = TypeFilter::Instruments;
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(
            names(&c, &r),
            [
                "=Lumen 2",
                "[Instruments 2]",
                "Flare",
                "Shutter",
                "=Plugins 2",
                "[Alpha 1]",
                "Deep Bass",
                "[Zed 1]",
                "Warm Synth"
            ]
        );
        v.kind = TypeFilter::Effects;
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(r.matching, 9);
        assert!(!names(&c, &r).iter().any(|n| n.starts_with("[blocked")));
    }

    #[test]
    fn category_chips_count_what_the_words_and_type_allow_and_narrow_the_list() {
        let c = catalog();
        let mut v = view();
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(r.chips[0], ("Dynamics".to_string(), 2));
        assert!(r.chips.iter().any(|(n, k)| n == "Bass" && *k == 1));

        v.kind = TypeFilter::Instruments;
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert!(
            !r.chips.iter().any(|(n, _)| n == "Reverb"),
            "no instrument is a reverb"
        );

        v.kind = TypeFilter::All;
        v.chip = Chip::Category("reverb".into());
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(
            names(&c, &r),
            [
                "=Plugins 2",
                "[Alpha 1]",
                "Room Reverb",
                "[Zed 1]",
                "Plate Reverb"
            ]
        );
    }

    #[test]
    fn favorites_show_only_favorites_and_recent_is_one_list_most_recent_first() {
        let c = catalog();
        let id = |name: &str| {
            c.devices()
                .iter()
                .find(|d| d.name == name)
                .unwrap()
                .id
                .clone()
        };
        let favorites: HashSet<String> = [id("Tape EQ")].into();
        let mut v = view();
        v.chip = Chip::Favorites;
        let r = rows(&c, &v, &favorites, &[]);
        assert_eq!(names(&c, &r), ["=Plugins 1", "[Alpha 1]", "Tape EQ"]);

        v.chip = Chip::Recent;
        let recents = vec![id("Warm Synth"), id("Deep Bass"), "vst3:GONE".to_string()];
        let r = rows(&c, &v, &favorites, &recents);
        assert_eq!(
            names(&c, &r),
            ["Warm Synth", "Deep Bass"],
            "no vendor rows, and a missing device is skipped"
        );
    }

    #[test]
    fn favorites_and_recent_mix_built_ins_with_plugins() {
        let c = catalog();
        let id = |name: &str| {
            c.devices()
                .iter()
                .find(|d| d.name == name)
                .unwrap()
                .id
                .clone()
        };
        let favorites: HashSet<String> = [id("Gravity"), id("Tape EQ")].into();
        let mut v = view();
        v.chip = Chip::Favorites;
        let r = rows(&c, &v, &favorites, &[]);
        assert_eq!(
            names(&c, &r),
            [
                "=Lumen 1",
                "[Dynamics 1]",
                "Gravity",
                "=Plugins 1",
                "[Alpha 1]",
                "Tape EQ"
            ]
        );

        v.chip = Chip::Recent;
        let recents = vec![id("Prism"), id("Warm Synth")];
        let r = rows(&c, &v, &favorites, &recents);
        assert_eq!(names(&c, &r), ["Prism", "Warm Synth"]);
    }

    #[test]
    fn blocked_modules_are_found_by_name_and_the_selected_one_shows_its_note() {
        let c = catalog();
        let mut v = view();
        v.text = "bad".into();
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(names(&c, &r), ["[blocked 1]", "!Bad"]);

        v.text.clear();
        v.selected = Some(Selection::Blocked("/p/Bad.vst3".into()));
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(
            &names(&c, &r)[names(&c, &r).len() - 3..],
            ["[blocked 1]", "!Bad", "note Bad"]
        );

        v.selected = None;
        v.blocked_folded = true;
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(
            &names(&c, &r)[names(&c, &r).len() - 1..],
            ["[blocked 1 folded]"]
        );
    }

    #[test]
    fn nothing_matching_is_an_empty_list_with_the_totals_intact() {
        let c = catalog();
        let mut v = view();
        v.text = "zzzz".into();
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert!(r.list.is_empty());
        assert_eq!((r.matching, r.total), (0, 13));
    }

    #[test]
    fn at_most_twelve_chips_are_offered() {
        let mut cache = Cache::new();
        let classes: Vec<ClassRecord> = (0..20)
            .map(|i| class(i, &format!("Fx {i}"), &format!("Fx|Cat{i:02}"), "V"))
            .collect();
        cache.set(Entry {
            path: "/p/Many.vst3".into(),
            binary: "/p/Many.vst3".into(),
            stamp: Stamp {
                len: 1,
                modified_ns: 1,
            },
            result: Scanned::Scanned {
                vendor: "V".into(),
                url: String::new(),
                load_ms: 1,
                classes,
            },
        });
        let c = Catalog::build(&cache, vec![]);
        assert_eq!(
            rows(&c, &view(), &HashSet::new(), &[]).chips.len(),
            MAX_CHIPS
        );
    }

    #[test]
    fn the_selection_steps_through_selectable_rows_and_stops_at_the_ends() {
        let c = catalog();
        let list = rows(&c, &view(), &HashSet::new(), &[]).list;
        let first = neighbour(&c, &list, None, 1).unwrap();
        assert_eq!(first, Selection::Group(group_key(Group::Instruments)));
        let second = neighbour(&c, &list, Some(&first), 1).unwrap();
        assert!(matches!(&second, Selection::Device(id) if id == "builtin:synth"));
        assert_eq!(
            neighbour(&c, &list, Some(&first), -1),
            Some(first.clone()),
            "no step before the first"
        );
        let last = neighbour(&c, &list, None, -1).unwrap();
        assert_eq!(last, Selection::Blocked("/p/Bad.vst3".into()));
        assert_eq!(
            neighbour(&c, &list, Some(&last), 1),
            Some(last),
            "no step past the last"
        );
        assert_eq!(neighbour(&c, &[], None, 1), None);
    }

    #[test]
    fn nothing_of_the_persons_own_is_what_the_no_plugins_yet_card_asks_about() {
        let c = catalog();
        assert!(
            !none_found(&c),
            "one module is blocked and two vendors were scanned"
        );
        let only_own = Catalog::build(&Cache::new(), builtin_devices());
        assert!(none_found(&only_own));
    }

    #[test]
    fn a_selection_that_is_no_longer_listed_steps_from_the_start() {
        let c = catalog();
        let list = rows(&c, &view(), &HashSet::new(), &[]).list;
        let gone = Selection::Device("vst3:GONE".into());
        assert_eq!(
            neighbour(&c, &list, Some(&gone), 1),
            Some(Selection::Group(group_key(Group::Instruments)))
        );
    }
}
