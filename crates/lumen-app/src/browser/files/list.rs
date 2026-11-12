//! Drawing Samples, Projects and Places (M9 R7c): the list's items, their
//! rows, their keys and menus, and the designed states. What the app does
//! with what was done is `app::browser::files::list` (audit M12).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::Key;

use super::walk::{Entry, Kind};
use super::{Listing, Place, saved};
use crate::browser::Tab;

/// A row of one line, and of two (a search result, with its folder).
pub(crate) const ROW: f32 = 28.0;
pub(crate) const ROW_TWO: f32 = 42.0;
/// A section's heading row (Projects' Recent).
pub(crate) const HEADING: f32 = 24.0;
/// Below this width the pictures give way to the names.
pub(crate) const PICTURES_FROM: f32 = 200.0;
/// Rows painted around the view, for the pictures asked ahead.
pub(crate) const AHEAD: usize = 8;

/// Something dragged from the file tabs onto the arrangement.
#[derive(Debug, Clone)]
pub(crate) struct Dragged(pub Vec<PathBuf>);

/// A row of the list, or a heading.
#[derive(Debug, Clone)]
pub(crate) enum Item {
    Heading(&'static str),
    Row(Arc<Entry>),
    /// A place on Places' top level: its name, whether it is there.
    Place(Arc<Entry>, bool),
}

impl Item {
    pub(crate) fn entry(&self) -> Option<&Entry> {
        match self {
            Item::Heading(_) => None,
            Item::Row(e) | Item::Place(e, _) => Some(e),
        }
    }
}

/// What a list of items was made from: the same key, the same list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemsKey {
    pub(crate) tab: Tab,
    pub(crate) generation: u64,
    pub(crate) words: String,
    pub(crate) narrow: Option<PathBuf>,
    pub(crate) recent: Vec<String>,
}

/// The keys the list answers; on a frame without one, it does nothing.
pub(crate) const LIST_KEYS: [Key; 10] = [
    Key::ArrowDown,
    Key::ArrowUp,
    Key::Home,
    Key::End,
    Key::Enter,
    Key::ArrowRight,
    Key::ArrowLeft,
    Key::Backspace,
    Key::Escape,
    Key::F10,
];

/// What was done in the list this frame.
#[derive(Default)]
pub(crate) struct Out {
    pub select: Option<PathBuf>,
    pub open: Option<Entry>,
    pub preview: Option<PathBuf>,
    pub stop: bool,
    pub reveal: Option<PathBuf>,
    pub add_place: Option<PathBuf>,
    pub remove_place: Option<PathBuf>,
    pub find_place: Option<PathBuf>,
}

/// The items `key` makes of `listing`'s rows.
pub(crate) fn made_items(listing: &mut Listing, key: &ItemsKey) -> Vec<Item> {
    let words = &key.words;
    if key.tab != Tab::Projects {
        let shown = listing.shown(words, key.narrow.as_deref()).to_vec();
        return shown
            .into_iter()
            .map(|i| Item::Row(Arc::clone(&listing.rows[i])))
            .collect();
    }
    let lower = words.to_lowercase();
    let recent: Vec<Arc<Entry>> = key
        .recent
        .iter()
        .map(|p| Entry::new(PathBuf::from(p), Kind::Project))
        .filter(|e| lower.split_whitespace().all(|w| e.key().contains(w)))
        .map(Arc::new)
        .collect();
    let picked = listing.shown(words, None).to_vec();
    let shown: Vec<Arc<Entry>> = picked
        .into_iter()
        .map(|i| Arc::clone(&listing.rows[i]))
        .filter(|e| !recent.iter().any(|r| r.path == e.path))
        .collect();
    let mut items = Vec::new();
    if !recent.is_empty() {
        items.push(Item::Heading("Recent"));
        items.extend(recent.into_iter().map(Item::Row));
    }
    if !shown.is_empty() {
        items.push(Item::Heading("In the projects folder"));
        items.extend(shown.into_iter().map(Item::Row));
    }
    items
}

/// Places' top level: the person's places, then Computer.
pub(crate) fn top_places(places: &[Place], present: &HashMap<PathBuf, bool>) -> Vec<Item> {
    let mut items: Vec<Item> = places
        .iter()
        .map(|p| {
            let path = PathBuf::from(&p.path);
            let there = present.get(&path).copied().unwrap_or(true);
            Item::Place(
                Arc::new(Entry::named(path, p.name.clone(), Kind::Folder)),
                there,
            )
        })
        .collect();
    items.push(Item::Row(Arc::new(Entry::named(
        PathBuf::from(saved::COMPUTER),
        "Computer".to_owned(),
        Kind::Folder,
    ))));
    items
}

/// Whether rows carry their folder on a second line: search results, and
/// projects.
pub(crate) fn two_lines(tab: Tab, words: &str) -> bool {
    tab == Tab::Projects || !words.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|i| match i {
                Item::Heading(text) => format!("[{text}]"),
                Item::Row(e) | Item::Place(e, _) => e.name.clone(),
            })
            .collect()
    }

    #[test]
    fn places_come_first_and_computer_last_and_a_missing_one_is_marked() {
        let places = [
            Place {
                name: "Samples".to_owned(),
                path: "/x/Samples".to_owned(),
            },
            Place {
                name: "Gone".to_owned(),
                path: "/x/Gone".to_owned(),
            },
        ];
        let present = HashMap::from([(PathBuf::from("/x/Gone"), false)]);
        let items = top_places(&places, &present);
        assert_eq!(words(&items), ["Samples", "Gone", "Computer"]);
        assert!(matches!(items[0], Item::Place(_, true)));
        assert!(matches!(items[1], Item::Place(_, false)));
    }

    #[test]
    fn search_results_and_projects_carry_their_folder_on_a_second_line() {
        assert!(two_lines(Tab::Projects, ""));
        assert!(two_lines(Tab::Samples, "kick"));
        assert!(!two_lines(Tab::Samples, "  "));
    }

    #[test]
    fn projects_are_a_recent_section_then_the_folder_minus_the_recent() {
        let mut listing = Listing::default();
        listing.rows.push(Arc::new(Entry::new(
            PathBuf::from("/p/Old.lumen"),
            Kind::Project,
        )));
        listing.rows.push(Arc::new(Entry::new(
            PathBuf::from("/p/New.lumen"),
            Kind::Project,
        )));
        let key = ItemsKey {
            tab: Tab::Projects,
            generation: listing.generation,
            words: String::new(),
            narrow: None,
            recent: vec!["/p/New.lumen".to_owned()],
        };
        let items = made_items(&mut listing, &key);
        assert_eq!(
            words(&items),
            ["[Recent]", "New", "[In the projects folder]", "Old"]
        );
    }
}
