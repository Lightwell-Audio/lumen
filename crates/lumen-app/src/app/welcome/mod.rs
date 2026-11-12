//! The first-run window (M9 slice R7b, ADR-0115; the owner's F A, S A, T A,
//! D A, P A, M A and W A): a `Dialog` over the empty song with three steps,
//! Sound, Plugins and Ready. The song behind stays usable; ✕, Esc and Skip
//! setup close it and keep what was set. It opens on a first start until it
//! is finished or closed, and from "Set up Lumen…". Its state and the rules
//! the window shows are `rules.rs`; the device and folder rows are the
//! Preferences pages' own.

mod plugins;
mod ready;
mod rules;
mod sound;

/// The window's state and the rules it shows, as `crate::welcome` (the name
/// startup and `App` still use).
pub(crate) use self::rules::{Welcome, opens_at_start};

use eframe::egui::{self, Align, Key, Layout, RichText, Ui, pos2, vec2};
use lumen_core::product::PRODUCT_NAME;
use lumen_engine::chime::{self, Sounding};

use crate::audio::{DEFAULT_DRIVER, Status, driver_label};
use crate::commands::Command;
use crate::icons::{self, Icon};
use crate::plugins::Status as Scan;
use crate::preferences::{Marks, audio};
use crate::settings::{self, AudioSettings};
use crate::widgets::{hint, row, section, warning};
use crate::{App, fonts, theme, widgets};

use self::rules::{AsioOffer, Health, Mark, Step};

/// How much of a short window's height the window's top sits below the
/// app's (at most `size::PREFERENCES_TOP`), as Preferences does.
const TOP_SHARE: f32 = 0.12;
/// The Plugins step when this run was started without plugins.
const OFF_THIS_RUN: &str = "This run started with plugins off. Start again normally to load them.";
/// How strong a lit light's ring is at silence; it reaches full at the
/// chime's peak.
const RING_LEAST: f32 = 0.38;
/// What the footer's hint says.
const KEYS_HINT: &str = "Tab moves between controls \u{b7} Esc skips";

/// What the footer asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Footer {
    Skip,
    Back,
    Next,
    Finish,
}

/// Which of Lumi's poses a step shows.
#[derive(Debug, Clone, Copy)]
enum Pose {
    Waving,
    Celebrating,
    Curious,
}

impl App {
    /// Each frame: draws the window while it is open.
    pub(crate) fn welcome_window(&mut self, ctx: &egui::Context) {
        if !self.welcome.open {
            return;
        }
        // Esc goes to an open menu, a field or another window first; only
        // then does it skip setup.
        let escape = ctx.input(|i| i.key_pressed(Key::Escape)) && !self.welcome.esc_for_child;
        let content = ctx.content_rect();
        let margins = ctx.global_style().spacing.window_margin.sum().x + 2.0 * theme::space::M;
        let width =
            (content.width() - margins).clamp(theme::size::PREFERENCES_LEAST, theme::size::WELCOME);
        let top = content.top() + theme::size::PREFERENCES_TOP.min(content.height() * TOP_SHARE);
        let title = format!("Welcome to {PRODUCT_NAME}");
        let step = self.welcome.step;
        let marks = Step::ALL.map(|s| self.step_mark(s));
        let mut picked = None;
        let mut footer = None;
        let closed = widgets::Dialog::new("welcome", &title, width)
            .at(pos2(content.center().x, top), egui::Align2::CENTER_TOP)
            .esc_to_caller()
            .head(|ui| step_pills(ui, step, marks, &mut picked))
            .show(ctx, |ui| {
                let room = content.bottom() - ui.cursor().top() - theme::size::PREFERENCES_BELOW;
                footer = self.welcome_body(ui, room);
            });
        if let Some(step) = picked {
            self.welcome.go(step);
        }
        match footer {
            Some(Footer::Back) => {
                if let Some(back) = self.welcome.step.back() {
                    self.welcome.go(back);
                }
            }
            Some(Footer::Next) => {
                if let Some(next) = self.welcome.step.next() {
                    self.welcome.go(next);
                }
            }
            Some(Footer::Skip | Footer::Finish) => self.finish_welcome(ctx),
            None => {}
        }
        if closed || escape {
            self.finish_welcome(ctx);
        }
        self.welcome.esc_for_child =
            egui::Popup::is_any_open(ctx) || ctx.text_edit_focused() || self.other_window_open();
    }

    /// Closes the window in any way: setup counts as done, and the settings
    /// are written at once. The keyboard goes back to the song.
    fn finish_welcome(&mut self, ctx: &egui::Context) {
        if !self.welcome.open {
            return;
        }
        self.welcome.open = false;
        self.transport.audio.stop_sound_check();
        // The Ready step's switch (M9 R8): a copy registered elsewhere is
        // left for the Preferences row's "Use this copy".
        if self.welcome.step == Step::Ready && crate::association::Association::supported() {
            use crate::association::State;
            let wish = self.settings.projects.associate_files;
            match self.association_status().map(|status| status.state.clone()) {
                Some(State::Off) if wish => self.set_association(true),
                Some(State::This) if !wish => self.set_association(false),
                _ => {}
            }
        }
        self.settings.setup_done = true;
        self.write_settings();
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        ctx.request_repaint();
    }

    /// How the Sound step stands.
    fn sound_health(&self) -> Health {
        let audio = &self.transport.audio;
        if audio.changing().is_some() || !audio.can_change() {
            return Health::Busy;
        }
        match audio.status {
            Status::Running { .. } => Health::Good,
            Status::Starting => Health::Busy,
            Status::Lost(_) | Status::Off(_) => Health::Needs,
        }
    }

    /// How the Plugins step stands.
    fn plugins_health(&self) -> Health {
        if self.safe.on || self.plugins.is_off() {
            return Health::Needs;
        }
        match self.plugins.status() {
            Scan::Failed(_) | Scan::Off => Health::Needs,
            Scan::Loading | Scan::Scanning { .. } => Health::Busy,
            Scan::Idle if self.plugins.waiting().is_some() => Health::Busy,
            Scan::Idle => Health::Good,
        }
    }

    fn step_mark(&self, step: Step) -> Mark {
        let seen = self.welcome.seen(step);
        match step {
            Step::Sound => rules::mark(self.sound_health(), seen),
            Step::Plugins => rules::mark(self.plugins_health(), seen),
            Step::Ready => Mark::None,
        }
    }

    /// The step, scrolling between the head and the footer, then the
    /// footer, so the pills and Next are always in reach.
    fn welcome_body(&mut self, ui: &mut Ui, room: f32) -> Option<Footer> {
        let footer = theme::size::CONTROL + 2.0 * theme::space::M + theme::stroke::HAIRLINE;
        let body = (room - footer).max(theme::size::PREFERENCES_LEAST_PAGE);
        egui::ScrollArea::vertical()
            .id_salt(("welcome step", self.welcome.step))
            .max_height(body)
            .auto_shrink([false, true])
            .show(ui, |ui| match self.welcome.step {
                Step::Sound => self.welcome_sound(ui),
                Step::Plugins => self.welcome_plugins(ui),
                Step::Ready => self.welcome_ready(ui),
            });
        ui.add_space(theme::space::S);
        ui.separator();
        ui.add_space(theme::space::XS);
        self.welcome_footer(ui)
    }

    /// Lumi (when there is room), the step's display title and one line.
    fn welcome_intro(&mut self, ui: &mut Ui, title: &str, line: &str, pose: Option<Pose>) {
        let ctx = ui.ctx().clone();
        let roomy = ui.available_width() >= theme::size::WELCOME_NARROW;
        let picture = pose.filter(|_| roomy).and_then(|pose| {
            let lumi = self.welcome.lumi(&ctx)?;
            Some(match pose {
                Pose::Waving => lumi.waving.id(),
                Pose::Celebrating => lumi.celebrating.id(),
                Pose::Curious => lumi.curious.id(),
            })
        });
        ui.add_space(theme::space::S);
        ui.horizontal_top(|ui| {
            if let Some(texture) = picture {
                // Decorative: Lumi has no accessible name.
                ui.add(egui::Image::new(egui::load::SizedTexture::new(
                    texture,
                    vec2(theme::size::LUMI, theme::size::LUMI),
                )));
                ui.add_space(theme::space::XL);
            }
            ui.vertical(|ui| {
                ui.add_space(if picture.is_some() {
                    theme::space::L
                } else {
                    0.0
                });
                ui.add(
                    egui::Label::new(
                        RichText::new(title).font(fonts::display(theme::size::DISPLAY)),
                    )
                    .wrap(),
                );
                ui.add_space(theme::space::XS);
                ui.add(egui::Label::new(RichText::new(line).weak()).wrap());
            });
        });
        ui.add_space(theme::space::S);
    }

    // ── Sound ───────────────────────────────────────────────────────────

    /// Skip setup or Back at the left, the keys at the middle, and the
    /// window's one filled button at the right: Next, or Start making
    /// music on Ready. Enter means Next only when nothing has the keyboard
    /// and no menu is open.
    fn welcome_footer(&mut self, ui: &mut Ui) -> Option<Footer> {
        let step = self.welcome.step;
        let mut asked = None;
        let width = ui.available_width();
        ui.allocate_ui_with_layout(
            vec2(width, theme::size::CONTROL),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_size(vec2(width, theme::size::CONTROL));
                let (label, tip, footer) = match step {
                    Step::Sound => (
                        "Skip setup",
                        "Close this window; everything here is also in Preferences",
                        Footer::Skip,
                    ),
                    _ => ("Back", "The step before", Footer::Back),
                };
                if widgets::raised::button(ui, label)
                    .on_hover_text(tip)
                    .clicked()
                {
                    asked = Some(footer);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (label, footer) = match step {
                        Step::Ready => ("Start making music", Footer::Finish),
                        _ => ("Next", Footer::Next),
                    };
                    let next = widgets::raised::primary_button(ui, label);
                    if std::mem::take(&mut self.welcome.focus_next) {
                        next.request_focus();
                    }
                    if next.clicked() {
                        asked = Some(footer);
                    }
                    let enter = ui.input(|i| i.key_pressed(Key::Enter))
                        && ui.ctx().memory(|m| m.focused().is_none())
                        && !egui::Popup::is_any_open(ui.ctx());
                    if enter && asked.is_none() {
                        asked = Some(footer);
                    }
                    ui.add(egui::Label::new(RichText::new(KEYS_HINT).small().weak()).truncate());
                });
            },
        );
        asked
    }
}

/// The step pills in the title row: each can be clicked, and shows ✓ when
/// its step is good or the warning shape when it needs something.
fn step_pills(ui: &mut Ui, step: Step, marks: [Mark; 3], picked: &mut Option<Step>) {
    for (i, s) in Step::ALL.into_iter().enumerate() {
        let label = format!("{} {}", i + 1, s.name());
        let pill = widgets::Pill {
            label: &label,
            tip: match s {
                Step::Sound => "Choose and test the sound device",
                Step::Plugins => "Plugin folders and scanning",
                Step::Ready => "What is set, and the look",
            },
        };
        let chosen = if s == step { 0 } else { 1 };
        if widgets::pill_tabs(ui, egui::Id::new(("welcome step", i)), chosen, &[pill]).is_some() {
            *picked = Some(s);
        }
        let size = theme::size::MENU_ICON;
        match marks[i] {
            Mark::None => {}
            Mark::Good => {
                let response = ui.label(RichText::new("\u{2713}").color(theme::p().accent));
                let name = format!("{} is set", s.name());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name)
                });
            }
            Mark::Needs => {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
                ui.painter()
                    .extend(icons::shapes(Icon::Warning, rect, theme::p().warning));
                let name = format!("{} needs attention", s.name());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name)
                });
            }
        }
        ui.add_space(theme::space::XS);
    }
}
