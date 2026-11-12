//! The preset menu's tests.

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
    assert_eq!(file("x", true, None).heading(), "Your presets");
    assert_eq!(
        file("x", false, Some("Bass")).heading(),
        "System presets \u{b7} Bass"
    );
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
