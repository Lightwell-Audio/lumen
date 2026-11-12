//! The command palette (M3 slice 7b; the owner's P2 with P3's values).
//! Ctrl+K opens it over the arrangement. Typing finds commands by fuzzy
//! search; each result names what it will act on ("Duplicate 2 clips"),
//! and a command that cannot run now stays listed, dimmed, with the reason.
//! Typed values become actions: a number offers that tempo or that bar,
//! and a track's or locator's name goes to it. A strip under the list
//! explains the chosen entry. Shortcuts show as K5 keycaps.
//!
//! By concern (audit M10): what it searches and how it scores (`search`),
//! what a command will act on (`labels`), the rows for a query and running
//! one (`rows`), the window and its search field (`window`), and the rows,
//! their keycaps and the strip (`list`). The palette's state is here.

mod labels;
mod list;
mod rows;
mod search;
mod window;

use lumen_core::time::Ticks;
use lumen_project::model::Id;

use crate::commands::{ALL, Command};

/// The palette's width, a row's height and how many rows show.
const WIDTH: f32 = 520.0;
const ROW: f32 = 30.0;
const ROWS: f32 = 10.0;
/// How far below the top of the window it opens.
const TOP: f32 = 48.0;
/// The field, the strip and the margins: what the palette needs besides
/// its rows.
const CHROME: f32 = 150.0;
/// Commands remembered as recent.
const RECENT: usize = 5;

/// The palette between frames (not saved).
#[derive(Debug, Default)]
pub(crate) struct Palette {
    open: bool,
    query: String,
    /// The chosen row.
    chosen: usize,
    /// Commands run from the palette, the latest first.
    recent: Vec<Command>,
    /// Why the command just picked could not run.
    notice: Option<&'static str>,
    /// Focus the field when it next shows.
    focus: bool,
    /// The room the strip keeps for what a row does, so the palette keeps
    /// its height as the chosen row changes.
    strip_room: Option<StripRoom>,
}

/// The tallest of every row's `about` text, laid out at the strip's width
/// and pixel density (a test keeps every `about` to 320 characters,
/// `commands::tests`). Reasons, notices and the rebind lines are not
/// measured: they are a line or two, and one taller than the room grows
/// the strip for that row rather than being cut.
#[derive(Debug, Clone, Copy)]
struct StripRoom {
    width: f32,
    pixels_per_point: f32,
    height: f32,
}

impl Entry {
    /// What the row does, said in the strip under the list.
    fn about(&self) -> &'static str {
        match self {
            Entry::Command(command) => command.info().about,
            Entry::Tempo(_) => "Sets the song's tempo.",
            Entry::Bar(_) => "Moves the playhead to the start of that bar.",
            Entry::Track(_) => "Selects the track and scrolls to it.",
            Entry::Locator(_) => "Moves the playhead to the locator.",
            Entry::Template(_) => {
                "Starts a new, unsaved project from the template: its tracks, devices, audio and settings."
            }
        }
    }

    /// Every text [`Entry::about`] can give.
    fn every_about() -> impl Iterator<Item = &'static str> {
        ALL.iter().map(|info| info.about).chain([
            Entry::Tempo(120.0).about(),
            Entry::Bar(1).about(),
            Entry::Track(Id(0)).about(),
            Entry::Locator(Ticks::ZERO).about(),
            Entry::Template(0).about(),
        ])
    }
}

impl Palette {
    /// A closed palette remembering the saved recent commands (ids).
    pub(crate) fn with_recent(ids: &[String]) -> Self {
        Self {
            recent: ids
                .iter()
                .filter_map(|id| crate::commands::by_id(id))
                .collect(),
            ..Self::default()
        }
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// The recent commands' ids, to save.
    fn recent_ids(&self) -> Vec<String> {
        self.recent.iter().map(|c| c.info().id.to_owned()).collect()
    }
}

/// What a row does.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Entry {
    Command(Command),
    /// Set the tempo to this many BPM.
    Tempo(f64),
    /// Go to this bar (counted from 1, as shown).
    Bar(i64),
    /// Select this track and show it.
    Track(Id),
    /// Move the playhead to this locator.
    Locator(Ticks),
    /// Start a new project from this template (its place in the list found).
    Template(usize),
}

/// A row of the palette.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub entry: Entry,
    /// What the row says ("Duplicate 2 clips", "Set tempo to 128 BPM").
    pub label: String,
    /// The characters of `label` the search matched, for highlighting.
    matched: Vec<usize>,
    /// Whether it can run now, and why not.
    pub ready: Result<(), &'static str>,
    score: i32,
}
