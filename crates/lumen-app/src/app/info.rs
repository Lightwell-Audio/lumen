use eframe::egui::{self, Rect, RichText, Sense, Shape, Ui, vec2};

use crate::commands::{Command, Keymap, keys::modifier_names};
use crate::{App, icons, theme, widgets};

const DIVIDER: f32 = 14.0;
const ACTION_GAP: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Held {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Held {
    pub const NONE: Held = Held {
        ctrl: false,
        shift: false,
        alt: false,
    };
    pub const CTRL: Held = Held {
        ctrl: true,
        ..Held::NONE
    };
    pub const SHIFT: Held = Held {
        shift: true,
        ..Held::NONE
    };
    pub const ALT: Held = Held {
        alt: true,
        ..Held::NONE
    };
    pub const CTRL_ALT: Held = Held {
        ctrl: true,
        alt: true,
        shift: false,
    };
    pub const CTRL_SHIFT: Held = Held {
        ctrl: true,
        shift: true,
        alt: false,
    };

    pub(crate) fn of(modifiers: egui::Modifiers) -> Self {
        Self {
            ctrl: modifiers.command,
            shift: modifiers.shift,
            alt: modifiers.alt,
        }
    }

    fn names(self) -> Vec<&'static str> {
        modifier_names(self.ctrl, self.shift, self.alt)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mouse {
    Click,
    DoubleClick,
    RightClick,
    Drag,
}

impl Mouse {
    fn word(self) -> &'static str {
        match self {
            Mouse::Click => "click",
            Mouse::DoubleClick => "double-click",
            Mouse::RightClick => "right-click",
            Mouse::Drag => "drag",
        }
    }

    fn shown(self) -> &'static str {
        match self {
            Mouse::Click => "Click",
            Mouse::DoubleClick => "Double-click",
            Mouse::RightClick => "Right-click",
            Mouse::Drag => "Drag",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum How {
    Pointer(Held, Mouse),
    Key(Command),
    Caps(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
struct Action {
    how: How,
    verb: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Info {
    what: String,
    actions: Vec<Action>,
}

impl Info {
    pub(crate) fn new(what: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            actions: Vec::new(),
        }
    }

    pub(crate) fn on(mut self, held: Held, gesture: Mouse, verb: &str) -> Self {
        self.actions.push(Action {
            how: How::Pointer(held, gesture),
            verb: verb.to_owned(),
        });
        self
    }

    pub(crate) fn key(mut self, command: Command, verb: &str) -> Self {
        self.actions.push(Action {
            how: How::Key(command),
            verb: verb.to_owned(),
        });
        self
    }

    pub(crate) fn caps(mut self, caps: &'static str, verb: &str) -> Self {
        self.actions.push(Action {
            how: How::Caps(caps),
            verb: verb.to_owned(),
        });
        self
    }

    fn shown<'a>(&'a self, keymap: &'a Keymap) -> impl Iterator<Item = &'a Action> {
        self.actions.iter().filter(|a| match a.how {
            How::Key(command) => keymap.keys(command).is_some(),
            How::Pointer(..) | How::Caps(_) => true,
        })
    }

    pub(crate) fn text(&self, keymap: &Keymap) -> String {
        let actions: Vec<String> = self
            .shown(keymap)
            .map(|a| match a.how {
                How::Pointer(held, gesture) => {
                    let mut how = held.names();
                    how.push(gesture.word());
                    format!("{} to {}", how.join("+"), a.verb)
                }
                How::Key(command) => {
                    let keys = keymap.text(command).unwrap_or_default();
                    format!("{keys} {}", a.verb)
                }
                How::Caps(caps) => format!("{caps} {}", a.verb),
            })
            .collect();
        if actions.is_empty() {
            self.what.clone()
        } else if self.what.is_empty() {
            actions.join(" \u{00B7} ")
        } else {
            format!("{}: {}", self.what, actions.join(" · "))
        }
    }
}

impl Info {
    pub(crate) fn lit_text(&self, keymap: &Keymap, held: Held) -> Option<String> {
        let action = self.shown(keymap).find(|a| lit(a, keymap, held))?;
        let only = Info {
            what: String::new(),
            actions: vec![action.clone()],
        };
        Some(only.text(keymap))
    }
}

fn lit(action: &Action, keymap: &Keymap, held: Held) -> bool {
    if held == Held::NONE {
        return false;
    }
    match action.how {
        How::Pointer(h, _) => h == held,
        How::Key(command) => keymap
            .keys(command)
            .is_some_and(|k| k.modifiers == held.names()),
        How::Caps(_) => false,
    }
}

impl Info {
    pub(crate) fn computer_keyboard(readout: Option<&str>) -> Self {
        Info::new(readout.unwrap_or_default())
            .caps("A\u{2013}K", "notes")
            .caps("Z X", "octave")
            .caps("C V", "velocity")
            .caps("Esc", "leave")
    }
}

impl App {
    pub(crate) fn set_info(&mut self, info: Info) {
        self.info = Some(info);
    }

    pub(crate) fn draw_info_line(&mut self, ui: &mut Ui) {
        let current = self.info.take().or_else(|| {
            let readout = self.keyboard.readout();
            let hidden = !self.transport.keyboard_readout_shown;
            self.keyboard
                .is_on()
                .then(|| Info::computer_keyboard(hidden.then_some(readout.as_str())))
        });
        if current != self.info_shown {
            ui.ctx().request_repaint();
        }
        let shown = std::mem::replace(&mut self.info_shown, current);
        if !self.settings.appearance.info_line {
            return;
        }
        let palette = theme::p();
        egui::Panel::bottom("info line")
            .exact_size(theme::size::INFO_LINE)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(palette.app)
                    .inner_margin(egui::Margin {
                        left: theme::space::M as i8,
                        right: theme::space::M as i8,
                        top: 0,
                        bottom: 0,
                    }),
            )
            .show(ui, |ui| {
                let strip = ui.max_rect();
                ui.allocate_rect(strip, Sense::hover());
                ui.painter().hline(
                    ui.min_rect().x_range().expand(theme::space::M),
                    strip.top(),
                    egui::Stroke::new(theme::stroke::HAIRLINE, palette.border),
                );
                let text = shown
                    .as_ref()
                    .map(|i| i.text(&self.keymap))
                    .unwrap_or_default();
                let response = ui.interact(strip, ui.id().with("info"), Sense::hover());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Label,
                        true,
                        format!("Info: {text}"),
                    )
                });
                if let Some(info) = &shown {
                    let held = ui.input(|i| Held::of(i.modifiers));
                    let row_rect = Rect::from_center_size(
                        strip.center() - vec2(0.0, widgets::KEY_LIGHT_ROOM / 2.0),
                        vec2(strip.width(), widgets::caps_height(ui)),
                    );
                    let mut row = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(row_rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    row.set_clip_rect(strip.intersect(ui.clip_rect()));
                    draw(&mut row, info, &self.keymap, held);
                }
            });
    }
}

fn draw(ui: &mut Ui, info: &Info, keymap: &Keymap, held: Held) {
    let palette = theme::p();
    ui.spacing_mut().item_spacing.x = theme::space::XS;
    let mut shown = info.shown(keymap).peekable();
    if !info.what.is_empty() {
        let (name, rest) = match info.what.split_once(" \u{00B7} ") {
            Some((name, rest)) => (name, Some(rest)),
            None => (info.what.as_str(), None),
        };
        ui.label(
            RichText::new(name)
                .size(theme::size::CAPTION)
                .color(palette.text),
        );
        if let Some(rest) = rest {
            ui.label(
                RichText::new(rest)
                    .size(theme::size::CAPTION)
                    .color(palette.text_secondary),
            );
        }
        if shown.peek().is_some() {
            ui.add_space(theme::space::XS);
            let (rect, _) =
                ui.allocate_exact_size(vec2(theme::stroke::HAIRLINE, DIVIDER), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, palette.border_strong);
            ui.add_space(theme::space::XS);
        }
    }
    let right = ui.max_rect().right();
    let middle = ui.max_rect().center().y;
    let any_lit = info.shown(keymap).any(|a| lit(a, keymap, held));
    for (n, action) in shown.enumerate() {
        let gap = if n > 0 { ACTION_GAP } else { 0.0 };
        let wanted = gap + width(ui, action, keymap) + 3.0 * theme::space::S;
        if ui.cursor().left() + wanted > right {
            ui.label(
                RichText::new("\u{2026}")
                    .size(theme::size::CAPTION)
                    .color(palette.text_hint),
            );
            break;
        }
        ui.add_space(gap);
        let lit = lit(action, keymap, held);
        let opacity = ui.opacity();
        if any_lit && !lit {
            ui.multiply_opacity(theme::DIMMED);
        }
        let background = ui.painter().add(Shape::Noop);
        let left = ui.cursor().left();
        let colour = if lit {
            palette.text
        } else {
            palette.text_secondary
        };
        match action.how {
            How::Pointer(held, gesture) => {
                if held != Held::NONE {
                    widgets::modifier_caps(ui, &held.names());
                }
                mouse(ui, colour);
                ui.label(
                    RichText::new(gesture.shown())
                        .size(theme::size::CAPTION)
                        .color(colour),
                );
            }
            How::Key(command) => {
                if let Some(keys) = keymap.keys(command) {
                    keys.picture(ui);
                }
            }
            How::Caps(caps) => {
                widgets::modifier_caps(ui, &[caps]);
            }
        }
        ui.label(
            RichText::new(&action.verb)
                .size(theme::size::CAPTION)
                .color(colour),
        );
        ui.set_opacity(opacity);
        let group = Rect::from_x_y_ranges(left..=ui.min_rect().right(), middle..=middle);
        if lit {
            ui.painter().set(
                background,
                Shape::rect_filled(
                    Rect::from_center_size(
                        egui::pos2(group.center().x, middle),
                        vec2(
                            group.width() + 2.0 * theme::space::XS,
                            theme::size::INFO_LINE
                                - 2.0 * theme::space::XS
                                - widgets::KEY_LIGHT_ROOM,
                        ),
                    ),
                    theme::radius::SMALL,
                    palette.accent_muted,
                ),
            );
        }
    }
}

fn mouse(ui: &mut Ui, colour: egui::Color32) {
    let side = theme::size::CAPTION + 2.0;
    let (rect, _) = ui.allocate_exact_size(vec2(side * 0.7, side), Sense::hover());
    let square = Rect::from_center_size(rect.center(), vec2(side, side));
    for shape in icons::shapes(icons::Icon::Mouse, square, colour) {
        ui.painter().add(shape);
    }
}

fn width(ui: &Ui, action: &Action, keymap: &Keymap) -> f32 {
    let font = egui::FontId::proportional(theme::size::CAPTION);
    let text = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
    };
    let gap = theme::space::XS;
    let how = match action.how {
        How::Pointer(held, gesture) => {
            let names = held.names();
            widgets::caps_width(ui, &names, None)
                + (theme::size::CAPTION + 2.0) * 0.7
                + text(gesture.shown())
                + 3.0 * gap
        }
        How::Key(command) => keymap.keys(command).map_or(0.0, |k| {
            widgets::caps_width(ui, &k.modifiers, Some(k.key)) + gap
        }),
        How::Caps(caps) => widgets::caps_width(ui, &[caps], None) + gap,
    };
    how + text(&action.verb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_reads_as_words_and_lights_the_drag_the_keys_choose() {
        let keymap = Keymap::default();
        let info = Info::new("Clip \u{201C}kick\u{201D}")
            .on(Held::NONE, Mouse::Drag, "move")
            .on(Held::CTRL, Mouse::Drag, "copy")
            .key(Command::Duplicate, "duplicate")
            .key(Command::FitLoop, "fit the loop");
        let ctrl = Held::CTRL.names()[0];
        let duplicate = keymap.text(Command::Duplicate).expect("a default");
        assert_eq!(
            info.text(&keymap),
            format!(
                "Clip \u{201C}kick\u{201D}: drag to move \u{00B7} {ctrl}+drag to copy \u{00B7} {duplicate} duplicate"
            ),
            "Fit loop has no shortcut, so it is left out"
        );
        let actions: Vec<&Action> = info.shown(&keymap).collect();
        let lit = |n: usize, held| lit(actions[n], &keymap, held);
        assert!(!lit(0, Held::NONE), "nothing lights with no key held");
        assert!(!lit(1, Held::NONE));
        assert!(lit(1, Held::CTRL));
        assert!(!lit(0, Held::CTRL), "a plain drag dims while Ctrl is held");
        assert!(!lit(1, Held::CTRL_ALT), "exactly the keys held");
        assert!(lit(2, Held::CTRL), "Ctrl lights the Ctrl shortcut too");
        assert!(!lit(2, Held::SHIFT));
    }
}
