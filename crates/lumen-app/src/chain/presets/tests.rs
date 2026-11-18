use super::menu::filter;
use super::state::{Choice, Current, Source};
use super::*;

fn program(index: usize, name: &str) -> Choice {
    Choice::Program {
        list: 0,
        index,
        name: name.to_owned(),
        group: None,
    }
}

fn file(name: &str, user: bool, group: Option<&str>) -> Choice {
    Choice::File {
        path: PathBuf::from(format!("/p/{name}.vstpreset")),
        user,
        name: name.to_owned(),
        group: group.map(str::to_owned),
    }
}

#[test]
fn a_search_needs_every_word_in_the_name_or_the_group() {
    let all = [
        program(0, "Warm Pad"),
        program(1, "Hard Lead"),
        file("Warm Bass", true, Some("Bass")),
    ];
    assert_eq!(filter(&all, ""), [0, 1, 2]);
    assert_eq!(filter(&all, "warm"), [0, 2]);
    assert_eq!(filter(&all, "warm bass"), [2]);
    assert_eq!(filter(&all, "BASS warm"), [2]);
    assert!(filter(&all, "nothing").is_empty());
}

#[test]
fn headings_say_where_a_sound_came_from() {
    assert_eq!(program(0, "x").heading(), "Factory");
    assert_eq!(
        Choice::Factory {
            preset: lumen_devices::Preset::Pad
        }
        .heading(),
        "Factory"
    );
    assert_eq!(file("x", true, None).heading(), "Your presets");
    assert_eq!(
        file("x", false, Some("Bass")).heading(),
        "System presets \u{b7} Bass"
    );
}

#[test]
fn a_factory_preset_is_named_and_found_ahead_of_your_presets() {
    let all = [
        Choice::Factory {
            preset: lumen_devices::Preset::Bass,
        },
        file("Mine", true, None),
    ];
    assert_eq!(all[0].name(), "Bass");
    assert_eq!(all[1].name(), "Mine");
    assert_eq!(filter(&all, "bass"), [0]);
    assert_eq!(filter(&all, ""), [0, 1]);
}

#[test]
fn the_current_factory_preset_is_found_by_its_id_not_its_name() {
    let current = Current {
        name: "Bass".to_owned(),
        source: Source::Factory { id: "bass" },
        baseline: None,
        rev: 0,
    };
    assert!(
        Choice::Factory {
            preset: lumen_devices::Preset::Bass
        }
        .is_current(&current)
    );
    assert!(
        !Choice::Factory {
            preset: lumen_devices::Preset::Pad
        }
        .is_current(&current)
    );
    assert!(!file("Bass", true, None).is_current(&current));
    assert!(!program(0, "Bass").is_current(&current));
}

#[test]
fn the_current_sound_is_found_by_its_place_not_its_name() {
    let current = Current {
        name: "Warm Pad".to_owned(),
        source: Source::Program { list: 0, index: 1 },
        baseline: None,
        rev: 0,
    };
    assert!(!program(0, "Warm Pad").is_current(&current));
    assert!(program(1, "Other name").is_current(&current));
    assert!(!file("Warm Pad", true, None).is_current(&current));
}
