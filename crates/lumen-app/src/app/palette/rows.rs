//! The palette's state and rows: opening and closing it, the rows for its
//! query, and running one.

use eframe::egui;

use super::search::{self, trim_number};
use super::{Entry, Palette, RECENT, Row};
use crate::App;
use crate::app::navigate::ViewStep;
use crate::commands::ALL;

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

    pub(super) fn close_palette(&mut self) {
        let recent = std::mem::take(&mut self.palette.recent);
        let strip_room = self.palette.strip_room;
        self.palette = Palette {
            recent,
            strip_room,
            ..Palette::default()
        };
    }

    /// The palette's rows for its query: values first, then commands and
    /// names by how well they match (the table's order on ties); with no
    /// query, recent commands first, then every command.
    pub(crate) fn palette_rows(&self, ctx: &egui::Context) -> Vec<Row> {
        let query = self.palette.query.trim();
        let project = self.document.project();
        let mut rows: Vec<Row> = search::values(query)
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
            let hit = search::fuzzy(query, &label).or_else(|| {
                search::fuzzy(query, info.name).map(|(score, _)| (score - 1, Vec::new()))
            });
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
                if let Some((score, matched)) = search::fuzzy(query, name) {
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
                if let Some((score, matched)) = search::fuzzy(query, &label) {
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
    pub(super) fn recent_shown(&self) -> usize {
        if self.palette.query.trim().is_empty() {
            self.palette.recent.len()
        } else {
            0
        }
    }

    /// Runs a row: a command that can run, a value, or a name. A command
    /// that cannot run leaves the palette open and says why.
    pub(super) fn run_row(&mut self, ctx: &egui::Context, row: &Row) {
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
}
