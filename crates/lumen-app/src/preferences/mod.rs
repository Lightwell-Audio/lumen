//! Preferences (M9 R7, the owner's PW A): one window of pages, Audio to
//! Appearance, with a list of the pages at the left and a search above it
//! that finds any row by a word. The window between frames lives here, with
//! the rows search knows and the outline that marks the row a search led
//! to; the app draws the window (`app/preferences/window.rs`) and the pages
//! that change its own settings (`app/preferences/pages.rs`).

use std::cell::Cell;

use eframe::egui::{self, Ui};

use crate::{motion, theme};

pub mod audio;
pub mod midi;

/// A page of Preferences, in the order the list shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Audio,
    Midi,
    Recording,
    Editing,
    Mixing,
    Plugins,
    Files,
    Appearance,
}

impl Page {
    pub const ALL: [Page; 8] = [
        Page::Audio,
        Page::Midi,
        Page::Recording,
        Page::Editing,
        Page::Mixing,
        Page::Plugins,
        Page::Files,
        Page::Appearance,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Page::Audio => "Audio",
            Page::Midi => "MIDI",
            Page::Recording => "Recording",
            Page::Editing => "Editing",
            Page::Mixing => "Mixing",
            Page::Plugins => "Plugins",
            Page::Files => "Files",
            Page::Appearance => "Appearance",
        }
    }

    /// What the page holds, said on hover over its name in the list.
    pub fn tip(self) -> &'static str {
        match self {
            Page::Audio => "The audio device, its format and channels, latency and the test tone",
            Page::Midi => "MIDI inputs and outputs, and hearing notes while editing",
            Page::Recording => "Count-in, Overdub, played notes and the metronome",
            Page::Editing => "Undo, autosave, automation and new tracks",
            Page::Mixing => "Solo and the meters",
            Page::Plugins => "Where plugins are looked for, blocked plugins and scanning",
            Page::Files => "Projects, imported audio, the cache and crash reports",
            Page::Appearance => "Theme, interface size, motion and the info line",
        }
    }
}

/// A row search can find: its key (which the page that draws it names), its
/// page, the name a result shows, and other words that find it.
#[derive(Debug)]
pub struct Row {
    pub key: &'static str,
    pub page: Page,
    pub name: &'static str,
    pub words: &'static str,
}

const fn row(key: &'static str, page: Page, name: &'static str, words: &'static str) -> Row {
    Row {
        key,
        page,
        name,
        words,
    }
}

/// Every row search can find, page by page. A row's key is drawn by its
/// page through [`Marks::row`]; the E2E suite checks each is drawn, and a
/// test here that each has words.
pub const ROWS: &[Row] = &[
    row(
        "audio.driver",
        Page::Audio,
        "Driver",
        "asio wasapi coreaudio alsa jack pipewire interface",
    ),
    row(
        "audio.device",
        Page::Audio,
        "Device",
        "audio interface sound card soundcard hardware",
    ),
    row(
        "audio.rate",
        Page::Audio,
        "Sample rate",
        "khz hz 44.1 48 96 frequency",
    ),
    row(
        "audio.buffer",
        Page::Audio,
        "Buffer size",
        "latency samples block control panel",
    ),
    row(
        "audio.output",
        Page::Audio,
        "Output channels",
        "outputs speakers main pair",
    ),
    row(
        "audio.cue",
        Page::Audio,
        "Cue output",
        "headphones cue pair monitoring",
    ),
    row(
        "audio.input",
        Page::Audio,
        "Input channels",
        "inputs record microphone mic",
    ),
    row(
        "audio.latency",
        Page::Audio,
        "Latency",
        "delay round trip milliseconds",
    ),
    row(
        "audio.offset",
        Page::Audio,
        "Recording offset",
        "latency compensation measure loopback cable delay",
    ),
    row(
        "audio.test_tone",
        Page::Audio,
        "Test tone",
        "sine 1 khz check sound",
    ),
    row(
        "audio.from_disk",
        Page::Audio,
        "Play audio from disk",
        "stream streaming memory long files",
    ),
    row(
        "midi.inputs",
        Page::Midi,
        "MIDI inputs",
        "keyboard controller ports track remote",
    ),
    row(
        "midi.outputs",
        Page::Midi,
        "MIDI outputs",
        "ports sync clock delay offset hardware synth",
    ),
    row(
        "midi.rescan",
        Page::Midi,
        "Look for MIDI devices",
        "rescan refresh plug ports",
    ),
    row(
        "midi.audition",
        Page::Midi,
        "Play notes while editing",
        "audition preview hear piano roll",
    ),
    row(
        "record.count_in",
        Page::Recording,
        "Count-in",
        "pre-roll bars before recording click",
    ),
    row(
        "record.only_while_recording",
        Page::Recording,
        "Metronome only while recording",
        "click silent",
    ),
    row(
        "record.overdub",
        Page::Recording,
        "Overdub or Replace",
        "takes merge record mode",
    ),
    row(
        "record.keep_played",
        Page::Recording,
        "Keep played notes for Capture MIDI",
        "capture retroactive memory",
    ),
    row(
        "record.metronome_volume",
        Page::Recording,
        "Metronome volume",
        "click level loud",
    ),
    row(
        "record.metronome_sound",
        Page::Recording,
        "Metronome sound",
        "click tone beep",
    ),
    row(
        "record.metronome_output",
        Page::Recording,
        "Metronome output",
        "click cue headphones main",
    ),
    row(
        "edit.undo_steps",
        Page::Editing,
        "Undo steps",
        "history memory depth levels redo",
    ),
    row(
        "edit.autosave",
        Page::Editing,
        "Autosave every",
        "backup crash recovery interval minutes seconds save",
    ),
    row(
        "edit.lock_envelopes",
        Page::Editing,
        "Lock envelopes",
        "automation clips move",
    ),
    row(
        "edit.new_audio_monitor",
        Page::Editing,
        "New audio tracks: monitoring",
        "default track input monitor in auto off",
    ),
    row(
        "edit.new_midi_instrument",
        Page::Editing,
        "New MIDI tracks: instrument",
        "default track synth preset poly keys bass pluck pad none",
    ),
    row(
        "mix.additive_solo",
        Page::Mixing,
        "Additive solo",
        "exclusive solo ctrl click",
    ),
    row(
        "mix.track_colour_meters",
        Page::Mixing,
        "Meters in track colors",
        "colour levels",
    ),
    row(
        "plugins.folders",
        Page::Plugins,
        "Plugin folders",
        "vst3 paths search locations add directory",
    ),
    row(
        "plugins.standard",
        Page::Plugins,
        "Standard plugin folders",
        "vst3 default system paths",
    ),
    row(
        "plugins.blocked",
        Page::Plugins,
        "Blocked plugins",
        "crashed failed retry vst3 scan problems",
    ),
    row(
        "plugins.scan_at_startup",
        Page::Plugins,
        "Scan plugins at startup",
        "launch background",
    ),
    row(
        "plugins.rescan",
        Page::Plugins,
        "Rescan plugins",
        "scan again refresh vst3 all",
    ),
    row(
        "files.projects_folder",
        Page::Files,
        "Projects folder",
        "songs save open new location directory documents",
    ),
    row(
        "files.copy_imported",
        Page::Files,
        "Copy imported audio into the project",
        "collect import samples media",
    ),
    row(
        "files.cache_folder",
        Page::Files,
        "Cache folder",
        "converted audio waveforms peaks location directory disk",
    ),
    row(
        "files.cache_limit",
        Page::Files,
        "Cache limit",
        "size gb disk space converted",
    ),
    row(
        "files.cache_clear",
        Page::Files,
        "Clear cache",
        "size space delete converted waveforms disk",
    ),
    row(
        "files.crash_reports",
        Page::Files,
        "Crash reports",
        "logs crashes folder problems",
    ),
    row(
        "look.theme",
        Page::Appearance,
        "Theme",
        "dark light system colour color mode",
    ),
    row(
        "look.scale",
        Page::Appearance,
        "Interface size",
        "scale zoom dpi bigger smaller",
    ),
    row(
        "look.reduce_motion",
        Page::Appearance,
        "Reduce motion",
        "animation accessibility",
    ),
    row(
        "look.info_line",
        Page::Appearance,
        "Info line",
        "status bar hints help bottom",
    ),
];

/// The rows that match `query`, best first: rows whose name starts with it,
/// then those whose name holds it, then those found by their other words
/// or their page's name; in page order within each. Every word of the
/// query must be found. Nothing for an empty query.
pub fn search(query: &str) -> Vec<&'static Row> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let whole = words.join(" ");
    let mut found: Vec<(u8, &'static Row)> = ROWS
        .iter()
        .filter_map(|row| {
            let name = row.name.to_lowercase();
            let all = format!("{name} {} {}", row.words, row.page.name().to_lowercase());
            if !words.iter().all(|w| all.contains(w.as_str())) {
                return None;
            }
            let rank = if name.starts_with(&whole) {
                0
            } else if name.contains(&whole) {
                1
            } else {
                2
            };
            Some((rank, row))
        })
        .collect();
    // A stable sort keeps the page order inside a rank.
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, row)| row).collect()
}

/// How the window fits the room the app's window leaves: its width, and
/// whether the page list folds into a menu above the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub width: f32,
    pub folded: bool,
}

/// The fit for `room` points of width.
pub fn fit(room: f32) -> Fit {
    let width = room.clamp(theme::size::PREFERENCES_LEAST, theme::size::PREFERENCES);
    Fit {
        width,
        folded: width < theme::size::PREFERENCES_FOLD,
    }
}

/// The row a search led to, outlined for a moment.
#[derive(Debug, Clone, Copy)]
struct Mark {
    key: &'static str,
    /// When it was chosen (input time, seconds).
    since: f64,
    /// It has yet to be scrolled into view.
    scroll: bool,
}

/// The window between frames (not saved; the page is remembered for the
/// session).
#[derive(Default)]
pub struct Window {
    pub open: bool,
    pub page: Page,
    pub query: String,
    mark: Option<Mark>,
    /// The search field takes the keyboard on the next frame.
    pub focus_search: bool,
    /// The Audio page's device lists and loopback card.
    pub audio: audio::State,
}

impl Window {
    /// Opens the window on the page shown last.
    pub fn show(&mut self) {
        if !self.open {
            self.focus_search = true;
        }
        self.open = true;
    }

    /// Opens the window on `page`.
    pub fn show_page(&mut self, page: Page) {
        self.show();
        self.page = page;
        self.query.clear();
    }

    /// Opens the window at the loopback helper, on the Audio page.
    pub fn show_measure(&mut self) {
        self.show_page(Page::Audio);
        // The helper is the point: the keyboard stays with the app (Space
        // still plays), not with the search.
        self.focus_search = false;
        self.audio.open_measure();
    }

    /// Goes to the page of `row`, a search result, and marks the row.
    pub fn choose(&mut self, row: &Row, now: f64) {
        self.page = row.page;
        self.query.clear();
        self.mark = Some(Mark {
            key: row.key,
            since: now,
            scroll: true,
        });
    }

    /// The mark to draw this frame, at `now`; a mark that has had its time
    /// goes.
    pub fn marks(&mut self, now: f64) -> Marks {
        let shown = motion::NOTE.as_secs_f32();
        let fade = motion::time(motion::BASE);
        let Some(mark) = self.mark.filter(|m| ((now - m.since) as f32) < shown) else {
            self.mark = None;
            return Marks::default();
        };
        let left = shown - (now - mark.since) as f32;
        let strength = if fade > 0.0 {
            (left / fade).min(1.0)
        } else {
            1.0
        };
        Marks {
            key: Some(mark.key),
            since: Some(mark.since),
            strength,
            scroll: Cell::new(mark.scroll),
        }
    }

    /// Notes what drawing `marks` did: the row, once scrolled to, stays put.
    /// A mark chosen while drawing (a result clicked) is a newer one, still
    /// to be scrolled to: only the mark that was drawn is changed.
    pub fn marked(&mut self, marks: &Marks) {
        if let Some(mark) = &mut self.mark
            && marks.since == Some(mark.since)
        {
            mark.scroll = marks.scroll.get();
        }
    }

    /// The row marked now, if any.
    pub fn marked_row(&self) -> Option<&'static str> {
        self.mark.map(|m| m.key)
    }
}

/// The row a search led to, as the pages draw it.
#[derive(Debug, Default)]
pub struct Marks {
    key: Option<&'static str>,
    /// When the mark drawn was chosen: which mark this is.
    since: Option<f64>,
    /// 1 while shown, falling to 0 as it fades.
    strength: f32,
    scroll: Cell<bool>,
}

impl Marks {
    /// Draws the row `key` with `add`; when it is the marked row, outlines
    /// it (a shape, so the mark does not rest on colour alone) and, the
    /// first time, scrolls it into view.
    pub fn row<R>(&self, ui: &mut Ui, key: &'static str, add: impl FnOnce(&mut Ui) -> R) -> R {
        let drawn = ui.scope(add);
        crate::testing::coverage::row_drawn(ui.ctx(), key);
        if self.key == Some(key) {
            let rect = drawn.response.rect.expand(theme::space::XS);
            let colour = theme::p().accent.gamma_multiply(self.strength);
            ui.painter().rect_stroke(
                rect,
                f32::from(theme::radius::MEDIUM),
                egui::Stroke::new(theme::stroke::FOCUS, colour),
                egui::StrokeKind::Outside,
            );
            if self.scroll.replace(false) {
                ui.scroll_to_rect(rect, Some(egui::Align::Center));
            }
            ui.ctx().request_repaint();
        }
        drawn.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_has_words_and_a_key_of_its_own() {
        let mut keys = std::collections::HashSet::new();
        for row in ROWS {
            assert!(!row.words.trim().is_empty(), "{} has no words", row.key);
            assert!(!row.name.is_empty(), "{} has no name", row.key);
            assert!(keys.insert(row.key), "{} is listed twice", row.key);
        }
    }

    #[test]
    fn every_page_has_a_row() {
        for page in Page::ALL {
            assert!(
                ROWS.iter().any(|r| r.page == page),
                "{} has no rows",
                page.name()
            );
        }
    }

    #[test]
    fn latency_finds_the_buffer_and_the_recording_offset() {
        let found: Vec<_> = search("latency").iter().map(|r| r.key).collect();
        assert_eq!(found.first(), Some(&"audio.latency"), "the name first");
        assert!(found.contains(&"audio.buffer"));
        assert!(found.contains(&"audio.offset"));
    }

    #[test]
    fn search_ignores_case_and_needs_every_word() {
        assert_eq!(search("DARK")[0].key, "look.theme");
        assert_eq!(search("metronome cue")[0].key, "record.metronome_output");
        assert!(search("metronome nothing-like-this").is_empty());
        assert!(search("   ").is_empty());
    }

    #[test]
    fn a_page_name_finds_its_rows() {
        let found = search("plugins");
        let on_page = ROWS.iter().filter(|r| r.page == Page::Plugins).count();
        assert_eq!(found.len(), on_page);
        assert!(found.iter().all(|r| r.page == Page::Plugins));
    }

    #[test]
    fn the_window_folds_when_narrow_and_never_grows_past_its_width() {
        let wide = fit(1200.0);
        assert_eq!(wide.width, theme::size::PREFERENCES);
        assert!(!wide.folded);
        // 960 × 540 at 200% leaves 480 points, less the window's margins.
        let small = fit(440.0);
        assert_eq!(small.width, 440.0);
        assert!(small.folded);
        assert!(fit(100.0).width >= theme::size::PREFERENCES_LEAST);
    }

    #[test]
    fn a_result_chosen_while_another_mark_is_drawn_still_scrolls_to_its_row() {
        let mut window = Window::default();
        window.choose(search("buffer")[0], 10.0);
        // The buffer's row is drawn and scrolled to.
        let drawn = window.marks(10.1);
        drawn.scroll.set(false);
        // In the same frame, the test tone is chosen.
        window.choose(search("test tone")[0], 10.1);
        window.marked(&drawn);
        let next = window.marks(10.2);
        assert_eq!(next.key, Some("audio.test_tone"));
        assert!(next.scroll.get(), "the new row is still to be scrolled to");
    }

    #[test]
    fn a_chosen_result_opens_its_page_and_its_mark_fades_after_a_while() {
        let mut window = Window {
            query: "buffer".into(),
            ..Window::default()
        };
        let row = search("buffer")[0];
        window.choose(row, 10.0);
        assert_eq!(window.page, Page::Audio);
        assert!(window.query.is_empty());
        let marks = window.marks(10.1);
        assert_eq!(marks.key, Some("audio.buffer"));
        assert_eq!(marks.strength, 1.0);
        assert!(marks.scroll.get(), "scrolled to once");
        let gone = window.marks(10.0 + motion::NOTE.as_secs_f64() + 0.01);
        assert_eq!(gone.key, None);
        assert_eq!(window.marked_row(), None);
    }
}
