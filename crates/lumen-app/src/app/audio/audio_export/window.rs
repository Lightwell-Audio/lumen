//! Export uses the shared floating window, rows, dropdowns and switches.

use eframe::egui::{self, Align2, Id, RichText, pos2};

use crate::export::{Buses, Format, Options, Scope};
use crate::widgets::{self, Dialog, Dropdown, raised};
use crate::widgets::{hint, khz, row, section};
use crate::{App, theme};

const WIDTH: f32 = 420.0;
const TOP: f32 = 64.0;
const CHROME: f32 = 150.0;
const MIN_CONTENT: f32 = 100.0;

fn choose<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    name: &str,
    value: &mut T,
    choices: &[(T, &str)],
) {
    let at = choices.iter().position(|(v, _)| v == value);
    let labels: Vec<&str> = choices.iter().map(|(_, label)| *label).collect();
    let label = if name == "Export sample rate" {
        "Sample rate"
    } else {
        name
    };
    row(ui, label, |ui| {
        let shown = at.map_or("Project rate", |i| labels[i]);
        if let Some(i) =
            Dropdown::new(name, shown, 240.0).show(ui, Id::new(("audio export", name)), &labels, at)
        {
            *value = choices[i].0;
        }
    });
}

fn choices(ui: &mut egui::Ui, options: &mut Options) {
    section(ui, "Range and output");
    choose(
        ui,
        "Range",
        &mut options.scope,
        &[
            (Scope::Project, "Whole project"),
            (Scope::Loop, "Loop region"),
            (Scope::Selection, "Time selection"),
        ],
    );
    choose(
        ui,
        "Output",
        &mut options.buses,
        &[
            (Buses::Master, "Master mix"),
            (Buses::Tracks, "Track stems"),
            (Buses::Groups, "Group stems"),
            (Buses::Returns, "Return stems"),
        ],
    );
    hint(
        ui,
        if options.buses == Buses::Master {
            "The complete master mix, including groups, sends and master processing."
        } else {
            "Separate stereo files at each bus's output, with its devices and fader. Sends leaving that bus are separate."
        },
    );

    section(ui, "Audio file");
    choose(
        ui,
        "File format",
        &mut options.format,
        &[
            (Format::Wav16, "WAV · 16-bit"),
            (Format::Wav24, "WAV · 24-bit"),
            (Format::WavFloat, "WAV · 32-bit float"),
            (Format::Flac16, "FLAC · 16-bit"),
            (Format::Flac24, "FLAC · 24-bit"),
        ],
    );
    let rates = crate::document::RATES;
    let names: Vec<String> = rates.iter().map(|r| khz(*r)).collect();
    let values: Vec<(u32, &str)> = rates
        .iter()
        .zip(&names)
        .map(|(&r, n)| (r, n.as_str()))
        .collect();
    choose(ui, "Export sample rate", &mut options.rate, &values);
    widgets::switch(ui, &mut options.normalize, "Normalize peak to −1 dBFS", true)
        .on_hover_text("Adjusts the file's highest sample to −1 dBFS. Each stem is normalized separately; silence stays silent.");
    let integer = options.format != Format::WavFloat;
    widgets::switch(ui, &mut options.dither, "TPDF dither", integer).on_hover_text(
        "Adds triangular dither at the final integer bit depth. Float files are never dithered.",
    );

    section(ui, "Rendering");
    choose(
        ui,
        "Render mode",
        &mut options.realtime,
        &[(false, "Offline"), (true, "Real-time")],
    );
    hint(
        ui,
        if options.realtime {
            "Plays an export snapshot through the audio device. Monitoring, live MIDI and the metronome are excluded. Playback stops when export begins."
        } else {
            "Renders in the background as fast as processing allows."
        },
    );
    widgets::switch(ui, &mut options.tail, "Include effect tails", true)
        .on_hover_text("Keeps the sound ringing after the range, until quiet or 10 seconds.");
}

fn db(value: f32) -> String {
    if value > 0.0 {
        format!("{:.1}", 20.0 * value.log10())
    } else {
        "−∞".into()
    }
}

impl App {
    pub(crate) fn audio_export_window(&mut self, ctx: &egui::Context) {
        if !self.export.open {
            return;
        }
        let mut options = self.export.options;
        let busy = self.export.busy();
        let unavailable = self.export_unavailable(options);
        let mut start = false;
        let mut stop = false;
        let room = (ctx.content_rect().height() - TOP - CHROME).max(MIN_CONTENT);
        let close = Dialog::new("audio export", "Export audio", WIDTH)
            .at(pos2(ctx.content_rect().center().x, TOP), Align2::CENTER_TOP)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(room)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add_enabled_ui(!busy, |ui| choices(ui, &mut options));
                        if let Some(why) = &self.export.error {
                            section(ui, "Could not export");
                            ui.label(why);
                        }
                        if !self.export.reports.is_empty() {
                            section(ui, "Export complete");
                            for report in &self.export.reports {
                                let name = report
                                    .path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy();
                                ui.label(RichText::new(name.as_ref()).strong())
                                    .on_hover_text(report.path.display().to_string());
                                let loudness = if report.lufs.is_finite() {
                                    format!("{:.1} LUFS", report.lufs)
                                } else {
                                    "No integrated reading (silent or under 400 ms)".into()
                                };
                                hint(
                                    ui,
                                    &format!(
                                        "{loudness} · {} dBTP · {:.2} s",
                                        db(report.true_peak),
                                        report.frames as f64 / f64::from(report.rate)
                                    ),
                                );
                            }
                        }
                    });
                ui.add_space(theme::space::M);
                if busy {
                    let p = self.export.progress.unwrap_or(0.0);
                    ui.label(format!("Exporting {}%", (p * 100.0) as u32));
                    stop = raised::button(ui, "Cancel export").clicked();
                } else {
                    if let Some(why) = unavailable {
                        hint(ui, why);
                    }
                    let response = ui
                        .add_enabled_ui(unavailable.is_none(), |ui| {
                            raised::primary_button(ui, "Export audio…")
                        })
                        .inner;
                    start = response
                        .on_hover_text("Choose a destination and export; stems use a folder")
                        .clicked();
                }
            });
        self.export.options = options;
        if std::mem::take(&mut self.export.focus) {
            ctx.memory_mut(|m| m.request_focus(Id::new(("audio export", "Range"))));
        }
        if stop || (close && busy) {
            self.cancel_audio_export();
        }
        if close {
            self.export.open = false;
        }
        if start {
            self.choose_export_destination(ctx);
        }
    }
}
