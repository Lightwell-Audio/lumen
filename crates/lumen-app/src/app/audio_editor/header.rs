//! The audio editor's header (M9.5 D24): the clip editors' shared strip, in
//! the first of five layouts that fits. A control that gives up its word
//! keeps it in its tooltip and accessible name; what moves into the ⋯ menu is
//! still there, with its keys.

use eframe::egui::{self, Color32, Rect, Ui, vec2};
use lumen_core::time::Ticks;
use lumen_project::envelope::Kind;
use lumen_project::model::Clip;

use crate::App;
use crate::app::clip_header::{GridChip, HeaderStrip, icon_toggle};
use crate::audio_editor::header::{self, Layout, Widths};
use crate::audio_editor::paint;
use crate::audio_editor::source::WholeFile;
use crate::audio_editor::{AudioEditor, Fit};
use crate::commands::Command;
use crate::icons::{self, Icon};
use crate::theme;
use crate::widgets::{self, raised};

/// An icon on a view button.
const VIEW_ICON: f32 = 14.0;
/// The ⋯ button's dots: their radius and how far apart.
const DOT: f32 = 1.25;
const DOT_GAP: f32 = 4.0;

/// The view buttons: what each says, its command and what it asks for.
const VIEWS: [View; 2] = [
    View {
        label: "Zoom to clip",
        icon: Icon::ZoomIn,
        command: Command::AudioEditorZoomToClip,
        fit: Fit::Clip,
        tip: "Show the part of the file the clip plays",
    },
    View {
        label: "Whole file",
        icon: Icon::Wave,
        command: Command::AudioEditorWholeFile,
        fit: Fit::File,
        tip: "Show the clip's whole file",
    },
];

/// The Zero crossings toggle's word.
const ZERO_CROSSINGS: &str = "Zero crossings";

/// The Loop clip toggle's word.
const LOOP_CLIP: &str = "Loop clip";

/// The envelope pills, Gain, Pan then Pitch (`Kind::ALL`'s order).
const PILLS: [EnvelopePill; 3] = [
    EnvelopePill {
        label: "Gain",
        name: "Gain envelope",
        menu: "Gain envelope",
        command: Command::AudioEditorGainEnvelope,
        tip: "Show the clip's gain envelope over the wave",
    },
    EnvelopePill {
        label: "Pan",
        name: "Pan envelope",
        menu: "Pan envelope",
        command: Command::AudioEditorPanEnvelope,
        tip: "Show the clip's pan envelope over the wave",
    },
    EnvelopePill {
        label: "Pitch",
        name: "Pitch envelope",
        menu: "Pitch envelope",
        command: Command::AudioEditorPitchEnvelope,
        tip: "Show the clip's pitch envelope over the wave, heard while the clip is warped in Melodic, Full mix or Rhythmic",
    },
];

struct EnvelopePill {
    label: &'static str,
    /// Its accessible name, and its row in the ⋯ menu.
    name: &'static str,
    menu: &'static str,
    command: Command,
    tip: &'static str,
}

fn pill_of(kind: Kind) -> &'static EnvelopePill {
    match kind {
        Kind::Gain => &PILLS[0],
        Kind::Pan => &PILLS[1],
        Kind::Pitch => &PILLS[2],
    }
}

/// Loop clip as an icon: a rounded track with an arrow head, going round.
fn loop_glyph(painter: &egui::Painter, centre: egui::Pos2, ink: Color32) {
    let body = Rect::from_center_size(centre, vec2(13.0, 8.0));
    painter.rect_stroke(
        body,
        4.0,
        egui::Stroke::new(theme::stroke::LIT, ink),
        egui::StrokeKind::Middle,
    );
    let tip = egui::pos2(centre.x + 2.0, body.top());
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip + vec2(-3.0, -3.0),
            tip + vec2(-3.0, 3.0),
            tip + vec2(2.0, 0.0),
        ],
        ink,
        egui::Stroke::NONE,
    ));
}

/// Zero crossings as an icon: a wave crossing its middle line, the crossing
/// marked.
fn crossing_glyph(painter: &egui::Painter, centre: egui::Pos2, ink: Color32) {
    let half = 6.0;
    let line = egui::Stroke::new(
        theme::stroke::HAIRLINE,
        ink.gamma_multiply(theme::alpha::EDGE_DIM),
    );
    painter.hline(centre.x - half..=centre.x + half, centre.y, line);
    let points: Vec<egui::Pos2> = (0..=16)
        .map(|i| {
            let t = i as f32 / 16.0;
            let x = centre.x - half + 2.0 * half * t;
            let y = centre.y - 4.0 * (std::f32::consts::TAU * t).sin() * 0.9;
            egui::pos2(x, y)
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(theme::stroke::GLYPH, ink),
    ));
    painter.circle_filled(centre, 1.75, ink);
}

struct View {
    label: &'static str,
    icon: Icon,
    command: Command,
    fit: Fit,
    tip: &'static str,
}

impl App {
    /// The header: the clip's title and the file's length, the grid chip, and
    /// at the right the view buttons and the ⋯ menu.
    pub(super) fn audio_header(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        (clip, colour): (&Clip, Color32),
        file: Option<&WholeFile>,
        ed: &mut AudioEditor,
    ) {
        let ready = file.is_some();
        let caption = self.audio_caption(clip, file);
        let layout = header::choose(self.header_widths(ui, rect, clip, &caption, ed));
        ed.layout = layout;
        let shows_views = layout.shows_views();
        let has_menu = !shows_views;
        let crossings = self.settings.editing.zero_crossings;
        let mut flip_crossings = false;
        let mut flip_loop = false;
        let mut pop = false;
        let popped = self.detail.audio_popped;
        let looping = ed.loop_clip.is_some();
        let right = |ui: &mut Ui| {
            if has_menu {
                let response = raised::icon_button(ui, "More", |painter, centre, ink| {
                    for step in [-DOT_GAP, 0.0, DOT_GAP] {
                        painter.circle_filled(centre + vec2(step, 0.0), DOT, ink);
                    }
                });
                let response = widgets::tip(response, "More", None, None);
                widgets::Menu::below(&response).show(|ui| {
                    if !shows_views {
                        let keys = self.keymap.keys(Command::AudioEditorZeroCrossings);
                        if widgets::menu_toggle(ui, "Zero crossings", keys.as_ref(), crossings)
                            .clicked()
                        {
                            flip_crossings = true;
                            ui.close();
                        }
                        let keys = self.keymap.keys(Command::AudioEditorLoopClip);
                        if widgets::menu_toggle(ui, LOOP_CLIP, keys.as_ref(), looping).clicked() {
                            flip_loop = true;
                            ui.close();
                        }
                    }
                    if !layout.shows_pills() {
                        for kind in Kind::ALL {
                            let pill = pill_of(kind);
                            let keys = self.keymap.keys(pill.command);
                            let on = ed.pill == Some(kind);
                            if widgets::menu_toggle(ui, pill.menu, keys.as_ref(), on).clicked() {
                                ed.toggle_pill(kind);
                                ui.close();
                            }
                        }
                    }
                    if !shows_views {
                        // Left to right as they are in the strip.
                        for view in VIEWS.iter().rev() {
                            let keys = self.keymap.keys(view.command);
                            if widgets::menu_item(ui, view.label, keys.as_ref(), ready).clicked() {
                                ed.fit = Some(view.fit);
                                ui.close();
                            }
                        }
                    }
                    if !shows_views {
                        let keys = self.keymap.keys(Command::PopOutEditor);
                        let label = if popped { "Dock" } else { "Pop out" };
                        if widgets::menu_item(ui, label, keys.as_ref(), true).clicked() {
                            pop = true;
                            ui.close();
                        }
                    }
                });
            }
            if shows_views {
                for view in &VIEWS {
                    let response = ui
                        .add_enabled_ui(ready, |ui| {
                            if layout == Layout::Words {
                                raised::button(ui, view.label)
                            } else {
                                raised::icon_button(ui, view.label, |painter, centre, ink| {
                                    let at =
                                        Rect::from_center_size(centre, vec2(VIEW_ICON, VIEW_ICON));
                                    painter.extend(icons::shapes(view.icon, at, ink));
                                })
                            }
                        })
                        .inner;
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, ready, view.label)
                    });
                    let keys = self.keymap.keys(view.command);
                    if widgets::tip(response, view.tip, keys.as_ref(), None).clicked() {
                        ed.fit = Some(view.fit);
                    }
                }
                // Pop out is an icon at every width, as the note editor's.
                let keys = self.keymap.keys(Command::PopOutEditor);
                let (name, tip, glyph): (&str, &str, fn(&egui::Painter, egui::Pos2, Color32)) =
                    if popped {
                        (
                            "Dock",
                            "Put the editor back in the detail panel",
                            crate::note_editor::glyphs::dock,
                        )
                    } else {
                        (
                            "Pop out",
                            "Open the editor in its own window",
                            crate::note_editor::glyphs::pop_out,
                        )
                    };
                let response = raised::icon_button(ui, name, glyph);
                if widgets::tip(response, tip, keys.as_ref(), None).clicked() {
                    pop = true;
                }
            }
            if layout.shows_pills() {
                // Right to left: Pitch, Pan, then Gain at its left.
                for kind in Kind::ALL.into_iter().rev() {
                    let pill = pill_of(kind);
                    let on = ed.pill == Some(kind);
                    let id = ui.id().with(("audio editor pill", pill.label));
                    let response = widgets::pill_switch(ui, id, pill.label, pill.name, on);
                    let keys = self.keymap.keys(pill.command);
                    if widgets::tip(response, pill.tip, keys.as_ref(), None).clicked() {
                        ed.toggle_pill(kind);
                    }
                }
            }
        };
        let mut strip = HeaderStrip::new(ui, rect, right);
        let (chip, tool_words) = match layout {
            Layout::Words | Layout::Icons => {
                strip.title(&clip.name, &caption, colour);
                (GridChip::Full, true)
            }
            Layout::Compact | Layout::More => {
                strip.title_short(&clip.name, colour);
                (GridChip::Value, false)
            }
            Layout::Least => {
                strip.title_bar(&clip.name, colour);
                (GridChip::Icon, false)
            }
        };
        let tips = [
            "Pick a range, place the edit cursor, and move the envelope's points",
            "Draw the envelope shown (Gain when none is)",
        ];
        if let Some(tool) = strip.tools_with(self, ed.tool, tips, tool_words) {
            ed.choose_tool(tool);
        }
        self.header_grid_chip(&mut strip.row, ed.time, chip);
        if shows_views && self.zero_crossings_toggle(&mut strip.row, layout == Layout::Words) {
            flip_crossings = true;
        }
        if shows_views && self.loop_clip_toggle(&mut strip.row, layout == Layout::Words, looping) {
            flip_loop = true;
        }
        if flip_crossings {
            let editing = &mut self.settings.editing;
            editing.zero_crossings = !editing.zero_crossings;
            self.settings_changed = Some(std::time::Instant::now());
        }
        // Run after the strip is drawn: the toggles act on the app.
        if flip_loop {
            self.audio_editor.loop_clip = ed.loop_clip;
            self.toggle_loop_clip();
            ed.loop_clip = self.audio_editor.loop_clip;
        }
        if pop {
            self.detail.audio_popped = !self.detail.audio_popped;
        }
    }

    /// The Loop clip toggle, with its word or as an icon. True when clicked.
    fn loop_clip_toggle(&self, row: &mut Ui, words: bool, on: bool) -> bool {
        let response = if words {
            let mut shown = on;
            widgets::toggle_button(row, &mut shown, LOOP_CLIP)
        } else {
            icon_toggle(row, LOOP_CLIP, on, loop_glyph)
        };
        let keys = self.keymap.keys(Command::AudioEditorLoopClip);
        widgets::tip(
            response,
            "Play this clip on repeat while you edit it",
            keys.as_ref(),
            Some("Repeats one pass of the clip, or the range picked on it, with the other tracks heard. The song's own loop is not changed"),
        )
        .clicked()
    }

    /// The Zero crossings toggle, with its word or as an icon. True when
    /// clicked.
    fn zero_crossings_toggle(&self, row: &mut Ui, words: bool) -> bool {
        let on = self.settings.editing.zero_crossings;
        let response = if words {
            let mut shown = on;
            widgets::toggle_button(row, &mut shown, ZERO_CROSSINGS)
        } else {
            icon_toggle(row, ZERO_CROSSINGS, on, crossing_glyph)
        };
        let keys = self.keymap.keys(Command::AudioEditorZeroCrossings);
        widgets::tip(
            response,
            "Snap edges to zero crossings",
            keys.as_ref(),
            Some("A trim, range or cursor edge moves within 5 ms to where the audio is nearest silence. Alt places it freely"),
        )
        .clicked()
    }

    /// What the header's parts measure this frame.
    fn header_widths(
        &self,
        ui: &Ui,
        rect: Rect,
        clip: &Clip,
        caption: &str,
        ed: &AudioEditor,
    ) -> Widths {
        let (grid, grid_value, grid_icon) = self.grid_chip_widths(ui, ed.time);
        let word = |label: &str| raised::button_width(ui, label);
        let pill = |label: &str| {
            ui.painter()
                .layout_no_wrap(
                    label.to_owned(),
                    egui::FontId::proportional(theme::size::BODY),
                    Color32::PLACEHOLDER,
                )
                .size()
                .x
                + 2.0 * theme::space::M
        };
        Widths {
            room: rect.width() - 2.0 * theme::space::M,
            title: HeaderStrip::title_width(ui, &clip.name, caption),
            title_short: HeaderStrip::title_short_width(),
            bar: HeaderStrip::title_bar_width(),
            tools: HeaderStrip::tools_width(ui, true),
            tools_icons: HeaderStrip::tools_width(ui, false),
            grid,
            grid_value,
            grid_icon,
            toggles_words: widgets::toggle_button_width(ui, ZERO_CROSSINGS)
                + theme::space::S
                + widgets::toggle_button_width(ui, LOOP_CLIP),
            toggles_icons: 2.0 * theme::size::CONTROL + theme::space::S,
            pills: PILLS.iter().map(|p| pill(p.label)).sum::<f32>()
                + (PILLS.len() - 1) as f32 * theme::space::S,
            // Pop out is an icon at every width.
            view_words: word(VIEWS[0].label)
                + theme::space::S
                + word(VIEWS[1].label)
                + theme::space::S
                + theme::size::CONTROL,
            view_icons: 3.0 * theme::size::CONTROL + 2.0 * theme::space::S,
            more: theme::size::CONTROL,
        }
    }

    /// "4 bars · 0:08.9 file": the bars the clip spans and its file's length.
    fn audio_caption(&self, clip: &Clip, file: Option<&WholeFile>) -> String {
        let meter = &self.transport.timeline.meter;
        let first = meter.bbt_at(clip.start);
        let end = meter.bbt_at(clip.start + clip.length);
        let bars =
            (end.bar - first.bar + i64::from(end.beat > 0 || end.ticks > Ticks::ZERO)).max(1);
        let plural = if bars == 1 { "" } else { "s" };
        let rate = f64::from(self.lanes.rate().max(1));
        match file.and_then(|f| f.spans.last().map(|s| (s, f))) {
            Some((last, _)) => {
                let frames = last.first_frame + (last.end - last.start) * last.frames_per_beat;
                format!(
                    "{bars} bar{plural} \u{00B7} {} file",
                    paint::seconds_text(frames / rate, 0.1)
                )
            }
            None => format!("{bars} bar{plural}"),
        }
    }
}
