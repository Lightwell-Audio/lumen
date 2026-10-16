//! The browser pane: its title, tabs and Devices tab, and what the app does
//! with what the person did in it.

use std::time::Instant;

use eframe::egui::{self, Key, Modifiers, Rect, pos2, vec2};
use lumen_plugin_catalog::catalog::Kind;
use lumen_plugin_catalog::scan::Mode;

use super::controls;
use super::list::{self, Hover, Input, Out};
use super::rows::{self, Row, Rows};
use super::view::{Chip, Selection};
use crate::app::info::{Held, Info, Mouse};
use crate::app::panes::{self, Pane};
use crate::app::projects::FileDialog;
use crate::{App, fonts, theme, widgets};

impl App {
    /// Draws the browser pane in `ui` (the panel's whole area).
    pub(crate) fn browser_panel(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        self.panes.anchor(ui, Pane::Browser, rect);
        // Its arrows are the list's keys while the pane has the keyboard.
        panes::keep_arrows(ui.ctx(), Pane::Browser);

        // Its title in the display face, inset like the window titles.
        let inset = vec2(theme::space::M, theme::space::M);
        let title = ui.painter().text(
            rect.left_top() + inset,
            egui::Align2::LEFT_TOP,
            "Browser",
            fonts::display(theme::size::HEADING),
            theme::p().text,
        );
        let body = Rect::from_min_max(
            pos2(
                rect.left() + theme::space::S,
                title.bottom() + theme::space::S,
            ),
            pos2(rect.right() - theme::space::S, rect.bottom()),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        child.set_clip_rect(body.intersect(ui.clip_rect()));
        self.browser_body(&mut child, body);
        ui.advance_cursor_after_rect(rect);
    }

    fn browser_body(&mut self, ui: &mut egui::Ui, body: Rect) {
        let ctx = ui.ctx().clone();
        ui.spacing_mut().item_spacing = vec2(theme::space::XS, theme::space::XS);

        // The controls sit 12 pt in from the pane's edge, in line with the
        // title; the list keeps its own 8 pt (its rows add their own room).
        // The side margin also leaves the focus rings whole at the edges.
        let side = (theme::space::M - theme::space::S) as i8;
        let rows = egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(side, 0))
            .show(ui, |ui| self.browser_controls(ui, &ctx))
            .inner;
        ui.add_space(theme::space::XS);

        let favorites = self.settings.plugins.favorite_set();
        let area = Rect::from_min_max(pos2(body.left(), ui.cursor().top()), body.max);
        let moved = self.browser_keys(&ctx, &rows);
        let lonely = self.no_plugins_yet() && !self.browser.view.is_filtering();
        let (list_area, footer) = if lonely {
            let split = (area.bottom() - FOOTER).max(area.top());
            (
                Rect::from_min_max(area.min, pos2(area.right(), split)),
                Some(Rect::from_min_max(pos2(area.left(), split), area.max)),
            )
        } else {
            (area, None)
        };
        if rows.list.is_empty() && self.browser.view.is_filtering() {
            self.no_match(ui, list_area);
        } else {
            let out = list::show(
                ui,
                list_area,
                &Input {
                    catalog: self.plugins.catalog(),
                    rows: &rows,
                    selected: self.browser.view.selected.as_ref(),
                    favorites: &favorites,
                    flat: self.browser.view.chip == Chip::Recent,
                    scroll_to_selected: moved,
                },
            );
            self.apply_list(&ctx, out);
        }
        if let Some(footer) = footer {
            self.first_run_card(ui, footer);
        }
    }

    /// The tabs, search, type switch, chips and scan line. Returns the rows
    /// the filters leave.
    fn browser_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Rows {
        // Samples and projects get their tabs when they are built. The tab's
        // word lines up with the title above it.
        let tabs = [widgets::Tab {
            label: "Devices",
            disabled_because: None,
            tip: "Instruments and effects",
        }];
        widgets::tabs_flush(ui, egui::Id::new("browser tabs"), 0, &tabs);
        ui.add_space(theme::space::XS);

        // The filters, each group a full step from the next.
        let searched = controls::search(
            ui,
            &mut self.browser.view.text,
            &mut self.browser.focus_search,
        );
        if searched.escaped {
            if self.browser.view.text.is_empty() {
                // Nothing to clear: give the keyboard back to the pane.
                panes::focus(ctx, Pane::Browser);
            } else {
                self.browser.view.text.clear();
            }
        }
        ui.add_space(theme::space::S);
        if let Some(kind) = controls::type_filter(ui, self.browser.view.kind) {
            self.browser.view.kind = kind;
            // A chip of a category the other kind lacks would show nothing.
            if matches!(self.browser.view.chip, Chip::Category(_)) {
                self.browser.view.chip = Chip::All;
            }
        }
        ui.add_space(theme::space::S);
        let favorites = self.settings.plugins.favorite_set();
        let mut rows = rows::rows(
            self.plugins.catalog(),
            &self.browser.view,
            &favorites,
            &self.settings.plugins.recents,
        );
        if let Some(chip) = controls::chips(ui, &self.browser.view.chip, &rows.chips) {
            self.browser.view.chip = chip;
            // A chip changes what is listed.
            rows = rows::rows(
                self.plugins.catalog(),
                &self.browser.view,
                &favorites,
                &self.settings.plugins.recents,
            );
        }
        ui.add_space(theme::space::XS);

        let scan = controls::scan_line(ui, &self.plugins);
        if scan.rescan {
            self.plugins
                .rescan(Mode::Changed, &self.settings.plugins, ctx);
        }
        if scan.cancel {
            self.plugins.cancel();
        }
        if scan.dismiss_note {
            self.plugins.dismiss_note();
        }
        rows
    }

    /// Nothing matches the words and filters.
    fn no_match(&mut self, ui: &mut egui::Ui, area: Rect) {
        let state = widgets::EmptyState {
            icon: None,
            icon_size: theme::space::L,
            title: "No devices match",
            hint: "Try other words, or clear the filters",
            action: Some(widgets::EmptyAction {
                label: "Clear filters",
                keys: None,
                tip: "Show every device again",
            }),
        };
        if widgets::empty_state(ui, area, &state) {
            self.browser.view.clear_filters();
        }
    }

    /// No plugin has been found: where Lumen looked, and how to look elsewhere.
    fn first_run_card(&mut self, ui: &mut egui::Ui, area: Rect) {
        // A scan under way or a failed one says its own piece above.
        if self.plugins.busy() || self.plugins.is_off() {
            return;
        }
        let state = widgets::EmptyState {
            icon: None,
            icon_size: theme::space::L,
            title: "No plugins found yet",
            hint: "Lumen looks in the standard VST3 folders. If yours are elsewhere, add the folder",
            action: Some(widgets::EmptyAction {
                label: "Scan now",
                keys: None,
                tip: "Look for plugins in the standard folders and your own",
            }),
        };
        if widgets::empty_state(ui, area, &state) {
            let ctx = ui.ctx().clone();
            self.plugins.rescan(Mode::All, &self.settings.plugins, &ctx);
        }
    }

    /// Whether the catalog holds nothing but Lumen's own devices and nothing
    /// is blocked.
    fn no_plugins_yet(&self) -> bool {
        let catalog = self.plugins.catalog();
        catalog.blocked().is_empty()
            && catalog
                .devices()
                .iter()
                .all(|d| d.source == lumen_plugin_catalog::catalog::Source::Builtin)
    }

    /// The keys of the list while the pane has the keyboard: the arrows
    /// move the selection, Left and Right fold, Enter chooses. True if the
    /// selection moved (to be scrolled into view).
    fn browser_keys(&mut self, ctx: &egui::Context, rows: &Rows) -> bool {
        if panes::focused(ctx) != Some(Pane::Browser) || ctx.text_edit_focused() {
            return false;
        }
        let key = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
        let catalog = self.plugins.catalog();
        let view = &mut self.browser.view;
        let mut chosen = None;
        let from = view.selected.clone();
        let mut to = None;
        if key(Key::ArrowDown) {
            to = rows::neighbour(catalog, &rows.list, from.as_ref(), 1);
        } else if key(Key::ArrowUp) {
            to = rows::neighbour(catalog, &rows.list, from.as_ref(), -1);
        } else if key(Key::Home) {
            to = rows::neighbour(catalog, &rows.list, None, 1);
        } else if key(Key::End) {
            to = rows::neighbour(catalog, &rows.list, None, -1);
        } else if key(Key::ArrowLeft) {
            match &from {
                Some(Selection::Vendor(name)) if !is_folded(rows, name) => {
                    view.folded.insert(name.clone());
                }
                Some(Selection::Device(id)) => {
                    // Up to the device's own header.
                    to = catalog
                        .find(id)
                        .map(|d| Selection::Vendor(d.vendor.to_lowercase()))
                        .filter(|s| vendor_listed(rows, s));
                }
                _ => {}
            }
        } else if key(Key::ArrowRight) {
            match &from {
                Some(Selection::Vendor(name)) if is_folded(rows, name) => {
                    view.folded.remove(name);
                }
                Some(Selection::Vendor(_)) => {
                    to = rows::neighbour(catalog, &rows.list, from.as_ref(), 1);
                }
                _ => {}
            }
        } else if key(Key::Enter) {
            match &from {
                Some(Selection::Vendor(name)) => {
                    if !view.folded.remove(name) {
                        view.folded.insert(name.clone());
                    }
                }
                Some(Selection::BlockedHeader) => view.blocked_folded = !view.blocked_folded,
                Some(Selection::Device(id)) => chosen = Some(id.clone()),
                _ => {}
            }
        }
        if let Some(id) = chosen {
            self.choose(ctx, &id);
        }
        match to {
            Some(next) if Some(&next) != from.as_ref() => {
                self.browser.view.selected = Some(next);
                true
            }
            _ => false,
        }
    }

    /// Applies what the pointer did in the list.
    fn apply_list(&mut self, ctx: &egui::Context, out: Out) {
        if let Some(selection) = out.select {
            self.browser.view.selected = Some(selection);
            // A click in the list is the keyboard moving there too.
            panes::focus(ctx, Pane::Browser);
        }
        if let Some(name) = out.toggle_fold {
            self.browser.view.toggle_fold(&name);
        }
        if out.toggle_blocked {
            self.browser.view.blocked_folded = !self.browser.view.blocked_folded;
        }
        if let Some(id) = out.favorite {
            self.toggle_favorite(&id);
        }
        if let Some(id) = out.choose {
            self.choose(ctx, &id);
        }
        if let Some(module) = out.retry {
            self.plugins.retry(&module, &self.settings.plugins, ctx);
        }
        if let Some(path) = out.reveal {
            crate::crash::reveal(&path);
        }
        if let Some(text) = out.copy {
            ctx.copy_text(text);
        }
        if let Some(hover) = out.hover {
            let info = self.browser_info(hover);
            self.set_info(info);
        }
    }

    /// Looks for plugins again (the commands, and the Rescan button's twin).
    pub(crate) fn rescan_plugins(&mut self, ctx: &egui::Context, mode: Mode) {
        self.plugins.rescan(mode, &self.settings.plugins, ctx);
    }

    /// Asks which folder to search besides the standard ones.
    pub(crate) fn pick_plugin_folder(&mut self, ctx: &egui::Context) {
        let owner = self.owner;
        let show = move || {
            let mut dialog = rfd::FileDialog::new().set_title("Add a plugin folder");
            if let Some(owner) = &owner {
                dialog = dialog.set_parent(owner);
            }
            FileDialog::PluginFolder(dialog.pick_folder())
        };
        self.file_dialog(ctx, show, |paths| {
            FileDialog::PluginFolder(paths.into_iter().next())
        });
    }

    /// Remembers `folder` as a place to look, and looks there.
    pub(crate) fn add_plugin_folder(&mut self, ctx: &egui::Context, folder: &std::path::Path) {
        if self.settings.plugins.add_folder(folder) {
            self.settings_changed = Some(Instant::now());
            self.toast = Some((
                format!("Searching {} for plugins", folder.display()),
                Instant::now(),
            ));
        } else {
            self.toast = Some((
                format!("{} is already searched", folder.display()),
                Instant::now(),
            ));
        }
        self.plugins
            .rescan(Mode::Changed, &self.settings.plugins, ctx);
    }

    /// Stars or unstars a device.
    pub(crate) fn toggle_favorite(&mut self, id: &str) {
        let now = self.settings.plugins.toggle_favorite(id);
        self.settings_changed = Some(Instant::now());
        let name = self
            .plugins
            .catalog()
            .find(id)
            .map_or("Device", |d| d.name.as_str())
            .to_owned();
        self.toast = Some((
            if now {
                format!("{name} added to favorites")
            } else {
                format!("{name} removed from favorites")
            },
            Instant::now(),
        ));
    }

    /// Double-click or Enter on a device. Putting it on a track is the next
    /// slice; for now the choice is remembered as a recent and said so.
    fn choose(&mut self, _ctx: &egui::Context, id: &str) {
        let Some(device) = self.plugins.catalog().find(id) else {
            return;
        };
        let name = device.name.clone();
        self.settings.plugins.touch(id);
        self.settings_changed = Some(Instant::now());
        self.toast = Some((
            format!("{name}: adding devices to tracks arrives in the next slice"),
            Instant::now(),
        ));
    }

    /// The info line for what the pointer is over in the list.
    fn browser_info(&self, hover: Hover) -> Info {
        let catalog = self.plugins.catalog();
        match hover {
            Hover::Device(i) => {
                let d = &catalog.devices()[i];
                let kind = match d.kind {
                    Kind::Instrument => "Instrument",
                    Kind::Effect => "Effect",
                };
                Info::new(format!("{} · {} · {kind}", d.name, d.vendor))
                    .on(Held::NONE, Mouse::DoubleClick, "choose")
                    .on(Held::NONE, Mouse::RightClick, "more")
            }
            Hover::Vendor(name) => Info::new(name).on(Held::NONE, Mouse::Click, "fold or open"),
            Hover::Blocked(i) => Info::new(format!(
                "{} is blocked: it will not be loaded",
                catalog.blocked()[i].name
            ))
            .on(Held::NONE, Mouse::Click, "see why")
            .on(Held::NONE, Mouse::RightClick, "retry"),
        }
    }
}

/// The room the "no plugins" card takes under the built-in devices.
const FOOTER: f32 = 150.0;

fn is_folded(rows: &Rows, vendor: &str) -> bool {
    rows.list.iter().any(
        |r| matches!(r, Row::Vendor { name, folded, .. } if name.to_lowercase() == vendor && *folded),
    )
}

fn vendor_listed(rows: &Rows, selection: &Selection) -> bool {
    rows.list.iter().any(|r| {
        matches!((r, selection), (Row::Vendor { name, .. }, Selection::Vendor(s)) if &name.to_lowercase() == s)
    })
}
