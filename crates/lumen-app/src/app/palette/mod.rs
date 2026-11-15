mod labels;
mod list;
mod rows;
mod search;
mod window;

use lumen_core::time::Ticks;
use lumen_project::model::Id;

use crate::commands::{ALL, Command};

const WIDTH: f32 = 520.0;
const ROW: f32 = 30.0;
const ROWS: f32 = 10.0;
const TOP: f32 = 48.0;
const CHROME: f32 = 150.0;
const RECENT: usize = 5;

#[derive(Debug, Default)]
pub(crate) struct Palette {
    open: bool,
    query: String,
    chosen: usize,
    recent: Vec<Command>,
    notice: Option<&'static str>,
    focus: bool,
    strip_room: Option<StripRoom>,
}

#[derive(Debug, Clone, Copy)]
struct StripRoom {
    width: f32,
    pixels_per_point: f32,
    height: f32,
}

impl Entry {
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

    fn recent_ids(&self) -> Vec<String> {
        self.recent.iter().map(|c| c.info().id.to_owned()).collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Entry {
    Command(Command),
    Tempo(f64),
    Bar(i64),
    Track(Id),
    Locator(Ticks),
    Template(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub entry: Entry,
    pub label: String,
    matched: Vec<usize>,
    pub ready: Result<(), &'static str>,
    score: i32,
}
