//! The Lumen application: the single-window layout from the specification
//! (transport bar, browser, arrangement, detail panel) on egui, with the
//! arrangement canvas drawn by `lumen-render` and audio from `lumen-engine`.
//! Features arrive milestone by milestone; controls that do not work yet are
//! shown disabled and say when they arrive.
//!
//! ```text
//! lumen                          run the application
//! lumen --audio asio:"Focusrite USB ASIO"
//!                                use this device (driver: asio, wasapi,
//!                                exclusive; cpal on macOS and Linux)
//! lumen --open-audio drums.wav   add an audio file at bar 1 (or drag files
//!                                from Explorer onto the arrangement)
//! lumen --scale 1.5              UI scale
//! lumen --audio-settings         open the audio settings window at start
//! lumen --probe-asio <clsid>     (helper) report one ASIO driver as JSON
//! lumen --smoke-test             render a few frames, report start-up time, exit
//! lumen --screenshot out.png     render, save a screenshot, exit
//! ```

mod audio;
mod audio_settings;
mod canvas;
mod document;
mod fonts;
mod icons;
mod lanes;
mod media;
mod motion;
mod panels;
mod position;
mod probe;
mod ruler;
mod settings;
mod theme;
mod trace;
mod transport;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Sense, Stroke, pos2, vec2};
use lumen_core::product::{PRODUCT_NAME, display_name};
use lumen_core::time::{TempoPoint, Ticks};
use lumen_project::model::{Project, TimeSignature};

use transport::Transport;

const RULER_HEIGHT: f32 = 22.0;

struct App {
    /// The open project with its undo history (M3 slice 3).
    document: document::Document,
    /// The document state the transport's timeline was built from.
    synced: Option<u64>,
    /// The undo history window is open.
    show_history: bool,
    /// The note shown after undo or redo, and since when.
    toast: Option<(String, Instant)>,
    transport: Transport,
    /// Audio clips on the simple timeline (M2 slice 6).
    lanes: lanes::Lanes,
    /// Where dropped files go: under the pointer, snapped, or the playhead.
    drop_target: Ticks,
    settings: settings::Settings,
    settings_path: Option<PathBuf>,
    /// Settings changed at this time and are not saved yet (saved once
    /// changes stop for half a second, so dragging a slider is one write).
    settings_changed: Option<Instant>,
    show_browser: bool,
    show_detail: bool,
    /// Full panel sizes, for sliding (kept while the panels are open).
    browser_size: f32,
    detail_size: f32,
    /// Set by the raw-input hook when Shift+Tab is pressed.
    toggle_detail_requested: bool,
    left_beat: f32,
    px_per_beat: f32,
    smoke_test_frames: Option<u32>,
    /// Where `--screenshot` saves, once the frames have rendered.
    screenshot: Option<PathBuf>,
    started: Instant,
    /// `--autoplay-ms`: toggle play and stop this often (latency tests).
    autoplay: Option<Duration>,
    autoplay_count: u32,
    /// Playing or not, as last traced, to log only changes.
    traced: Option<bool>,
    /// `--scale` or `--appearance` given: a one-off, not saved; the file
    /// keeps `saved_appearance` until the user changes it in the menu.
    appearance_override: bool,
    saved_appearance: settings::Appearance,
    /// The OS theme when the appearance was last applied ("Match system").
    system_theme: Option<egui::Theme>,
    /// The device banner's last message, kept while it slides away.
    banner: Option<(String, bool)>,
}

pub(crate) fn empty_project() -> Project {
    Project {
        name: "Untitled".into(),
        sample_rate: 48_000,
        tempo: vec![TempoPoint {
            at: Ticks::ZERO,
            bpm: 120.0,
        }],
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tracks: Vec::new(),
        media: Vec::new(),
        transport: Default::default(),
        view: Default::default(),
    }
}

fn arrives(milestone: &str) -> String {
    format!("Arrives in {milestone}")
}

impl App {
    fn transport(&mut self, ui: &mut egui::Ui) {
        // The panel toggles are laid out first, from the right, so they keep
        // their place at any width or scale; the transport takes the rest and
        // the status text shortens to fit.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.toggle_value(&mut self.show_detail, "Detail")
                .on_hover_text("Show or hide the detail panel (Shift+Tab)");
            ui.toggle_value(&mut self.show_browser, "Browser")
                .on_hover_text("Show or hide the browser");
            ui.separator();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                // Whatever does not fit is cut off here rather than drawn
                // over the panel toggles.
                ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                self.app_menu(ui);
                ui.separator();
                self.transport.ui(ui, &mut self.document);
            });
        });
    }

    /// Undo (or redo) one step, with a brief note of what changed.
    fn undo(&mut self, redo: bool) {
        let done = if redo {
            self.document.redo().map(|l| format!("Redo: {l}"))
        } else {
            self.document.undo().map(|l| format!("Undo: {l}"))
        };
        if let Some(text) = done {
            trace::log(|| text.clone());
            self.toast = Some((text, Instant::now()));
        }
    }

    /// Undo and redo from the keyboard: Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y.
    /// Not while typing in a field, which has its own text undo.
    fn undo_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        use egui::{Key, KeyboardShortcut, Modifiers};
        let redo = ctx.input_mut(|i| {
            i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::Z,
            )) || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y))
        });
        let undo = ctx
            .input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)));
        if redo {
            self.undo(true);
        } else if undo {
            self.undo(false);
        }
    }

    /// Keeps the app in step with the document: gestures end when the
    /// pointer is released, and the transport follows tempo and meter
    /// changes from edits, undo and redo.
    fn sync_document(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.pointer.any_down()) {
            self.document.seal();
        }
        if self.synced != Some(self.document.state()) {
            self.transport.timeline_changed(self.document.project());
            self.synced = Some(self.document.state());
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.document.title()));
        }
    }

    /// The undo history: every step, the current one highlighted; click a
    /// step to go back (or forward) to it.
    fn history_window(&mut self, ctx: &egui::Context) {
        if !self.show_history {
            return;
        }
        let mut open = true;
        let mut jump = None;
        egui::Window::new("History")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .default_pos(pos2(ctx.content_rect().right() - 280.0, 72.0))
            .min_width(240.0)
            .max_width(240.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("History").font(fonts::display(theme::size::TITLE)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::close_button(ui).clicked() {
                            open = false;
                        }
                    });
                });
                ui.add_space(theme::space::XS);
                let (done, undone) = self.document.history().steps();
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        let row = |ui: &mut egui::Ui, text: &str, current: bool, future: bool| {
                            let text = if future {
                                RichText::new(text).color(theme::p().text_hint)
                            } else {
                                RichText::new(text)
                            };
                            ui.add_sized(
                                [ui.available_width(), 22.0],
                                egui::Button::selectable(current, text),
                            )
                            .clicked()
                        };
                        if row(ui, "Opened", done.is_empty(), false) {
                            jump = Some(0);
                        }
                        for (i, label) in done.iter().enumerate() {
                            if row(ui, label, i + 1 == done.len(), false) {
                                jump = Some(i + 1);
                            }
                        }
                        for (i, label) in undone.iter().enumerate() {
                            if row(ui, label, false, true) {
                                jump = Some(done.len() + i + 1);
                            }
                        }
                    });
                if done.is_empty() && undone.is_empty() {
                    ui.label(
                        RichText::new("Edits appear here. Click one to go back to it.")
                            .small()
                            .weak(),
                    );
                }
            });
        if let Some(step) = jump {
            self.document.jump_to(step);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        self.show_history = open;
    }

    /// A short note after undo or redo, fading out after a moment.
    fn toast(&mut self, ctx: &egui::Context, at: egui::Pos2) {
        let Some((text, since)) = &self.toast else {
            return;
        };
        const SHOWN: Duration = Duration::from_millis(1400);
        let visible = since.elapsed() < SHOWN;
        let fade = motion::fade(ctx, egui::Id::new("toast"), visible);
        if !visible && fade == 0.0 {
            self.toast = None;
            return;
        }
        if visible {
            ctx.request_repaint_after(SHOWN.saturating_sub(since.elapsed()));
        }
        let text = text.clone();
        egui::Area::new(egui::Id::new("toast area"))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(at + vec2(0.0, (1.0 - fade) * 6.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.multiply_opacity(fade);
                egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(text));
            });
    }

    /// The product name opens the app menu: appearance (theme, motion, UI
    /// scale) and audio settings. Preferences proper arrive in M9.
    fn app_menu(&mut self, ui: &mut egui::Ui) {
        let before = self.settings.appearance.clone();
        let menu = ui.menu_button(
            RichText::new(PRODUCT_NAME).font(fonts::display(theme::size::WORDMARK)),
            |ui| {
                let undo = self.document.history().undo_label().map(str::to_owned);
                let redo = self.document.history().redo_label().map(str::to_owned);
                let undo_text = undo.map_or("Undo".to_owned(), |l| format!("Undo {l}"));
                let redo_text = redo.map_or("Redo".to_owned(), |l| format!("Redo {l}"));
                if ui
                    .add_enabled(
                        self.document.history().can_undo(),
                        egui::Button::new(undo_text).shortcut_text("Ctrl+Z"),
                    )
                    .clicked()
                {
                    self.undo(false);
                }
                if ui
                    .add_enabled(
                        self.document.history().can_redo(),
                        egui::Button::new(redo_text).shortcut_text("Ctrl+Shift+Z"),
                    )
                    .clicked()
                {
                    self.undo(true);
                }
                if ui.button("History…").clicked() {
                    self.show_history = true;
                    ui.close();
                }
                ui.separator();
                ui.label(RichText::new("Theme").small().weak());
                let mut chosen = theme::Appearance::from_key(&self.settings.appearance.theme);
                for option in theme::Appearance::ALL {
                    ui.radio_value(&mut chosen, option, option.name());
                }
                self.settings.appearance.theme = chosen.key().to_owned();
                ui.separator();
                ui.label(RichText::new("Interface size").small().weak());
                let mut scale = self.settings.appearance.scale();
                for option in settings::UI_SCALES {
                    ui.radio_value(&mut scale, option, format!("{:.0}%", option * 100.0));
                }
                self.settings.appearance.ui_scale = scale;
                ui.separator();
                ui.checkbox(&mut self.settings.appearance.reduce_motion, "Reduce motion")
                    .on_hover_text("Things change in place instead of sliding or fading");
                ui.separator();
                if ui.button("Audio settings…").clicked() {
                    self.transport.settings_window.open = true;
                    ui.close();
                }
            },
        );
        menu.response
            .on_hover_text("Appearance, interface size and audio settings");
        if self.settings.appearance != before {
            self.appearance_override = false;
            self.saved_appearance = self.settings.appearance.clone();
            self.apply_appearance(ui.ctx());
            self.settings_changed = Some(Instant::now());
        }
    }

    /// Applies the saved (or one-off) appearance: palette and egui style,
    /// motion, and UI scale.
    fn apply_appearance(&mut self, ctx: &egui::Context) {
        let appearance = &self.settings.appearance;
        motion::set_reduced(appearance.reduce_motion);
        theme::apply(
            ctx,
            theme::Appearance::from_key(&appearance.theme),
            motion::time(motion::BASE),
        );
        ctx.set_zoom_factor(appearance.scale());
        self.system_theme = ctx.system_theme();
    }

    fn arrangement(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(full, Sense::hover());
        let ruler = egui::Rect::from_min_size(full.min, vec2(full.width(), RULER_HEIGHT));
        let canvas = egui::Rect::from_min_max(pos2(full.left(), ruler.bottom()), full.max);

        // `contains_pointer`, not `hovered`: the ruler's own click area sits on
        // top, and scrolling and zooming must work over it too.
        if response.contains_pointer() {
            // egui turns Ctrl+wheel (and trackpad pinch) into a zoom factor
            // and reports no scroll for it, so zoom must read `zoom_delta`.
            let (delta, zoom, pointer) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            if zoom != 1.0 {
                let anchor = pointer.map_or(canvas.center().x, |p| p.x) - canvas.left();
                let beat = self.left_beat + anchor / self.px_per_beat;
                self.px_per_beat = (self.px_per_beat * zoom).clamp(2.0, 400.0);
                self.left_beat = beat - anchor / self.px_per_beat;
            } else {
                self.left_beat -= (delta.x + delta.y) / self.px_per_beat;
            }
            self.left_beat = self.left_beat.max(0.0);
        }

        let painter = ui.painter_at(full);
        painter.rect_filled(full, 0.0, theme::p().canvas);
        let meter = self.transport.timeline.meter.meter_at(Ticks::ZERO);
        painter.add(canvas::grid_shape(
            canvas,
            ui.ctx().pixels_per_point(),
            self.left_beat,
            self.px_per_beat,
            canvas::GridMeter {
                bar: meter.bar().as_quarters() as f32,
                beat: meter.beat().as_quarters() as f32,
                colors: [theme::p().grid_bar, theme::p().grid_beat],
            },
        ));
        let x_of = |quarters: f32| canvas.left() + (quarters - self.left_beat) * self.px_per_beat;
        self.lanes
            .paint(&painter, canvas, self.document.project(), x_of);
        // A hover card with the file's details.
        let hover = ui.interact(canvas, ui.id().with("clips"), Sense::hover());
        if let Some(pointer) = hover.hover_pos()
            && let Some(item) = self
                .lanes
                .file_at(self.document.project(), pointer, canvas, x_of)
        {
            hover.on_hover_ui_at_pointer(|ui| lanes::file_card(ui, item));
        }

        // Dropped files land under the pointer (snapped) when the system
        // reports it, otherwise at the playhead.
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| canvas.contains(*p));
        self.drop_target = match pointer {
            Some(p) => ruler::snap(
                &self.transport.timeline.meter,
                ruler::ticks_at_x(p.x, canvas.left(), self.left_beat, self.px_per_beat),
                self.px_per_beat,
            ),
            None => self.transport.state().ticks,
        };

        // Ruler: loop range, then bar numbers thinned so labels never collide.
        painter.rect_filled(ruler, 0.0, theme::p().raised);
        let looped = self.transport.loop_region;
        if looped.enabled {
            let span = egui::Rect::from_x_y_ranges(
                x_of(looped.start.as_quarters() as f32)..=x_of(looped.end.as_quarters() as f32),
                ruler.y_range(),
            );
            painter.rect_filled(
                span.intersect(ruler),
                0.0,
                theme::p().accent.gamma_multiply(0.18),
            );
        }
        let bar_quarters = meter.bar().as_quarters() as f32;
        let mut step = 1;
        while step as f32 * bar_quarters * self.px_per_beat < 40.0 {
            step *= 2;
        }
        let first_bar = (self.left_beat / bar_quarters).floor() as i64;
        let mut bar = first_bar - first_bar.rem_euclid(step);
        loop {
            let start = self.transport.timeline.meter.bar_start(bar);
            let x = x_of(start.as_quarters() as f32);
            if x > full.right() || bar >= canvas::GRID_BARS as i64 {
                break;
            }
            if x >= full.left() {
                painter.text(
                    pos2(x + 4.0, ruler.center().y),
                    egui::Align2::LEFT_CENTER,
                    (bar + 1).to_string(),
                    egui::FontId::monospace(11.0),
                    theme::p().text_secondary,
                );
            }
            bar += step;
        }

        // Click-to-locate: the ruler moves the playhead to the nearest line of
        // the zoom-adaptive grid (Alt bypasses snapping). A ghost line shows
        // where a click will land.
        let ruler_response = ui
            .interact(ruler, ui.id().with("ruler"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Click to move the playhead. Hold Alt to place it freely");
        ruler_response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Ruler: click to move the playhead",
            )
        });
        if let Some(pointer) = ruler_response.hover_pos() {
            let free = ui.input(|i| i.modifiers.alt);
            let raw = ruler::ticks_at_x(pointer.x, canvas.left(), self.left_beat, self.px_per_beat);
            let target = if free {
                raw
            } else {
                ruler::snap(&self.transport.timeline.meter, raw, self.px_per_beat)
            };
            let gx = x_of(target.as_quarters() as f32);
            painter.line_segment(
                [pos2(gx, ruler.top()), pos2(gx, full.bottom())],
                Stroke::new(1.0, theme::p().accent.gamma_multiply(0.5)),
            );
            if ruler_response.clicked() {
                trace::log(|| format!("ruler: locate {target:?} (snapped: {})", !free));
                self.transport.locate(target);
            }
        }

        // The start marker: where Play starts. A neutral arrow on the ruler
        // only, at its bottom edge, so it stays visible when the playhead
        // (blue, at the top edge) is at the same place.
        // Both glide to a new place while stopped (motion: GLIDE) and follow
        // the audio exactly while playing.
        let state = self.transport.state();
        let ctx = ui.ctx().clone();
        let marker = motion::glide(
            &ctx,
            egui::Id::new("start marker"),
            state.marker.as_quarters() as f32,
            !state.playing,
        );
        let head = motion::glide(
            &ctx,
            egui::Id::new("playhead"),
            state.ticks.as_quarters() as f32,
            !state.playing,
        );
        let mx = x_of(marker);
        if (full.left()..=full.right()).contains(&mx) {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(mx - 6.0, ruler.bottom() - 9.0),
                    pos2(mx + 6.0, ruler.bottom() - 9.0),
                    pos2(mx, ruler.bottom() - 1.0),
                ],
                theme::p().start_marker,
                Stroke::new(1.0, theme::p().canvas),
            ));
        }

        // Playhead.
        let x = x_of(head);
        if (full.left()..=full.right()).contains(&x) {
            painter.line_segment(
                [pos2(x, ruler.top()), pos2(x, full.bottom())],
                Stroke::new(1.0, theme::p().accent),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    pos2(x - 5.0, ruler.top()),
                    pos2(x + 5.0, ruler.top()),
                    pos2(x, ruler.top() + 7.0),
                ],
                theme::p().accent,
                Stroke::NONE,
            ));
        }

        // While files are dragged over the window, say which will be added.
        let hovered: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .hovered_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !hovered.is_empty() {
            let supported = hovered.iter().filter(|p| media::is_audio_file(p)).count();
            let color = if supported > 0 {
                theme::p().accent
            } else {
                theme::p().warning
            };
            painter.rect_stroke(
                canvas.shrink(2.0),
                4.0,
                Stroke::new(2.0, color),
                egui::StrokeKind::Inside,
            );
            let mut lines: Vec<String> = hovered
                .iter()
                .take(8)
                .map(|p| {
                    let name = p
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    match media::format_name(p) {
                        Some(format) => format!("✓  {name}  ({format})"),
                        None => format!("✕  {name}  (not an audio file Lumen reads)"),
                    }
                })
                .collect();
            if hovered.len() > 8 {
                lines.push(format!("…and {} more", hovered.len() - 8));
            }
            let headline = match (supported, hovered.len()) {
                (0, _) => "Nothing here can be added".to_owned(),
                (n, total) if n == total => {
                    format!(
                        "Drop to add {n} audio file{}",
                        if n == 1 { "" } else { "s" }
                    )
                }
                (n, total) => format!("Drop to add {n} of {total} files"),
            };
            painter.text(
                canvas.center(),
                egui::Align2::CENTER_CENTER,
                format!("{headline}\n\n{}", lines.join("\n")),
                egui::FontId::proportional(15.0),
                color,
            );
        } else if self.lanes.is_empty(self.document.project()) {
            painter.text(
                canvas.center(),
                egui::Align2::CENTER_CENTER,
                "Drop audio files here\nWAV, AIFF, FLAC, MP3 or Ogg Vorbis",
                egui::FontId::proportional(14.0),
                theme::p().text_hint,
            );
        }
        self.lanes
            .panel(ui.ctx(), pos2(full.left() + 8.0, full.bottom() - 8.0));
        self.toast(ui.ctx(), pos2(full.center().x, full.bottom() - 16.0));
    }
}

impl App {
    /// A banner under the transport when the audio device needs attention:
    /// lost (reconnecting), busy in another application, or failed. Each
    /// offers a way forward.
    fn device_banner(&mut self, ui: &mut egui::Ui) {
        let audio = &self.transport.audio;
        let current = match &audio.status {
            audio::Status::Lost(_) => Some((
                format!(
                    "{} was disconnected. {} reconnects when it is back.",
                    audio.device,
                    lumen_core::product::PRODUCT_NAME
                ),
                false,
            )),
            audio::Status::Off(_) if audio.is_busy() => Some((
                format!(
                    "{} is in use by another application. Close it there, then try again.",
                    audio.device
                ),
                true,
            )),
            audio::Status::Off(why)
                if !audio.is_on() && why != "Audio is off" && !why.is_empty() =>
            {
                Some((format!("Audio could not start: {why}"), true))
            }
            _ => None,
        };
        // The banner slides in and out (motion: BASE); while it slides
        // away it keeps showing its last message, with its buttons off.
        let mut open = current.is_some();
        if let Some(current) = current {
            self.banner = Some(current);
        }
        let Some((message, retry)) = self.banner.clone() else {
            return;
        };
        let live = open;
        let shown = egui::Panel::top("device banner").show_collapsible(ui, &mut open, |ui| {
            ui.horizontal(|ui| {
                let warning = theme::p().warning;
                ui.label(RichText::new("⚠").color(warning));
                ui.label(RichText::new(&message).color(warning));
                ui.add_enabled_ui(live, |ui| {
                    if retry
                        && self.transport.audio.can_switch()
                        && ui.button("Try again").clicked()
                    {
                        self.transport.audio.set_on(true);
                    }
                    if ui.button("Audio settings…").clicked() {
                        self.transport.settings_window.open = true;
                    }
                });
            });
        });
        if shown.is_none() {
            self.banner = None;
        }
    }

    /// Notes settings changes and writes them once they settle.
    fn save_settings_when_settled(&mut self, ctx: &egui::Context) {
        // A device setup that worked (chosen in the audio settings).
        if let Some(audio) = self.transport.audio.confirmed.take() {
            self.settings.audio = Some(audio);
            self.settings_changed = Some(Instant::now());
        }
        let metronome = settings::Metronome::from(self.transport.metronome);
        if metronome != self.settings.metronome {
            self.settings.metronome = metronome;
            self.settings_changed = Some(Instant::now());
        }
        let Some(changed) = self.settings_changed else {
            return;
        };
        let settle = Duration::from_millis(500);
        if changed.elapsed() < settle {
            ctx.request_repaint_after(settle - changed.elapsed());
            return;
        }
        self.settings_changed = None;
        let mut file = self.settings.clone();
        if self.appearance_override {
            file.appearance = self.saved_appearance.clone();
        }
        if let Some(path) = &self.settings_path
            && let Err(e) = settings::save(path, &file)
        {
            self.transport.audio.notice = Some(format!("Could not save settings: {e}"));
        }
    }
}

impl eframe::App for App {
    /// Shift+Tab toggles the detail panel (spec §5). It is taken here, before
    /// egui sees it, so it does not also move keyboard focus backwards.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        for event in &raw_input.events {
            if let egui::Event::PointerButton { pos, pressed, .. } = event {
                trace::log(|| {
                    format!(
                        "input: mouse {} at ({:.0}, {:.0})",
                        if *pressed { "down" } else { "up" },
                        pos.x,
                        pos.y
                    )
                });
            }
            if let egui::Event::Key {
                key, pressed: true, ..
            } = event
            {
                trace::log(|| format!("input: key {key:?}"));
            }
        }
        raw_input.events.retain(|event| {
            let toggle = matches!(
                event,
                egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. }
                    if modifiers.shift && !modifiers.ctrl && !modifiers.alt
            );
            if toggle {
                self.toggle_detail_requested = true;
            }
            // Drop the release too, so egui never sees half of the shortcut.
            !matches!(
                event,
                egui::Event::Key { key: egui::Key::Tab, modifiers, .. } if modifiers.shift && !modifiers.ctrl && !modifiers.alt
            )
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.transport.update();
        self.transport.shortcuts(&ctx);
        self.undo_shortcuts(&ctx);
        let rate = self.transport.state().sample_rate;
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            trace::log(|| format!("drop: {} file(s) at {:?}", dropped.len(), self.drop_target));
            self.lanes
                .add_files(dropped, self.drop_target, rate, &ctx, &mut self.document);
        }
        self.lanes.update(
            rate,
            &ctx,
            &mut self.document,
            &mut self.transport.audio.clips,
        );
        self.sync_document(&ctx);
        self.transport.clip_memory = self.lanes.library.memory();
        self.save_settings_when_settled(&ctx);
        if let Some(every) = self.autoplay {
            // Toggle play and stop every `every`, as Space would.
            let elapsed = self.started.elapsed();
            let due = every * self.autoplay_count;
            if elapsed >= due {
                self.autoplay_count += 1;
                trace::log(|| format!("autoplay: toggle {}", self.autoplay_count));
                self.transport.play_stop();
            } else {
                ctx.request_repaint_after(due - elapsed);
            }
        }
        if trace::enabled() {
            let state = self.transport.state();
            if self.traced != Some(state.playing) {
                trace::log(|| {
                    format!(
                        "frame: shows {} at sample {}",
                        if state.playing { "PLAYING" } else { "STOPPED" },
                        state.sample
                    )
                });
                self.traced = Some(state.playing);
            }
        }
        if ctx.system_theme() != self.system_theme {
            self.apply_appearance(&ctx);
        }
        let panels_before = (self.show_browser, self.show_detail);
        if std::mem::take(&mut self.toggle_detail_requested) {
            self.show_detail = !self.show_detail;
        }

        egui::Panel::top("transport").show(ui, |ui| self.transport(ui));
        self.device_banner(ui);
        self.transport.windows(&ctx);
        self.history_window(&ctx);

        // A focused control must not vanish with its panel: AccessKit
        // rejects focus on a node that no longer exists. Release focus
        // whenever a panel is hidden, whatever hid it.
        let hid_a_panel =
            (panels_before.0 && !self.show_browser) || (panels_before.1 && !self.show_detail);
        if hid_a_panel {
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
        // Panels slide open and closed (motion: GLIDE, panels.rs).
        panels::sliding(
            ui,
            "browser",
            panels::Side::Left,
            self.show_browser,
            &mut self.browser_size,
            |ui| {
                ui.label(RichText::new("Browser").font(fonts::display(theme::size::HEADING)));
                ui.add_space(8.0);
                ui.label(RichText::new("Devices, samples, projects and your places").weak());
                ui.label(RichText::new(arrives("Milestones 3 and 6")).weak().small());
            },
        );
        panels::sliding(
            ui,
            "detail",
            panels::Side::Bottom,
            self.show_detail,
            &mut self.detail_size,
            |ui| {
                ui.horizontal(|ui| {
                    for (tab, milestone) in [
                        ("Devices", "Milestone 6"),
                        ("Clip", "Milestone 5"),
                        ("Mixer", "Milestone 4"),
                    ] {
                        ui.add_enabled(false, egui::Button::new(tab))
                            .on_disabled_hover_text(arrives(milestone));
                    }
                });
                ui.label(
                    RichText::new(
                        "Select a track or clip to see its devices, editor or mixer strip.",
                    )
                    .weak(),
                );
            },
        );
        egui::CentralPanel::default().show(ui, |ui| self.arrangement(ui));

        // Scheduled after drawing, because buttons are clicked while drawing.
        // Every frame while playing or settling after a command; otherwise
        // once a second for device events, so idle CPU stays near zero.
        if self.transport.animating() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        if let Some(path) = &self.screenshot {
            let image = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                    _ => None,
                })
            });
            if let Some(image) = image {
                match save_png(path, &image) {
                    Ok(()) => println!("screenshot saved to {}", path.display()),
                    Err(e) => eprintln!("screenshot: {e}"),
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if let Some(frames) = self.smoke_test_frames.as_mut() {
            *frames = frames.saturating_sub(1);
            if *frames == 0 && self.screenshot.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_test_frames = None;
            } else if *frames == 0 {
                println!(
                    "smoke test: {} frames rendered; first frame within {:.0} ms of start",
                    SMOKE_FRAMES,
                    FIRST_FRAME_MS.get().copied().unwrap_or(f64::NAN)
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint();
            }
        }
        FIRST_FRAME_MS.get_or_init(|| self.started.elapsed().as_secs_f64() * 1e3);
    }
}

const SMOKE_FRAMES: u32 = 30;
static FIRST_FRAME_MS: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

fn save_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let [w, h] = image.size;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|c| c.to_srgba_unmultiplied())
        .collect();
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&bytes))
        .map_err(|e| e.to_string())
}

/// The value after `flag`, if given.
fn arg_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() -> eframe::Result {
    // The helper mode for safe ASIO queries (probe.rs): no window.
    if let Some(clsid) = arg_value(probe::FLAG) {
        std::process::exit(probe::run_helper(&clsid));
    }
    let started = Instant::now();
    let screenshot = arg_value("--screenshot").map(PathBuf::from);
    let smoke_test = screenshot.is_some() || std::env::args().any(|a| a == "--smoke-test");
    let scale: Option<f32> = arg_value("--scale")
        .and_then(|s| s.parse().ok())
        .filter(|s| (0.5..=3.0).contains(s));
    let settings_path = settings::default_path();
    let (saved, settings_warning) = settings_path
        .as_deref()
        .map(settings::load)
        .unwrap_or_default();
    // `--audio` is a one-off choice; otherwise the saved device, otherwise
    // the system default.
    let choice = match arg_value("--audio") {
        Some(arg) => audio::Choice::parse(&arg).unwrap_or_else(|e| {
            eprintln!("{e}; using the default output");
            audio::Choice::Default
        }),
        None => saved
            .audio
            .clone()
            .map_or(audio::Choice::Default, audio::Choice::Saved),
    };
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(display_name())
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([960.0, 540.0]),
        ..Default::default()
    };
    // Compiled GPU pipelines are cached between launches (ADR-0002).
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        let base = setup.device_descriptor.clone();
        setup.device_descriptor = Arc::new(move |adapter| {
            let mut descriptor = base(adapter);
            descriptor.required_features |=
                adapter.features() & eframe::wgpu::Features::PIPELINE_CACHE;
            descriptor
        });
    }
    eframe::run_native(
        PRODUCT_NAME,
        options,
        Box::new(move |cc| {
            if let Some(state) = cc.wgpu_render_state.as_ref() {
                canvas::install(state).map_err(|e| format!("renderer: {e}"))?;
            }
            let mut document = document::Document::new(empty_project());
            let mut transport = Transport::new(document.project(), &choice);
            transport.restore_metronome((&saved.metronome).into());
            transport.settings_window.open = std::env::args().any(|a| a == "--audio-settings");
            if let Some(warning) = settings_warning {
                transport.audio.notice = Some(match transport.audio.notice.take() {
                    Some(notice) => format!("{warning}\n{notice}"),
                    None => warning,
                });
            }
            let mut lanes = lanes::Lanes::default();
            if let Some(path) = arg_value("--open-audio") {
                lanes.add_files(
                    vec![PathBuf::from(path)],
                    Ticks::ZERO,
                    transport.state().sample_rate,
                    &cc.egui_ctx,
                    &mut document,
                );
            }
            let saved_appearance = saved.appearance.clone();
            let mut app = App {
                transport,
                lanes,
                drop_target: Ticks::ZERO,
                settings: saved,
                settings_path,
                settings_changed: None,
                document,
                synced: None,
                show_history: false,
                toast: None,
                show_browser: true,
                show_detail: true,
                browser_size: 220.0,
                detail_size: 220.0,
                toggle_detail_requested: false,
                left_beat: 0.0,
                px_per_beat: 24.0,
                smoke_test_frames: smoke_test.then_some(SMOKE_FRAMES),
                screenshot,
                started,
                autoplay: arg_value("--autoplay-ms")
                    .and_then(|ms| ms.parse().ok())
                    .map(Duration::from_millis),
                autoplay_count: 1,
                traced: None,
                appearance_override: false,
                saved_appearance: saved_appearance.clone(),
                system_theme: None,
                banner: None,
            };
            // `--scale` and `--appearance` are one-off overrides (tests,
            // screenshots); they are not saved unless changed in the menu.
            if let Some(scale) = scale {
                app.settings.appearance.ui_scale = scale;
                app.appearance_override = true;
            }
            if let Some(theme) = arg_value("--appearance") {
                app.settings.appearance.theme = theme::Appearance::from_key(&theme).key().into();
                app.appearance_override = true;
            }
            fonts::install(&cc.egui_ctx);
            app.apply_appearance(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_new_project_is_valid_and_survives_save_and_load() {
        let project = empty_project();
        assert_eq!(lumen_project::validate(&project), Ok(()));
        let bytes = lumen_project::to_bytes(&project).expect("saves");
        assert_eq!(lumen_project::from_bytes(&bytes).expect("loads"), project);
    }

    #[test]
    fn the_window_title_comes_from_the_product_name() {
        assert!(display_name().starts_with(PRODUCT_NAME));
    }
}
