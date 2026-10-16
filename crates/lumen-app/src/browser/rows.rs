//! The browser's list as flat rows: what the catalog, the filters and the
//! fold state make of it. Pure, so the whole behavior is tested without a
//! window; `list.rs` paints and answers the pointer.

use std::collections::HashSet;

use lumen_plugin_catalog::catalog::{Catalog, Device, Query, Source};

use super::view::{Chip, Selection, View};

/// The most category chips offered.
pub(crate) const MAX_CHIPS: usize = 12;

/// One row. Indexes are into the catalog's `devices()` and `blocked()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Row {
    Vendor {
        name: String,
        /// Devices of this vendor that match.
        count: usize,
        folded: bool,
    },
    Device(usize),
    BlockedHeader {
        count: usize,
        folded: bool,
    },
    Blocked(usize),
    /// The reason and the actions, under the selected blocked module.
    BlockedNote(usize),
}

/// The rows, and what the controls above them need to know.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Rows {
    pub list: Vec<Row>,
    /// Category chips: name and how many devices match the words and type,
    /// most first.
    pub chips: Vec<(String, usize)>,
    /// Devices that match everything (words, type, chip), folded or not.
    pub matching: usize,
    /// Devices in the catalog.
    pub total: usize,
}

/// The rows for `view` over `catalog`.
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
    };
    // Everything that fits the words and the type: the chips count these.
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
        // Most recent first: grouping by vendor would lose the order.
        list.extend(shown.iter().map(|d| Row::Device(index(d))));
    } else {
        // Groups by vendor, Lumen's own first, then by name.
        let mut groups: Vec<(String, bool, Vec<&Device>)> = Vec::new();
        for device in &shown {
            let builtin = device.source == Source::Builtin;
            match groups
                .iter_mut()
                .find(|(v, _, _)| v.eq_ignore_ascii_case(&device.vendor))
            {
                Some((_, has_builtin, members)) => {
                    *has_builtin |= builtin;
                    members.push(device);
                }
                None => groups.push((device.vendor.clone(), builtin, vec![device])),
            }
        }
        groups.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });
        // One spelling per vendor, from the whole catalog, so a header does
        // not change with what the filters leave in it.
        let spelled = catalog.vendors();
        for (vendor, _, members) in groups {
            let vendor = spelled
                .iter()
                .find(|(v, _)| v.eq_ignore_ascii_case(&vendor))
                .map_or(vendor, |(v, _)| v.clone());
            // Searching opens every group: a hit must not hide in a fold.
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

    // Blocked modules sit at the end, only when nothing but words narrows
    // the list: a chip or a type is about devices.
    if view.chip == Chip::All && view.kind == super::view::TypeFilter::All {
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

/// What selecting `row` selects; `None` for a row that is not selectable (a
/// blocked module's note).
pub(crate) fn selection_of(catalog: &Catalog, row: &Row) -> Option<Selection> {
    match row {
        Row::Vendor { name, .. } => Some(Selection::Vendor(name.to_lowercase())),
        Row::Device(i) => Some(Selection::Device(catalog.devices()[*i].id.clone())),
        Row::BlockedHeader { .. } => Some(Selection::BlockedHeader),
        Row::Blocked(i) => Some(Selection::Blocked(
            catalog.blocked()[*i].path.to_string_lossy().into_owned(),
        )),
        Row::BlockedNote(_) => None,
    }
}

/// Where the selection goes `delta` selectable rows on (negative: back),
/// stopping at either end. From nothing, down goes to the first row and up
/// to the last. `None` when there is nothing to select.
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
    fn devices_group_by_vendor_with_lumen_first_then_by_name_and_blocked_last() {
        let c = catalog();
        let r = rows(&c, &view(), &HashSet::new(), &[]);
        assert_eq!(
            names(&c, &r),
            [
                "[Lumen 1]",
                "Synth",
                // "alpha" and "Alpha" are one vendor.
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
        assert_eq!((r.matching, r.total), (6, 6));
    }

    #[test]
    fn a_folded_vendor_hides_its_devices_but_a_search_opens_every_group() {
        let c = catalog();
        let mut v = view();
        v.toggle_fold("Alpha");
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert!(names(&c, &r).contains(&"[Alpha 3 folded]".to_string()));
        assert!(!names(&c, &r).contains(&"Deep Bass".to_string()));
        assert_eq!(r.matching, 6, "folded devices still match");

        v.text = "bass".into();
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(names(&c, &r), ["[Alpha 1]", "Deep Bass"]);
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
                "[Lumen 1]",
                "Synth",
                "[Alpha 1]",
                "Deep Bass",
                "[Zed 1]",
                "Warm Synth"
            ]
        );
        v.kind = TypeFilter::Effects;
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(r.matching, 3);
        assert!(!names(&c, &r).iter().any(|n| n.starts_with("[blocked")));
    }

    #[test]
    fn category_chips_count_what_the_words_and_type_allow_and_narrow_the_list() {
        let c = catalog();
        let mut v = view();
        let r = rows(&c, &v, &HashSet::new(), &[]);
        assert_eq!(r.chips[0], ("Reverb".to_string(), 2));
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
            ["[Alpha 1]", "Room Reverb", "[Zed 1]", "Plate Reverb"]
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
        assert_eq!(names(&c, &r), ["[Alpha 1]", "Tape EQ"]);

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
        assert_eq!((r.matching, r.total), (0, 6));
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
        assert_eq!(first, Selection::Vendor("lumen".into()));
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
    fn a_selection_that_is_no_longer_listed_steps_from_the_start() {
        let c = catalog();
        let list = rows(&c, &view(), &HashSet::new(), &[]).list;
        let gone = Selection::Device("vst3:GONE".into());
        assert_eq!(
            neighbour(&c, &list, Some(&gone), 1),
            Some(Selection::Vendor("lumen".into()))
        );
    }
}
