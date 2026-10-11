//! Changing shortcuts (M3 slice 7c; the owner's E2 with E3). The Keyboard
//! shortcuts window shows a keyboard lit by area over the list of every
//! command: click a shortcut and press new keys. A clash is shown on the
//! row, with Reassign or Cancel. The palette records the same way (Ctrl+
//! Enter on a row, or a click on its keycaps). Changes are saved in the
//! settings by command id; only what differs from the defaults is kept.

use eframe::egui::{self, Align2, FontId, Key, Rect, RichText, Sense, Stroke, Vec2, pos2, vec2};
use eframe::emath::GuiRounding as _;

use crate::commands::{ALL, Area, Chord, Command};
use crate::widgets::raised;
use crate::{App, theme, widgets};

/// The window's width.
const WIDTH: f32 = 820.0;
/// The keyboard: the main block is 15 key widths, the arrows and the keys
/// above them 3, with half a key between.
const KEYBOARD_UNITS: f32 = 18.5;
/// The space between two keys on the map, and the letters' inset in a key.
const KEY_GAP: f32 = 3.0;
const KEY_INSET: f32 = 4.0;
/// The light under a command's key on the map: as in the keycaps (K5), its
/// halo and its height.
const KEY_LIGHT_HEIGHT: f32 = 2.0;
const KEY_LIGHT_HALO: f32 = 1.5;
const KEY_LIGHT_HALO_ALPHA: f32 = 0.3;
/// An area's light beside its name: in the layer row and in a heading.
const AREA_LIGHT: Vec2 = vec2(8.0, 3.0);
const AREA_DOT: f32 = 8.0;

/// A chord being recorded for a command, in the window or the palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rebind {
    pub command: Command,
    /// The chord replaced; `None` adds one.
    pub slot: Option<usize>,
    /// The chord pressed, and the command that has it, waiting for
    /// Reassign or Cancel.
    pub clash: Option<(Chord, Command)>,
}

/// The window between frames (not saved).
#[derive(Debug, Default)]
pub(crate) struct Shortcuts {
    pub open: bool,
    query: String,
    /// Layers chosen on the map (Ctrl, Shift, Alt).
    layer: (bool, bool, bool),
    /// The chord of the key last clicked on the map.
    picked: Option<Chord>,
}

/// A key on the map: what it is (none for a modifier), its label and its
/// width in keys.
struct MapKey {
    key: Option<Key>,
    label: &'static str,
    width: f32,
}

const fn k(key: Key, label: &'static str) -> MapKey {
    MapKey {
        key: Some(key),
        label,
        width: 1.0,
    }
}

const fn wide(key: Key, label: &'static str, width: f32) -> MapKey {
    MapKey {
        key: Some(key),
        label,
        width,
    }
}

const fn modifier(label: &'static str, width: f32) -> MapKey {
    MapKey {
        key: None,
        label,
        width,
    }
}

/// The main block, row by row.
const MAIN: [&[MapKey]; 6] = [
    &[
        k(Key::Escape, "Esc"),
        k(Key::F1, "F1"),
        k(Key::F2, "F2"),
        k(Key::F3, "F3"),
        k(Key::F4, "F4"),
        k(Key::F5, "F5"),
        k(Key::F6, "F6"),
        k(Key::F7, "F7"),
        k(Key::F8, "F8"),
        k(Key::F9, "F9"),
        k(Key::F10, "F10"),
        k(Key::F11, "F11"),
        k(Key::F12, "F12"),
    ],
    &[
        k(Key::Backtick, "`"),
        k(Key::Num1, "1"),
        k(Key::Num2, "2"),
        k(Key::Num3, "3"),
        k(Key::Num4, "4"),
        k(Key::Num5, "5"),
        k(Key::Num6, "6"),
        k(Key::Num7, "7"),
        k(Key::Num8, "8"),
        k(Key::Num9, "9"),
        k(Key::Num0, "0"),
        k(Key::Minus, "-"),
        k(Key::Equals, "="),
        wide(Key::Backspace, "Backspace", 2.0),
    ],
    &[
        wide(Key::Tab, "Tab", 1.5),
        k(Key::Q, "Q"),
        k(Key::W, "W"),
        k(Key::E, "E"),
        k(Key::R, "R"),
        k(Key::T, "T"),
        k(Key::Y, "Y"),
        k(Key::U, "U"),
        k(Key::I, "I"),
        k(Key::O, "O"),
        k(Key::P, "P"),
        k(Key::OpenBracket, "["),
        k(Key::CloseBracket, "]"),
        wide(Key::Backslash, "\\", 1.5),
    ],
    &[
        modifier("Caps", 1.75),
        k(Key::A, "A"),
        k(Key::S, "S"),
        k(Key::D, "D"),
        k(Key::F, "F"),
        k(Key::G, "G"),
        k(Key::H, "H"),
        k(Key::J, "J"),
        k(Key::K, "K"),
        k(Key::L, "L"),
        k(Key::Semicolon, ";"),
        k(Key::Quote, "'"),
        wide(Key::Enter, "Enter", 2.25),
    ],
    &[
        modifier("Shift", 2.25),
        k(Key::Z, "Z"),
        k(Key::X, "X"),
        k(Key::C, "C"),
        k(Key::V, "V"),
        k(Key::B, "B"),
        k(Key::N, "N"),
        k(Key::M, "M"),
        k(Key::Comma, ","),
        k(Key::Period, "."),
        k(Key::Slash, "/"),
        modifier("Shift", 2.75),
    ],
    &[
        modifier("Ctrl", 1.5),
        modifier("Alt", 1.5),
        wide(Key::Space, "Space", 9.0),
        modifier("Alt", 1.5),
        modifier("Ctrl", 1.5),
    ],
];

/// The keys right of the main block: two rows above the arrows.
const SIDE: [&[MapKey]; 2] = [
    &[
        k(Key::Insert, "Ins"),
        k(Key::Home, "Home"),
        k(Key::PageUp, "PgUp"),
    ],
    &[
        k(Key::Delete, "Del"),
        k(Key::End, "End"),
        k(Key::PageDown, "PgDn"),
    ],
];

impl Shortcuts {
    /// The chord for `key` on the layer showing now.
    fn chord(&self, held: egui::Modifiers, key: Key) -> Chord {
        let (ctrl, shift, alt) = self.layer;
        Chord {
            ctrl: ctrl || held.command,
            shift: shift || held.shift,
            alt: alt || held.alt,
            key,
        }
    }
}

impl App {
    /// Opens the Keyboard shortcuts window.
    pub(crate) fn open_shortcuts(&mut self) {
        self.shortcuts.open = true;
    }

    /// Starts recording a chord for `command`, replacing chord `slot`
    /// (`None` adds one).
    pub(crate) fn start_rebind(&mut self, command: Command, slot: Option<usize>) {
        self.rebind = Some(Rebind {
            command,
            slot,
            clash: None,
        });
    }

    /// The keymap changed: keep it for saving.
    fn keymap_changed(&mut self) {
        self.settings.commands.shortcuts = self.keymap.saved();
        self.settings_changed = Some(std::time::Instant::now());
    }

    /// While a chord is recorded, the next key pressed is it (Esc
    /// cancels); with a clash waiting, Enter reassigns and Esc cancels.
    /// Every key and letter typed this frame is taken, so nothing else
    /// runs or types it. Runs before the command keys.
    pub(crate) fn capture_chord(&mut self, ctx: &egui::Context) {
        let Some(rebind) = self.rebind else {
            return;
        };
        let pressed: Vec<Chord> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match *event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some(Chord::pressed(modifiers, key)),
                    _ => None,
                })
                .collect()
        });
        ctx.input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
        });
        let Some(chord) = pressed.first().copied() else {
            return;
        };
        let plain = |key| chord == crate::commands::keys::plain(key);
        match rebind.clash {
            Some(_) if plain(Key::Enter) => self.reassign_clash(),
            _ if plain(Key::Escape) => self.rebind = None,
            Some(_) => {}
            None => self.record(chord),
        }
        ctx.request_repaint();
    }

    /// Records `chord` for the command being rebound, or notes the clash.
    fn record(&mut self, chord: Chord) {
        let Some(rebind) = self.rebind.as_mut() else {
            return;
        };
        match self.keymap.set(rebind.command, rebind.slot, chord) {
            Ok(()) => {
                self.rebind = None;
                self.keymap_changed();
            }
            Err(other) => rebind.clash = Some((chord, other)),
        }
    }

    /// Takes the clashing chord for the command being rebound.
    pub(crate) fn reassign_clash(&mut self) {
        if let Some(Rebind {
            command,
            slot,
            clash: Some((chord, _)),
        }) = self.rebind.take()
        {
            self.keymap.reassign(command, slot, chord);
            self.keymap_changed();
        }
    }

    /// The Keyboard shortcuts window, when open.
    pub(crate) fn shortcuts_window(&mut self, ctx: &egui::Context) {
        if !self.shortcuts.open {
            return;
        }
        let at = pos2(
            ctx.content_rect().center().x,
            ctx.content_rect().top() + 48.0,
        );
        // Esc while recording cancels the recording, not the window.
        let dialog = widgets::Dialog::new("shortcuts", "Keyboard shortcuts", WIDTH)
            .at(at, Align2::CENTER_TOP);
        let recording = self.rebind.is_some();
        let close = dialog.show(ctx, |ui| {
            self.layer_chips(ui);
            self.keyboard_map(ui);
            ui.add_space(theme::space::S);
            self.shortcut_list(ui);
        });
        if close && !recording {
            self.shortcuts.open = false;
            self.rebind = None;
        }
    }

    /// The layer chips (Ctrl, Shift, Alt) and the areas' lights.
    fn layer_chips(&mut self, ui: &mut egui::Ui) {
        let palette = *theme::p();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Layer").color(palette.text_hint));
            let layer = &mut self.shortcuts.layer;
            for (on, name) in [
                (&mut layer.0, "Ctrl"),
                (&mut layer.1, "Shift"),
                (&mut layer.2, "Alt"),
            ] {
                crate::widgets::toggle_button(ui, on, name).on_hover_text(format!(
                    "Show what {name} with each key does (or hold {name})"
                ));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for area in AREAS.iter().rev() {
                    ui.label(
                        RichText::new(area.name())
                            .size(theme::size::CAPTION)
                            .color(palette.text_hint),
                    );
                    let (rect, _) = ui.allocate_exact_size(AREA_LIGHT, Sense::hover());
                    ui.painter()
                        .rect_filled(rect, AREA_LIGHT.y / 2.0, area.light());
                }
            });
        });
    }

    /// The keyboard: each key that runs a command on this layer is
    /// outlined, with the command's name on it and its area's light under
    /// it. Clicking a key searches the list for its chord.
    fn keyboard_map(&mut self, ui: &mut egui::Ui) {
        let palette = *theme::p();
        let held = ui.input(|i| i.modifiers);
        let width = ui.available_width();
        let unit = width / KEYBOARD_UNITS;
        let gap = KEY_GAP;
        let height = unit * 0.95;
        let (area, _) = ui.allocate_exact_size(vec2(width, height * 6.0), Sense::hover());
        let ppp = ui.pixels_per_point();
        let mut clicked = None;
        let mut draw = |ui: &mut egui::Ui, map_key: &MapKey, rect: Rect| {
            let rect = rect.shrink(gap / 2.0).round_to_pixels(ppp);
            let Some(key) = map_key.key else {
                // A modifier: lit while its layer shows.
                let (ctrl, shift, alt) = self.shortcuts.layer;
                let on = match map_key.label {
                    "Ctrl" => ctrl || held.command,
                    "Shift" => shift || held.shift,
                    "Alt" => alt || held.alt,
                    _ => false,
                };
                let (fill, text) = if on {
                    (palette.accent_muted, palette.accent)
                } else {
                    (palette.raised, palette.text_hint)
                };
                ui.painter().rect_filled(rect, theme::radius::SMALL, fill);
                if on {
                    // One accent hairline, as every control that is on has.
                    ui.painter().rect_stroke(
                        rect,
                        theme::radius::SMALL,
                        Stroke::new(theme::stroke::HAIRLINE, theme::lit(theme::Tone::On).edge),
                        egui::StrokeKind::Inside,
                    );
                }
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    map_key.label,
                    FontId::proportional(theme::size::CAPTION),
                    text,
                );
                return;
            };
            let chord = self.shortcuts.chord(held, key);
            let command = self.keymap.command_for(chord);
            let id = ui.id().with(("map key", map_key.label));
            let response = ui.interact(rect, id, Sense::click());
            let name = match command {
                Some(c) => format!("{}: {}", chord.text(), c.name()),
                None => format!("{}: free", chord.text()),
            };
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
            if response.clicked() {
                clicked = Some(chord);
            }
            let painter = ui.painter();
            match command {
                Some(c) => {
                    painter.rect_filled(rect, theme::radius::SMALL, palette.overlay);
                    painter.rect_stroke(
                        rect,
                        theme::radius::SMALL,
                        Stroke::new(theme::stroke::HAIRLINE, palette.border_strong),
                        egui::StrokeKind::Inside,
                    );
                    let light = c.info().area.light();
                    let bar = Rect::from_min_max(
                        pos2(rect.left() + KEY_INSET, rect.bottom() - KEY_INSET),
                        pos2(
                            rect.right() - KEY_INSET,
                            rect.bottom() - KEY_INSET + KEY_LIGHT_HEIGHT,
                        ),
                    );
                    painter.rect_filled(
                        bar.expand(KEY_LIGHT_HALO),
                        KEY_LIGHT_HEIGHT,
                        light.gamma_multiply(KEY_LIGHT_HALO_ALPHA),
                    );
                    painter.rect_filled(bar, KEY_LIGHT_HEIGHT / 2.0, light);
                    let room = rect.width() - 2.0 * KEY_INSET;
                    let label = elided(painter, map_key.label, palette.text, room);
                    painter.galley(
                        rect.left_top() + vec2(KEY_INSET, theme::space::XXS),
                        label,
                        palette.text,
                    );
                    let name = elided(painter, c.name(), palette.text_secondary, room);
                    let at = pos2(
                        rect.left() + KEY_INSET,
                        rect.bottom() - KEY_INSET - theme::space::XXS - name.size().y,
                    );
                    painter.galley(at, name, palette.text_secondary);
                }
                None => {
                    painter.rect_filled(rect, theme::radius::SMALL, palette.raised);
                    painter.text(
                        rect.left_top() + vec2(KEY_INSET, theme::space::XXS),
                        Align2::LEFT_TOP,
                        map_key.label,
                        FontId::proportional(theme::size::CAPTION),
                        palette.text_hint,
                    );
                }
            }
            // Hover is Ember, washed over the key; the accent ring is the
            // keyboard's focus.
            if response.hovered() {
                painter.rect_filled(rect, theme::radius::SMALL, palette.ember);
            }
            if response.has_focus() {
                raised::paint_focus(painter, rect, theme::radius::SMALL);
            }
        };
        for (row, keys) in MAIN.iter().enumerate() {
            let mut x = area.left();
            for map_key in keys.iter() {
                let rect = Rect::from_min_size(
                    pos2(x, area.top() + row as f32 * height),
                    vec2(map_key.width * unit, height),
                );
                draw(ui, map_key, rect);
                x += map_key.width * unit;
            }
        }
        let side = area.left() + 15.5 * unit;
        for (row, keys) in SIDE.iter().enumerate() {
            for (i, map_key) in keys.iter().enumerate() {
                let rect = Rect::from_min_size(
                    pos2(
                        side + i as f32 * unit,
                        area.top() + (row + 1) as f32 * height,
                    ),
                    vec2(unit, height),
                );
                draw(ui, map_key, rect);
            }
        }
        let arrow = |col: f32, row: f32| {
            Rect::from_min_size(
                pos2(side + col * unit, area.top() + row * height),
                vec2(unit, height),
            )
        };
        draw(ui, &k(Key::ArrowUp, "\u{2191}"), arrow(1.0, 4.0));
        draw(ui, &k(Key::ArrowLeft, "\u{2190}"), arrow(0.0, 5.0));
        draw(ui, &k(Key::ArrowDown, "\u{2193}"), arrow(1.0, 5.0));
        draw(ui, &k(Key::ArrowRight, "\u{2192}"), arrow(2.0, 5.0));
        if let Some(chord) = clicked {
            self.shortcuts.query = chord.text();
            self.shortcuts.picked = Some(chord);
        }
    }

    /// The search field and every command with its shortcuts, by area.
    fn shortcut_list(&mut self, ui: &mut egui::Ui) {
        let palette = *theme::p();
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.shortcuts.query)
                    .hint_text("Search commands or keys")
                    .desired_width(ui.available_width() - 220.0),
            );
            field.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search shortcuts")
            });
            ui.label(RichText::new("Keymap: Ableton").color(palette.text_hint));
            if ui
                .add_enabled(!self.keymap.saved().is_empty(), |ui: &mut egui::Ui| {
                    raised::button(ui, "Reset all")
                })
                .on_hover_text("Every shortcut back to its default")
                .clicked()
            {
                self.keymap = crate::commands::Keymap::default();
                self.rebind = None;
                self.keymap_changed();
            }
        });
        ui.add_space(theme::space::XS);
        let query = self.shortcuts.query.trim().to_lowercase();
        let matches = |info: &crate::commands::Info, keymap: &crate::commands::Keymap| {
            query.is_empty()
                || info.name.to_lowercase().contains(&query)
                || keymap
                    .chords(info.command)
                    .iter()
                    .any(|c| c.text().to_lowercase() == query)
        };
        let shown: Vec<Command> = ALL
            .iter()
            .filter(|info| matches(info, &self.keymap))
            .map(|info| info.command)
            .collect();
        let room = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 60.0).max(120.0);
        egui::ScrollArea::vertical()
            .max_height(room)
            .min_scrolled_height(room)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if shown.is_empty() {
                    ui.add_space(theme::space::S);
                    let picked = self
                        .shortcuts
                        .picked
                        .filter(|c| c.text() == self.shortcuts.query);
                    let text = match picked {
                        Some(chord) => format!(
                            "{} is free. Click a command's shortcut and press {} to give it to the command.",
                            chord.text(),
                            chord.text()
                        ),
                        None => format!(
                            "No command matches \u{201C}{}\u{201D}.",
                            self.shortcuts.query.trim()
                        ),
                    };
                    ui.add(egui::Label::new(RichText::new(text).color(palette.text_hint)).wrap());
                }
                let mut area = None;
                for command in shown {
                    let info = command.info();
                    if area != Some(info.area) {
                        area = Some(info.area);
                        area_heading(ui, info.area);
                    }
                    self.shortcut_row(ui, command);
                }
            });
    }

    fn shortcut_row(&mut self, ui: &mut egui::Ui, command: Command) {
        let palette = *theme::p();
        let rebind = self.rebind.filter(|r| r.command == command);
        ui.horizontal(|ui| {
            ui.set_min_height(theme::size::CONTROL + theme::space::XS);
            ui.label(command.name());
            if !self.keymap.is_default(command) {
                ui.label(
                    RichText::new("changed")
                        .size(theme::size::CAPTION)
                        .color(palette.text_hint),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.keymap.is_default(command)
                    && raised::button(ui, "\u{21BA}")
                        .on_hover_text("Back to the default")
                        .clicked()
                {
                    self.keymap.reset(command);
                    self.rebind = None;
                    self.keymap_changed();
                }
                if let Some(rebind) = rebind {
                    // A clash waiting is answered on its own line below.
                    if rebind.clash.is_some() {
                        return;
                    }
                    if raised::button(ui, "Cancel").clicked() {
                        self.rebind = None;
                    }
                    if let Some(slot) = rebind.slot
                        && rebind.clash.is_none()
                        && raised::button(ui, "Remove")
                            .on_hover_text("Take this shortcut away")
                            .clicked()
                    {
                        self.keymap.remove(command, slot);
                        self.rebind = None;
                        self.keymap_changed();
                    }
                    ui.label(RichText::new("Press keys\u{2026}").color(palette.accent));
                    return;
                }
                let add = raised::button(ui, "+").on_hover_text("Add a shortcut");
                add.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        format!("Add a shortcut to {}", command.name()),
                    )
                });
                if add.clicked() {
                    self.start_rebind(command, None);
                }
                let chords = self.keymap.chords(command).to_vec();
                for (slot, chord) in chords.iter().enumerate().rev() {
                    let caps = widgets::keycaps(
                        ui,
                        &chord.modifier_names(),
                        chord.key_name(),
                        command.info().area.light(),
                        false,
                    );
                    let caps = caps.on_hover_text("Click, then press new keys");
                    caps.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("Change {} shortcut {}", command.name(), chord.text()),
                        )
                    });
                    if caps.clicked() {
                        self.start_rebind(command, Some(slot));
                    }
                }
            });
        });
        if let Some(Rebind {
            clash: Some((chord, other)),
            ..
        }) = rebind
        {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} is used by {}.", chord.text(), other.name()))
                        .color(palette.warning),
                );
                if raised::button(ui, "Reassign").clicked() {
                    self.reassign_clash();
                }
                if raised::button(ui, "Cancel").clicked() {
                    self.rebind = None;
                }
            });
        }
    }
}

/// `text` on one line in `width`, ending in "…" when it does not fit.
fn elided(
    painter: &egui::Painter,
    text: &str,
    colour: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        FontId::proportional(theme::size::CAPTION),
        colour,
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    painter.layout_job(job)
}

/// Every area, in the table's order.
const AREAS: [Area; 10] = [
    Area::Transport,
    Area::Edit,
    Area::Clip,
    Area::Track,
    Area::Mixer,
    Area::View,
    Area::Grid,
    Area::Locators,
    Area::Midi,
    Area::File,
];

/// A group's heading in the list: its light and its name.
fn area_heading(ui: &mut egui::Ui, area: Area) {
    let palette = theme::p();
    ui.add_space(theme::space::XS);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(AREA_DOT), Sense::hover());
        ui.painter().rect_filled(
            rect.shrink(theme::stroke::HAIRLINE),
            theme::radius::SMALL / 2,
            area.light(),
        );
        ui.label(
            RichText::new(area.name())
                .size(theme::size::CAPTION)
                .color(palette.text_hint),
        );
    });
}
