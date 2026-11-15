use lumen_hosting::{ParamInfo, UnitInfo};

pub(crate) fn automatable(list: &[ParamInfo]) -> Vec<usize> {
    let movable = |p: &ParamInfo| !p.hidden() && !p.read_only() && !p.program_change();
    let marked: Vec<usize> = list
        .iter()
        .enumerate()
        .filter(|(_, p)| movable(p) && p.can_automate())
        .map(|(i, _)| i)
        .collect();
    if !marked.is_empty() {
        return marked;
    }
    list.iter()
        .enumerate()
        .filter(|(_, p)| movable(p))
        .map(|(i, _)| i)
        .collect()
}

pub(crate) fn group_name(units: &[UnitInfo], id: i32) -> String {
    crate::chain::unit_name(units, id)
}

pub(crate) fn matching(
    list: &[ParamInfo],
    units: &[UnitInfo],
    base: &[usize],
    search: &str,
    group: Option<i32>,
) -> Vec<usize> {
    let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
    base.iter()
        .copied()
        .filter(|&i| group.is_none_or(|g| list[i].unit == g))
        .filter(|&i| {
            if words.is_empty() {
                return true;
            }
            let p = &list[i];
            let hay = format!("{} {}", p.title, group_name(units, p.unit)).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .collect()
}

pub(crate) fn groups(
    list: &[ParamInfo],
    units: &[UnitInfo],
    base: &[usize],
) -> Vec<(i32, String, usize)> {
    let mut found: Vec<(i32, usize)> = Vec::new();
    for &i in base {
        let unit = list[i].unit;
        match found.iter_mut().find(|(u, _)| *u == unit) {
            Some((_, n)) => *n += 1,
            None => found.push((unit, 1)),
        }
    }
    if found.len() < 2 {
        return Vec::new();
    }
    found
        .into_iter()
        .map(|(u, n)| (u, group_name(units, u), n))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_hosting::{FLAG_CAN_AUTOMATE, FLAG_HIDDEN, FLAG_PROGRAM_CHANGE, FLAG_READ_ONLY};

    fn param(id: u32, title: &str, unit: i32, flags: u32) -> ParamInfo {
        ParamInfo {
            id,
            title: title.to_owned(),
            units: String::new(),
            steps: 0,
            default: 0.0,
            flags,
            unit,
            value: 0.0,
        }
    }

    fn unit(id: i32, name: &str) -> UnitInfo {
        UnitInfo {
            id,
            parent: 0,
            name: name.to_owned(),
        }
    }

    #[test]
    fn only_what_a_lane_can_drive_is_offered() {
        let list = [
            param(1, "Cutoff", 0, FLAG_CAN_AUTOMATE),
            param(2, "Hidden", 0, FLAG_CAN_AUTOMATE | FLAG_HIDDEN),
            param(3, "Meter", 0, FLAG_CAN_AUTOMATE | FLAG_READ_ONLY),
            param(4, "Program", 0, FLAG_CAN_AUTOMATE | FLAG_PROGRAM_CHANGE),
            param(5, "Plain", 0, 0),
            param(6, "Resonance", 0, FLAG_CAN_AUTOMATE),
        ];
        assert_eq!(automatable(&list), [0, 5]);
    }

    #[test]
    fn a_plugin_that_marks_nothing_automatable_offers_what_is_left() {
        let list = [
            param(1, "A", 0, 0),
            param(2, "B", 0, FLAG_HIDDEN),
            param(3, "C", 0, FLAG_READ_ONLY),
            param(4, "D", 0, 0),
        ];
        assert_eq!(automatable(&list), [0, 3]);
    }

    #[test]
    fn a_search_needs_every_word_in_the_name_or_the_group() {
        let list = [
            param(1, "Cutoff", 1, 1),
            param(2, "Resonance", 1, 1),
            param(3, "Cutoff", 2, 1),
        ];
        let units = [unit(1, "Filter"), unit(2, "Delay")];
        let base = automatable(&list);
        assert_eq!(matching(&list, &units, &base, "", None), [0, 1, 2]);
        assert_eq!(matching(&list, &units, &base, "cut", None), [0, 2]);
        assert_eq!(matching(&list, &units, &base, "FILTER cut", None), [0]);
        assert_eq!(
            matching(&list, &units, &base, "nothing", None),
            [] as [usize; 0]
        );
        assert_eq!(matching(&list, &units, &base, "", Some(2)), [2]);
    }

    #[test]
    fn groups_are_offered_only_when_there_are_several() {
        let list = [
            param(1, "A", 1, 1),
            param(2, "B", 1, 1),
            param(3, "C", 2, 1),
        ];
        let units = [unit(1, "Filter"), unit(2, "Delay")];
        let base = automatable(&list);
        assert_eq!(
            groups(&list, &units, &base),
            [(1, "Filter".to_owned(), 2), (2, "Delay".to_owned(), 1)]
        );
        assert!(groups(&list[..2], &units, &[0, 1]).is_empty());
    }

    #[test]
    fn two_thousand_parameters_filter_at_once() {
        let list: Vec<ParamInfo> = (0..2_000)
            .map(|k| param(k, &format!("Param {k}"), (k % 8) as i32, FLAG_CAN_AUTOMATE))
            .collect();
        let base = automatable(&list);
        assert_eq!(base.len(), 2_000);
        let started = std::time::Instant::now();
        let mut found = 0;
        for query in ["", "1", "param 19", "param 1999", "zzz"] {
            for _ in 0..20 {
                found += matching(&list, &[], &base, query, None).len();
            }
        }
        assert!(found > 0);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(1_000),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(matching(&list, &[], &base, "param 1999", None), [1_999]);
    }
}
