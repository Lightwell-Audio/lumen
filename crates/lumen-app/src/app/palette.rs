//! The command palette (M3 slice 7b; the owner's P2 with P3's values).
//! Ctrl+K opens it over the arrangement. Typing finds commands by fuzzy
//! search; each result names what it will act on ("Duplicate 2 clips"),
//! and a command that cannot run now stays listed, dimmed, with the reason.
//! Typed values become actions: a number offers that tempo or that bar,
//! and a track's or locator's name goes to it. A strip under the list
//! explains the chosen entry. Shortcuts show as K5 keycaps.

use eframe::egui::{
    self, Align2, Color32, FontId, Key, Modifiers, RichText, Sense, Stroke, Vec2, text::LayoutJob,
    vec2,
};
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::navigate::ViewStep;
use crate::commands::{ALL, Command};
use crate::{App, icons, motion, theme, widgets};

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
    /// The room the strip keeps for what a row does: the tallest of every
    /// row's text at the strip's width and the pixel density, so the
    /// palette keeps its height as the chosen row changes.
    strip_room: Option<(f32, f32, f32)>,
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

/// Where `query` matches `text` as a subsequence, ignoring case: a score
/// (consecutive letters and starts of words count more; a text that starts
/// with the query more still, and the whole text most) and the matched
/// characters' indices. `None` when it does not match.
pub(crate) fn fuzzy(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    let query: Vec<char> = query.trim().to_lowercase().chars().collect();
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let mut matched = Vec::with_capacity(query.len());
    let mut score = 0;
    let mut q = 0;
    for (i, &c) in text.iter().enumerate() {
        if q < query.len() && c == query[q] {
            let start = i == 0 || !text[i - 1].is_alphanumeric();
            let follows = matched.last() == Some(&(i.wrapping_sub(1)));
            score += 1 + if follows { 3 } else { 0 } + if start { 4 } else { 0 };
            matched.push(i);
            q += 1;
        }
    }
    if q < query.len() {
        return None;
    }
    if text.starts_with(&query) {
        score += 10;
        if text.len() == query.len() {
            score += 10;
        }
    }
    Some((score, matched))
}

/// Values in the query: a tempo and a bar for a number ("128", "128 bpm",
/// "bar 17", "tempo 140").
fn values(query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    let number = |s: &str| s.trim().parse::<f64>().ok().filter(|n| n.is_finite());
    let tempo = |n: f64| (20.0..=999.0).contains(&n).then_some(Entry::Tempo(n));
    let bar = |n: f64| (n >= 1.0 && n.fract() == 0.0).then_some(Entry::Bar(n as i64));
    if let Some(n) = q.strip_prefix("bar").and_then(number) {
        return bar(n).into_iter().collect();
    }
    if let Some(n) = q
        .strip_prefix("tempo")
        .or_else(|| q.strip_suffix("bpm"))
        .and_then(number)
    {
        return tempo(n).into_iter().collect();
    }
    match number(&q) {
        Some(n) => tempo(n).into_iter().chain(bar(n)).collect(),
        None => Vec::new(),
    }
}

/// "2 clips", "clip “kick”".
fn count(n: usize, one: &str, many: &str, name: Option<&str>) -> String {
    match (n, name) {
        (1, Some(name)) => format!("{one} \u{201C}{name}\u{201D}"),
        (1, None) => one.to_owned(),
        _ => format!("{n} {many}"),
    }
}

impl App {
    /// Opens the palette, or closes it when open (Ctrl+K).
    pub(crate) fn toggle_palette(&mut self) {
        if self.palette.open {
            self.close_palette();
        } else {
            self.palette.open = true;
            self.palette.focus = true;
        }
    }

    fn close_palette(&mut self) {
        let recent = std::mem::take(&mut self.palette.recent);
        self.palette = Palette {
            recent,
            ..Palette::default()
        };
    }

    /// What `command` will act on, in words: "Duplicate 2 clips", "Undo:
    /// Move clip", "Rename track “Bass”". Plain names otherwise.
    pub(crate) fn command_label(&self, command: Command) -> String {
        let project = self.document.project();
        let clips: Vec<_> = project
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| self.arrangement.clips.contains(&c.id))
            .collect();
        let tracks: Vec<_> = project
            .tracks
            .iter()
            .filter(|t| self.arrangement.selected.contains(&t.id))
            .collect();
        let locator = self
            .locators
            .selected
            .and_then(|id| project.markers.iter().find(|m| m.id == id));
        let first_clip = clips.first().map(|c| c.name.as_str());
        let first_track = tracks.first().map(|t| t.name.as_str());
        let what = || {
            if self.arrangement.range.is_some() {
                Some("time range".to_owned())
            } else if !clips.is_empty() {
                Some(count(clips.len(), "clip", "clips", first_clip))
            } else if !tracks.is_empty() {
                Some(count(tracks.len(), "track", "tracks", first_track))
            } else {
                locator.map(|m| count(1, "locator", "locators", Some(&m.name)))
            }
        };
        let name = command.name();
        let history = self.document.history();
        match command {
            Command::Undo => history.undo_label().map(|l| format!("Undo: {l}")),
            Command::Redo => history.redo_label().map(|l| format!("Redo: {l}")),
            Command::Cut | Command::Copy | Command::Duplicate | Command::Delete => {
                what().map(|w| format!("{name} {w}"))
            }
            Command::Rename => what().map(|w| format!("Rename {w}")),
            Command::MuteClips if !clips.is_empty() => {
                let verb = if clips.iter().all(|c| c.muted) {
                    "Unmute"
                } else {
                    "Mute"
                };
                Some(format!(
                    "{verb} {}",
                    count(clips.len(), "clip", "clips", first_clip)
                ))
            }
            Command::ClipProperties => {
                first_clip.map(|n| format!("Properties of \u{201C}{n}\u{201D}"))
            }
            // The arrows follow the selection (M3 review H2).
            Command::FoldTracks if !clips.is_empty() => Some("Previous clip on the track".into()),
            Command::UnfoldTracks if !clips.is_empty() => Some("Next clip on the track".into()),
            Command::SelectTrackAbove if !clips.is_empty() => Some("Select the clip above".into()),
            Command::SelectTrackBelow if !clips.is_empty() => Some("Select the clip below".into()),
            Command::ExtendSelectionUp if !clips.is_empty() => {
                Some("Add the clip above to the selection".into())
            }
            Command::ExtendSelectionDown if !clips.is_empty() => {
                Some("Add the clip below to the selection".into())
            }
            Command::FoldTracks | Command::UnfoldTracks if !tracks.is_empty() => {
                let verb = if command == Command::FoldTracks {
                    "Fold"
                } else {
                    "Unfold"
                };
                Some(format!(
                    "{verb} {}",
                    count(tracks.len(), "track", "tracks", first_track)
                ))
            }
            Command::MoveTracksUp | Command::MoveTracksDown if !tracks.is_empty() => {
                let way = if command == Command::MoveTracksUp {
                    "up"
                } else {
                    "down"
                };
                Some(format!(
                    "Move {} {way}",
                    count(tracks.len(), "track", "tracks", first_track)
                ))
            }
            _ => None,
        }
        .unwrap_or_else(|| name.to_owned())
    }

    /// The palette's rows for its query: values first, then commands and
    /// names by how well they match (the table's order on ties); with no
    /// query, recent commands first, then every command.
    pub(crate) fn palette_rows(&self, ctx: &egui::Context) -> Vec<Row> {
        let query = self.palette.query.trim();
        let project = self.document.project();
        let mut rows: Vec<Row> = values(query)
            .into_iter()
            .map(|entry| {
                let label = match entry {
                    Entry::Tempo(bpm) => format!("Set tempo to {} BPM", trim_number(bpm)),
                    Entry::Bar(bar) => format!("Go to bar {bar}"),
                    _ => unreachable!("values are tempos and bars"),
                };
                Row {
                    entry,
                    label,
                    matched: Vec::new(),
                    ready: Ok(()),
                    score: i32::MAX,
                }
            })
            .collect();
        let mut found: Vec<Row> = Vec::new();
        for info in ALL {
            let label = self.command_label(info.command);
            // The name finds it even when the label says more ("Properties
            // of “kick”" is found by "clip prop").
            let hit = fuzzy(query, &label)
                .or_else(|| fuzzy(query, info.name).map(|(score, _)| (score - 1, Vec::new())));
            if let Some((score, matched)) = hit {
                found.push(Row {
                    entry: Entry::Command(info.command),
                    label,
                    matched,
                    ready: self.availability(ctx, info.command),
                    score,
                });
            }
        }
        if query.chars().count() >= 2 {
            let names = project
                .tracks
                .iter()
                .map(|t| (Entry::Track(t.id), "Go to track", &t.name))
                .chain(
                    project
                        .markers
                        .iter()
                        .map(|m| (Entry::Locator(m.at), "Go to locator", &m.name)),
                );
            for (entry, verb, name) in names {
                if let Some((score, matched)) = fuzzy(query, name) {
                    let prefix = verb.chars().count() + 2;
                    found.push(Row {
                        entry,
                        label: format!("{verb} \u{201C}{name}\u{201D}"),
                        matched: matched.into_iter().map(|i| i + prefix).collect(),
                        ready: Ok(()),
                        score,
                    });
                }
            }
        }
        if query.chars().count() >= 2 {
            for (i, template) in self.templates.listing.found().iter().enumerate() {
                let label = format!("New from template \u{201C}{}\u{201D}", template.name);
                if let Some((score, matched)) = fuzzy(query, &label) {
                    found.push(Row {
                        entry: Entry::Template(i),
                        label,
                        matched,
                        ready: self.new_unavailable(),
                        score,
                    });
                }
            }
        }
        if query.is_empty() {
            // Recent first, in the order they were run.
            let rank = |row: &Row| match row.entry {
                Entry::Command(c) => self
                    .palette
                    .recent
                    .iter()
                    .position(|&r| r == c)
                    .unwrap_or(usize::MAX),
                _ => usize::MAX,
            };
            found.sort_by_key(rank);
        } else {
            found.sort_by_key(|row| std::cmp::Reverse(row.score));
        }
        rows.extend(found);
        rows
    }

    /// How many of the empty palette's first rows are recent commands.
    fn recent_shown(&self) -> usize {
        if self.palette.query.trim().is_empty() {
            self.palette.recent.len()
        } else {
            0
        }
    }

    /// Runs a row: a command that can run, a value, or a name. A command
    /// that cannot run leaves the palette open and says why.
    fn run_row(&mut self, ctx: &egui::Context, row: &Row) {
        if let Err(why) = row.ready {
            self.palette.notice = Some(why);
            return;
        }
        self.close_palette();
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        match row.entry {
            Entry::Command(command) => {
                self.palette.recent.retain(|&c| c != command);
                self.palette.recent.insert(0, command);
                self.palette.recent.truncate(RECENT);
                self.settings.commands.recent = self.palette.recent_ids();
                self.settings_changed = Some(std::time::Instant::now());
                self.run(ctx, command);
            }
            Entry::Tempo(bpm) => {
                let at = self.transport.playhead();
                self.document.set_tempo_at(at, bpm, None);
            }
            Entry::Bar(bar) => {
                let at = self.transport.timeline.meter.bar_start(bar - 1);
                self.transport.locate(at);
                self.transport.follow_paused = false;
            }
            Entry::Track(id) => self.nav.steps.push(ViewStep::Reveal(id)),
            Entry::Locator(at) => {
                self.transport.locate(at);
                self.transport.follow_paused = false;
            }
            Entry::Template(i) => self.new_from_listed_template(ctx, i),
        }
        ctx.request_repaint();
    }

    /// The palette, when open: the search field, the rows, the strip that
    /// explains the chosen one, and the keys. Esc, Ctrl+K or a click
    /// elsewhere closes it.
    pub(crate) fn palette_window(&mut self, ctx: &egui::Context) {
        if !self.palette.open {
            return;
        }
        // Templates are rows too: a template added in the file manager shows
        // up within a moment of the palette opening.
        self.templates.listing.ask(ctx, self.templates_dir(), false);
        // The list's keys, before the field sees them.
        // Ctrl+Enter records new keys for the chosen command (the owner's
        // E3); while recording, `capture_chord` has taken the keys.
        let (rekey, up, down, enter, close) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape)
                    || i.consume_key(Modifiers::COMMAND, Key::K),
            )
        });
        if close {
            self.close_palette();
            return;
        }
        let rows = self.palette_rows(ctx);
        let last = rows.len().saturating_sub(1);
        if up {
            self.palette.chosen = self.palette.chosen.checked_sub(1).unwrap_or(last);
        }
        if down {
            self.palette.chosen = if self.palette.chosen >= last {
                0
            } else {
                self.palette.chosen + 1
            };
        }
        self.palette.chosen = self.palette.chosen.min(last);
        if up || down {
            self.palette.notice = None;
        }
        if rekey
            && let Some(Entry::Command(command)) = rows.get(self.palette.chosen).map(|r| &r.entry)
        {
            let slot = (!self.keymap.chords(*command).is_empty()).then_some(0);
            self.start_rebind(*command, slot);
        }
        if enter && let Some(row) = rows.get(self.palette.chosen) {
            self.run_row(ctx, row);
            return;
        }

        let palette = *theme::p();
        // The click that opened it (the transport button) is not a click
        // elsewhere.
        let fresh = self.palette.focus;
        let mut picked = None;
        let shown = egui::Area::new(egui::Id::new("command palette"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_TOP, vec2(0.0, TOP))
            .show(ctx, |ui| {
                // A window-sized floating surface (the Palette card):
                // `radius::XL`, `overlay`, `border_strong` and the edge
                // light along its top inside edge.
                let frame = egui::Frame::popup(ui.style())
                    .fill(palette.overlay)
                    .stroke(Stroke::new(theme::stroke::HAIRLINE, palette.border_strong))
                    .corner_radius(theme::radius::XL)
                    .inner_margin(theme::space::S)
                    .show(ui, |ui| {
                        ui.set_width(WIDTH);
                        self.palette_field(ui);
                        ui.separator();
                        picked = self.palette_list(ui, &rows, up || down);
                        ui.separator();
                        self.palette_strip(ui, rows.get(self.palette.chosen));
                    });
                widgets::raised::lip(
                    ui.painter(),
                    frame.response.rect,
                    theme::radius::XL,
                    palette.edge_light,
                );
            });
        if let Some(i) = picked
            && let Some(row) = rows.get(i)
        {
            self.run_row(ctx, row);
        } else if !fresh && shown.response.clicked_elsewhere() {
            self.close_palette();
        }
    }

    fn palette_field(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::splat(theme::size::CONTROL_SMALL), Sense::hover());
            ui.painter().extend(icons::shapes(
                icons::Icon::Search,
                rect,
                theme::p().text_hint,
            ));
            let before = self.palette.query.clone();
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.palette.query)
                    .id(egui::Id::new("command palette field"))
                    .frame(egui::Frame::NONE)
                    .font(FontId::proportional(theme::size::TITLE))
                    .hint_text("Search commands, or type a tempo, a bar or a track")
                    .desired_width(f32::INFINITY),
            );
            field.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search commands")
            });
            if std::mem::take(&mut self.palette.focus) || !field.has_focus() {
                field.request_focus();
            }
            if self.palette.query != before {
                self.palette.chosen = 0;
                self.palette.notice = None;
            }
        });
    }

    /// The rows; the index of one clicked.
    fn palette_list(&mut self, ui: &mut egui::Ui, rows: &[Row], moved: bool) -> Option<usize> {
        let palette = *theme::p();
        let recent = self.recent_shown().min(rows.len());
        let mut picked = None;
        // Short windows show fewer rows, so the palette never runs over
        // the transport bar or off the bottom.
        let room = ui.ctx().content_rect().height() - TOP - CHROME;
        egui::ScrollArea::vertical()
            .max_height(room.clamp(ROW * 3.0, ROW * ROWS))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if rows.is_empty() {
                    ui.add_space(theme::space::S);
                    ui.label(
                        RichText::new(format!(
                            "No command matches \u{201C}{}\u{201D}. Try fewer letters.",
                            self.palette.query.trim()
                        ))
                        .color(palette.text_hint),
                    );
                    ui.add_space(theme::space::S);
                }
                // The chosen row's fill, drawn under the rows once they are
                // laid out, slides from row to row (M4.5, M5).
                let highlight = ui.painter().add(egui::Shape::Noop);
                let list_top = ui.min_rect().top();
                let mut chosen_rect = None;
                for (i, row) in rows.iter().enumerate() {
                    if recent > 0 && (i == 0 || i == recent) {
                        let heading = if i == 0 { "Recent" } else { "All commands" };
                        ui.label(
                            RichText::new(heading)
                                .size(theme::size::CAPTION)
                                .color(palette.text_hint),
                        );
                    }
                    let (response, rekey) = self.palette_row(ui, row, i == self.palette.chosen);
                    if i == self.palette.chosen {
                        chosen_rect = Some(response.rect);
                    }
                    if let (Some(slot), Entry::Command(command)) = (rekey, &row.entry) {
                        self.palette.chosen = i;
                        self.start_rebind(*command, Some(slot));
                    }
                    if i == self.palette.chosen && moved {
                        response.scroll_to_me(None);
                    }
                    // Hover picks a row only when the pointer moves, so a
                    // resting pointer does not fight the arrow keys.
                    if response.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                        self.palette.chosen = i;
                    }
                    if response.clicked() {
                        picked = Some(i);
                    }
                }
                if let Some(rect) = chosen_rect {
                    let y = motion::glide_over(
                        ui.ctx(),
                        egui::Id::new("palette highlight"),
                        rect.top() - list_top,
                        true,
                        motion::FAST,
                    );
                    let shown = rect.translate(vec2(0.0, list_top + y - rect.top()));
                    ui.painter().set(
                        highlight,
                        egui::Shape::rect_filled(
                            shown,
                            theme::radius::MEDIUM,
                            theme::p().accent_muted,
                        ),
                    );
                }
            });
        picked
    }

    /// A row; with the chord slot whose keycaps were clicked, to record
    /// new keys for it. The `chosen` row is under the list's sliding
    /// highlight, so its keycaps step up; a hovered row that is not chosen
    /// (the pointer rests while the arrow keys move) is Ember.
    fn palette_row(
        &self,
        ui: &mut egui::Ui,
        row: &Row,
        chosen: bool,
    ) -> (egui::Response, Option<usize>) {
        let mut rekey = None;
        let palette = *theme::p();
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
        let keys = match row.entry {
            Entry::Command(command) => self.keymap.text(command),
            _ => None,
        };
        let name = match (&row.ready, keys) {
            (Err(why), _) => format!("{}, unavailable: {why}", row.label),
            (Ok(()), Some(keys)) => format!("{}, {keys}", row.label),
            (Ok(()), None) => row.label.clone(),
        };
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, row.ready.is_ok(), &name)
        });
        if !ui.is_rect_visible(rect) {
            return (response, None);
        }
        // The chosen row's fill is drawn by the list, sliding (M4.5).
        if response.hovered() && !chosen {
            ui.painter()
                .rect_filled(rect, theme::radius::MEDIUM, palette.ember);
        }
        let text = if row.ready.is_ok() {
            palette.text
        } else {
            palette.text_disabled
        };
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(theme::space::S, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.label(highlighted(&row.label, &row.matched, text, palette.accent));
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Entry::Command(command) = row.entry {
                let area = command.info().area;
                if let Some(chord) = self.keymap.chords(command).first() {
                    let caps = widgets::keycaps(
                        ui,
                        &chord.modifier_names(),
                        chord.key_name(),
                        area.light(),
                        chosen,
                    );
                    caps.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("Change {} shortcut {}", command.name(), chord.text()),
                        )
                    });
                    if caps.clicked() {
                        rekey = Some(0);
                    }
                }
                ui.label(
                    RichText::new(area.name())
                        .size(theme::size::CAPTION)
                        .color(palette.text_hint),
                );
            }
        });
        (response, rekey)
    }

    /// What the chosen row does, or why it cannot run, then the list's keys
    /// with every chord that runs it. The text keeps the room of the
    /// longest a row can say, so the palette never changes height as the
    /// chosen row changes.
    fn palette_strip(&mut self, ui: &mut egui::Ui, row: Option<&Row>) {
        let palette = *theme::p();
        let rebind = match row.map(|r| &r.entry) {
            Some(Entry::Command(command)) => self.rebind.filter(|r| r.command == *command),
            _ => None,
        };
        let said;
        let (text, colour) = match (row, rebind) {
            (None, _) => ("", palette.text_secondary),
            (Some(_), Some(r)) => match r.clash {
                Some((chord, other)) => {
                    said = format!(
                        "{} is used by {}. Enter takes it \u{B7} Esc cancels",
                        chord.text(),
                        other.name()
                    );
                    (said.as_str(), palette.warning)
                }
                None => {
                    said = format!(
                        "Press the new keys for {} \u{B7} Esc cancels",
                        r.command.name()
                    );
                    (said.as_str(), palette.accent)
                }
            },
            (Some(row), None) => match (self.palette.notice, row.ready) {
                (Some(why), _) | (None, Err(why)) => (why, palette.warning),
                (None, Ok(())) => (row.entry.about(), palette.text_secondary),
            },
        };
        let width = ui.available_width();
        let font = FontId::proportional(theme::size::BODY);
        let ppp = ui.ctx().pixels_per_point();
        let room = match self.palette.strip_room {
            Some((w, p, room)) if w == width && p == ppp => room,
            _ => {
                let room = Entry::every_about()
                    .map(|about| {
                        ui.fonts_mut(|f| {
                            f.layout(about.to_owned(), font.clone(), colour, width)
                                .size()
                                .y
                        })
                    })
                    .fold(0.0, f32::max);
                self.palette.strip_room = Some((width, ppp, room));
                room
            }
        };
        let galley = ui.fonts_mut(|f| f.layout(text.to_owned(), font, colour, width));
        let (rect, response) =
            ui.allocate_exact_size(vec2(width, room.max(galley.size().y)), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
        ui.painter().galley(rect.left_top(), galley, colour);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(
                    "\u{2191}\u{2193} move \u{B7} Enter run \u{B7} Ctrl+Enter change keys \u{B7} Esc close",
                )
                .size(theme::size::CAPTION)
                .color(palette.text_hint),
            );
            let Some(Entry::Command(command)) = row.map(|r| &r.entry) else {
                return;
            };
            let area = command.info().area;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let chords = self.keymap.chords(*command);
                for (i, chord) in chords.iter().enumerate().rev() {
                    let caps = widgets::keycaps(
                        ui,
                        &chord.modifier_names(),
                        chord.key_name(),
                        area.light(),
                        false,
                    );
                    caps.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Label,
                            true,
                            format!("Shortcut {}", chord.text()),
                        )
                    });
                    if i > 0 {
                        ui.label(RichText::new("or").color(palette.text_hint));
                    }
                }
            });
        });
    }
}

/// `label` with the characters at `matched` in `accent`.
fn highlighted(label: &str, matched: &[usize], text: Color32, accent: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = FontId::proportional(theme::size::BODY);
    for (i, c) in label.chars().enumerate() {
        let colour = if matched.contains(&i) { accent } else { text };
        job.append(
            &c.to_string(),
            0.0,
            egui::TextFormat::simple(font.clone(), colour),
        );
    }
    job
}

/// "128", "128.5".
fn trim_number(n: f64) -> String {
    let text = format!("{n:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_search_prefers_word_starts_and_runs() {
        let (dup, at) = fuzzy("dup", "Duplicate 2 clips").expect("matches");
        assert_eq!(at, [0, 1, 2]);
        let (spread, _) = fuzzy("dup", "Add audio track up").expect("matches too");
        assert!(dup > spread, "a run at a word start scores higher");
        assert!(fuzzy("xyz", "Duplicate").is_none());
        assert_eq!(fuzzy("  ", "Anything"), Some((0, Vec::new())));
        let (_, at) = fuzzy("AT", "Add audio track").expect("ignores case");
        assert_eq!(at, [0, 10]);
        // The command named exactly so beats word starts elsewhere.
        let (cut, _) = fuzzy("cut", "Cut").expect("matches");
        let (initials, _) = fuzzy("cut", "Next clip or unfold track").expect("matches");
        assert!(cut > initials);
    }

    #[test]
    fn numbers_become_a_tempo_and_a_bar() {
        assert_eq!(values("128"), [Entry::Tempo(128.0), Entry::Bar(128)]);
        assert_eq!(values("12"), [Entry::Bar(12)], "too slow for a tempo");
        assert_eq!(values("128.5"), [Entry::Tempo(128.5)], "not a bar");
        assert_eq!(values("bar 1000"), [Entry::Bar(1000)]);
        assert_eq!(values("140 bpm"), [Entry::Tempo(140.0)]);
        assert_eq!(values("tempo 90"), [Entry::Tempo(90.0)]);
        assert_eq!(values("tempo 5"), []);
        assert_eq!(values("kick"), []);
        assert_eq!(trim_number(128.0), "128");
        assert_eq!(trim_number(128.25), "128.25");
    }

    #[test]
    fn counts_read_naturally() {
        assert_eq!(
            count(1, "clip", "clips", Some("kick")),
            "clip \u{201C}kick\u{201D}"
        );
        assert_eq!(count(3, "clip", "clips", Some("kick")), "3 clips");
    }
}
