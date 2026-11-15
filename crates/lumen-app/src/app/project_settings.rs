use eframe::egui::{self, Align2, Color32, Id, RichText, WidgetInfo, WidgetType, pos2};
use lumen_core::pan::PanLaw;
use lumen_project::model::{Color, Project};

use crate::app::clip_card::{self, Swatches};
use crate::commands::Command;
use crate::document::RATES;
use crate::widgets::{self, Dialog, Dropdown};
use crate::widgets::{hint, khz, row, section};
use crate::{App, theme};

const WIDTH: f32 = 380.0;
const TOP: f32 = 96.0;
const CHROME: f32 = 88.0;
const NOTES: f32 = 150.0;
const MOST_SHRUNK: f32 = 120.0;

#[derive(Default)]
pub(crate) struct ProjectSettings {
    open: bool,
    pub(crate) editing: bool,
}

impl ProjectSettings {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }
}

#[derive(Default)]
struct Picked {
    rate: Option<u32>,
    law: Option<PanLaw>,
    crossfades: bool,
    color: Option<Option<Color>>,
}

fn sections(ui: &mut egui::Ui, project: &Project, device_rate: u32, next: Color32) -> Picked {
    let mut picked = Picked::default();
    section(ui, "Audio");
    row(ui, "Sample rate", |ui| {
        let labels: Vec<String> = RATES.iter().map(|&r| khz(r)).collect();
        let index = RATES.iter().position(|&r| r == project.sample_rate);
        let shown = khz(project.sample_rate);
        picked.rate = Dropdown::new("Sample rate", &shown, 120.0)
            .show(ui, Id::new("project rate"), &labels, index)
            .map(|i| RATES[i]);
    });
    hint(
        ui,
        "Joined, bounced and frozen audio is made at this rate. Playback and recording \
         follow the audio device.",
    );
    if device_rate != project.sample_rate && device_rate != 0 {
        ui.label(
            RichText::new(format!(
                "The audio device runs at {}, so audio made at this rate is converted to play.",
                khz(device_rate)
            ))
            .small()
            .color(theme::p().text_secondary),
        );
    }

    section(ui, "Mixing");
    row(ui, "Pan law", |ui| {
        let names: Vec<&str> = PanLaw::ALL.iter().map(|l| l.name()).collect();
        let index = PanLaw::ALL.iter().position(|&l| l == project.pan_law);
        picked.law = Dropdown::new("Pan law", project.pan_law.name(), 200.0)
            .show(ui, Id::new("project pan law"), &names, index)
            .map(|i| PanLaw::ALL[i]);
    });
    hint(
        ui,
        "How loud a mono track is at the center. Stereo tracks balance.",
    );
    ui.add_space(theme::space::XS);
    let mut crossfades = project.auto_crossfade;
    picked.crossfades = widgets::switch(ui, &mut crossfades, "Automatic crossfades", true)
        .on_hover_text("Automatically blend eligible audio joins in this project")
        .changed();

    section(ui, "New tracks");
    row(ui, "Color", |ui| {
        let look = Swatches {
            name: "New track color",
            round: true,
            none: (next, "Colors in turn"),
        };
        picked.color = clip_card::swatches(ui, project.new_track_color, look);
    });
    hint(ui, "New tracks take this color, or the palette in turn.");
    picked
}

impl App {
    pub(crate) fn open_project_settings(&mut self) {
        self.project_settings.open = true;
    }

    pub(crate) fn project_settings_window(&mut self, ctx: &egui::Context) {
        if !self.project_settings.open {
            self.project_settings.editing = false;
            return;
        }
        let device_rate = self.transport.state().sample_rate;
        let project = self.document.project();
        let next = theme::CLIP_COLORS[project.tracks.len() % theme::CLIP_COLORS.len()];
        let mut notes = project.notes.clone();
        let name = self.document.name();
        let (mut picked, mut typing, mut notes_changed) = (Picked::default(), false, false);
        let room = (ctx.content_rect().height() - TOP - CHROME - NOTES).max(MOST_SHRUNK);
        let close = Dialog::new("project settings", "Project settings", WIDTH)
            .at(pos2(ctx.content_rect().center().x, TOP), Align2::CENTER_TOP)
            .show(ctx, |ui| {
                ui.label(RichText::new(format!("Settings of {name}")).small().weak());
                egui::ScrollArea::vertical()
                    .max_height(room)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        picked = sections(ui, project, device_rate, next);
                    });

                section(ui, "Notes");
                let field = ui.add(
                    egui::TextEdit::multiline(&mut notes)
                        .id(Id::new("project notes"))
                        .hint_text("Notes about this project")
                        .desired_rows(5)
                        .desired_width(f32::INFINITY),
                );
                field.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::TextEdit, true, "Project notes")
                });
                typing = field.has_focus();
                notes_changed = field.changed();
                hint(ui, "Kept in the project file.");
            });

        self.project_settings.editing = typing;
        if let Some(rate) = picked.rate {
            self.document.set_sample_rate(rate);
        }
        if let Some(law) = picked.law {
            self.document.set_pan_law(law);
        }
        if picked.crossfades {
            self.run(ctx, Command::ToggleAutoCrossfade);
        }
        if let Some(color) = picked.color {
            self.document.set_new_track_color(color);
        }
        if notes_changed {
            self.document.set_notes(&notes);
        }
        if close {
            self.project_settings.open = false;
            self.project_settings.editing = false;
        }
    }
}
