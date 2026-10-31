//! The Preferences window (M9 R7, the owner's PW A): a search field and the
//! list of pages at the left, the page at the right. Below
//! `PREFERENCES_FOLD` points the list folds into a page menu above the
//! page. While the search has words, its results stand in for the page; a
//! result opens its page with its row outlined.

use std::time::Instant;

use eframe::egui::{self, Align, Layout, Ui, vec2};

use crate::preferences::{self, Fit, Marks, Page};
use crate::{App, midi, theme, widgets};

/// Where the window's top sits below the app's at most, and as a share of
/// a short window's height.
const TOP: f32 = 72.0;
const TOP_SHARE: f32 = 0.12;
/// Room kept under the window.
const BELOW: f32 = 40.0;
/// The page's least height: a very short window scrolls rather than cuts.
const LEAST_PAGE: f32 = 80.0;
/// The search field's words when empty, and its accessible name.
const SEARCH: &str = "Search preferences";

impl App {
    /// Each frame: opens the window when the transport asked, draws it while
    /// open, and carries out what the MIDI chip and the MIDI page asked.
    pub(crate) fn preferences_window(&mut self, ctx: &egui::Context) {
        if let Some(page) = self.transport.open_preferences.take() {
            self.preferences.show_page(page);
        }
        let mut midi_asked = self.transport.take_midi_asked();
        if self.preferences.open {
            self.draw_preferences(ctx, &mut midi_asked);
        }
        for action in midi_asked {
            self.midi_asked(&action);
        }
    }

    /// Applies a MIDI port change and remembers it in the settings.
    fn midi_asked(&mut self, action: &midi::Action) {
        self.midi.apply(action);
        match action {
            midi::Action::Use(id, using) => {
                midi::Midi::remember(&mut self.settings.midi, id, *using);
                self.settings_changed = Some(Instant::now());
            }
            midi::Action::SyncDelay(name, ms) => {
                midi::Midi::set_sync_delay(&mut self.settings.midi, name, *ms);
                self.settings_changed = Some(Instant::now());
            }
            midi::Action::Rescan => {}
        }
    }

    fn draw_preferences(&mut self, ctx: &egui::Context, midi_asked: &mut Vec<midi::Action>) {
        let content = ctx.content_rect();
        let margins = ctx.global_style().spacing.window_margin.sum().x + 2.0 * theme::space::M;
        let fit = preferences::fit(content.width() - margins);
        let top = content.top() + TOP.min(content.height() * TOP_SHARE);
        // Esc with words in the search clears them; the next Esc closes.
        // (egui lets go of the field on Esc before this runs, so the words,
        // not the focus, decide.)
        if !self.preferences.query.is_empty()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.preferences.query.clear();
        }
        let now = ctx.input(|i| i.time);
        let marks = self.preferences.marks(now);
        let close = widgets::Dialog::new("preferences", "Preferences", fit.width)
            .at(
                egui::pos2(content.center().x, top),
                egui::Align2::CENTER_TOP,
            )
            .show(ctx, |ui| {
                let room = (content.bottom() - ui.cursor().top() - BELOW).max(LEAST_PAGE);
                if fit.folded {
                    self.preferences_folded(ui, room, &marks, midi_asked);
                } else {
                    self.preferences_beside(ui, fit, room, &marks, midi_asked);
                }
            });
        self.preferences.marked(&marks);
        if close {
            self.preferences.open = false;
        }
    }

    /// Wide: the search and the page list at the left, the page beside them.
    fn preferences_beside(
        &mut self,
        ui: &mut Ui,
        fit: Fit,
        room: f32,
        marks: &Marks,
        midi_asked: &mut Vec<midi::Action>,
    ) {
        let list = theme::size::PAGE_LIST;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(list, room), Layout::top_down(Align::Min), |ui| {
                ui.set_width(list);
                ui.set_min_height(room);
                self.preferences_search(ui);
                ui.add_space(theme::space::S);
                for page in Page::ALL {
                    let on = self.preferences.query.is_empty() && self.preferences.page == page;
                    let response = widgets::choice_row(ui, page.name(), on);
                    // Named as a page, so a screen reader (and a test) tells
                    // it from other controls that say "Audio".
                    let name = format!("{} page", page.name());
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, &name)
                    });
                    if response.on_hover_text(page.tip()).clicked() {
                        self.preferences.page = page;
                        self.preferences.query.clear();
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    self.shortcuts_link(ui);
                });
            });
            ui.separator();
            let width = fit.width - list - ui.spacing().item_spacing.x * 2.0 - theme::space::M;
            ui.vertical(|ui| {
                ui.set_width(width);
                self.preferences_body(ui, room, marks, midi_asked);
            });
        });
    }

    /// Narrow: the search, then the page menu, then the page.
    fn preferences_folded(
        &mut self,
        ui: &mut Ui,
        room: f32,
        marks: &Marks,
        midi_asked: &mut Vec<midi::Action>,
    ) {
        self.preferences_search(ui);
        ui.add_space(theme::space::S);
        ui.horizontal(|ui| {
            let names = Page::ALL.map(Page::name);
            let index = Page::ALL.iter().position(|p| *p == self.preferences.page);
            let width = (ui.available_width() - theme::size::CONTROL - theme::space::S)
                .min(theme::size::PAGE_LIST * 1.5);
            if let Some(i) = widgets::Dropdown::new("Page", self.preferences.page.name(), width)
                .show(ui, egui::Id::new("preferences page"), &names, index)
            {
                self.preferences.page = Page::ALL[i];
                self.preferences.query.clear();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.shortcuts_link(ui);
            });
        });
        ui.add_space(theme::space::S);
        let room =
            (room - theme::size::CONTROL_LARGE - theme::size::CONTROL - 2.0 * theme::space::S)
                .max(LEAST_PAGE);
        self.preferences_body(ui, room, marks, midi_asked);
    }

    /// The search field; Enter takes the first result.
    fn preferences_search(&mut self, ui: &mut Ui) {
        let window = &mut self.preferences;
        crate::browser::controls::search(ui, &mut window.query, &mut window.focus_search, SEARCH);
        let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if entered && let Some(first) = preferences::search(&window.query).first() {
            window.choose(first, ui.input(|i| i.time));
        }
    }

    /// The link to the Keyboard shortcuts window, which keeps its own.
    fn shortcuts_link(&mut self, ui: &mut Ui) {
        if widgets::raised::button(ui, "Keyboard shortcuts\u{2026}")
            .on_hover_text("Shortcuts have a window of their own")
            .clicked()
        {
            self.preferences.open = false;
            self.open_shortcuts();
        }
    }

    /// The results of the search, or the page.
    fn preferences_body(
        &mut self,
        ui: &mut Ui,
        room: f32,
        marks: &Marks,
        midi_asked: &mut Vec<midi::Action>,
    ) {
        let query = self.preferences.query.clone();
        egui::ScrollArea::vertical()
            .id_salt((
                "preferences page",
                self.preferences.page as u8,
                query.is_empty(),
            ))
            .max_height(room)
            .min_scrolled_height(room)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if query.trim().is_empty() {
                    self.preferences_page(ui, marks, midi_asked);
                } else {
                    self.preferences_results(ui, &query);
                }
            });
    }

    /// What search found: each row's name and its page; a click goes there.
    fn preferences_results(&mut self, ui: &mut Ui, query: &str) {
        let found = preferences::search(query);
        if found.is_empty() {
            ui.add_space(theme::space::M);
            widgets::hint(
                ui,
                &format!(
                    "Nothing matches \u{201c}{}\u{201d}. Try another word, such as latency or theme.",
                    query.trim()
                ),
            );
            return;
        }
        for row in found {
            let label = format!("{}   {}", row.name, row.page.name());
            let response = widgets::choice_row(ui, &label, false).on_hover_text(format!(
                "Opens the {} page at this setting",
                row.page.name()
            ));
            if response.clicked() {
                self.preferences.choose(row, ui.input(|i| i.time));
            }
        }
        ui.add_space(theme::space::S);
        widgets::hint(ui, "Enter opens the first result.");
    }
}
