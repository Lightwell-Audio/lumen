//! The audio editor's header (M9.5 D24): the clip editors' shared strip, in
//! the first of five layouts that fits. A control that gives up its word
//! keeps it in its tooltip and accessible name; what moves into the ⋯ menu is
//! still there, with its keys.

use eframe::egui::{self, Color32, Rect, Ui, vec2};
use lumen_core::time::Ticks;
use lumen_project::model::Clip;

use crate::App;
use crate::app::clip_header::{GridChip, HeaderStrip};
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
        let folded = ed.folded;
        let shows_views = matches!(layout, Layout::Words | Layout::Icons | Layout::Compact);
        let has_menu = folded || !shows_views;
        let ctx = ui.ctx().clone();
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
                        // Left to right as they are in the strip.
                        for view in VIEWS.iter().rev() {
                            let keys = self.keymap.keys(view.command);
                            if widgets::menu_item(ui, view.label, keys.as_ref(), ready).clicked() {
                                ed.fit = Some(view.fit);
                                ui.close();
                            }
                        }
                    }
                    if folded {
                        let keys = self.keymap.keys(Command::ClipProperties);
                        if widgets::menu_item(ui, "Clip properties", keys.as_ref(), true).clicked()
                        {
                            self.select_opened_clip(clip.id);
                            self.run(&ctx, Command::ClipProperties);
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
            }
        };
        let mut strip = HeaderStrip::new(ui, rect, right);
        let chip = match layout {
            Layout::Words | Layout::Icons => {
                strip.title(&clip.name, &caption, colour);
                GridChip::Full
            }
            Layout::Compact | Layout::More => {
                strip.title_short(&clip.name, colour);
                GridChip::Value
            }
            Layout::Least => {
                strip.title_bar(&clip.name, colour);
                GridChip::Icon
            }
        };
        self.header_grid_chip(&mut strip.row, ed.time, chip);
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
        Widths {
            room: rect.width() - 2.0 * theme::space::M,
            title: HeaderStrip::title_width(ui, &clip.name, caption),
            title_short: HeaderStrip::title_short_width(),
            bar: HeaderStrip::title_bar_width(),
            grid,
            grid_value,
            grid_icon,
            view_words: word(VIEWS[0].label) + theme::space::S + word(VIEWS[1].label),
            view_icons: 2.0 * theme::size::CONTROL + theme::space::S,
            more: theme::size::CONTROL,
            always_more: ed.folded,
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
