//! A strip's output: the button that says where it plays to, and the menu
//! that chooses (M4 slice 5, the owner's B6: groups, returns, the master
//! and the device's pairs; choices that would feed back are never
//! offered). Split from `strip.rs` (M4 review M1).

use eframe::egui::{self, Align2, FontId, Id as EguiId, Rect, Sense, Ui, pos2};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{Id, Output, Project, Track, TrackKind};
use lumen_project::routing::{self as rules, Destination};

use super::Arrangement;
use crate::document::Document;
use crate::theme::Tone;
use crate::widgets::{self, raised};
use crate::{motion, theme};
use lumen_core::product::PRODUCT_NAME;

/// Where `track` plays to, in words: a group's or return's name, "Master",
/// or a pair of outputs ("1/2").
fn destination_text(project: &Project, track: &Track) -> String {
    match rules::destination(project, track) {
        Destination::Track(id) if id == project.master.id => "Master".into(),
        Destination::Track(id) => project
            .tracks
            .iter()
            .find(|t| t.id == id)
            .map_or_else(|| "Master".into(), |t| t.name.clone()),
        Destination::Device(pair) => format!("{}/{}", 2 * pair + 1, 2 * pair + 2),
    }
}

/// Whether sending track `id` to `output` would feed the audio back into
/// itself: such choices are never offered (risk R4).
fn would_loop(project: &Project, id: Id, output: Output) -> bool {
    let mut trial = project.clone();
    if let Some(track) = trial.tracks.iter_mut().find(|t| t.id == id) {
        std::sync::Arc::make_mut(track).output = output;
    }
    rules::has_loop(&trial)
}

/// A strip's row that opens a menu (the output, and a MIDI track's From and
/// To): `text` shortened to fit beside an arrow, named `name`, with `tip` on
/// hover. `part` names the row, to tell the rows of one strip apart.
pub(super) fn row_button(
    ui: &mut Ui,
    row: Rect,
    id: EguiId,
    part: &str,
    text: &str,
    name: &str,
    tip: &str,
) -> egui::Response {
    let palette = *theme::p();
    let response = ui
        .interact(row, id.with(part), Sense::click())
        .on_hover_text(tip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, name));
    let hover = motion::hover(ui.ctx(), id.with((part, "hover")), response.hovered());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    // A raised combo (ADR-0052), on while its menu is open.
    let face = raised::Face::new(theme::controls(false))
        .hover(hover)
        .pressed(response.is_pointer_button_down_on())
        .lit(
            motion::hover(ui.ctx(), id.with((part, "open")), open),
            Tone::On,
        );
    let row = row.round_to_pixels(ui.pixels_per_point());
    let body = raised::paint(
        ui.painter(),
        row,
        theme::radius::for_height(row.height()),
        &face,
    );
    let painter = ui.painter();
    let font = FontId::proportional(theme::size::CAPTION);
    let room = row.width() - 20.0;
    let shown = if painter
        .layout_no_wrap(text.to_owned(), font.clone(), palette.text)
        .size()
        .x
        > room
    {
        widgets::shorten(painter, text, font.clone(), room)
    } else {
        text.to_owned()
    };
    painter.text(
        pos2(row.left() + 5.0, body.center().y),
        Align2::LEFT_CENTER,
        shown,
        font,
        palette
            .text_secondary
            .lerp_to_gamma(theme::lit(Tone::On).text, face.lit),
    );
    widgets::draw_arrow(
        painter,
        pos2(row.right() - 8.0, body.center().y),
        1.0,
        palette.text_hint,
    );
    raised::focus_ring(painter, &response, row);
    response
}

impl Arrangement {
    /// Where the track plays to; a click opens the choices.
    pub(super) fn output_button(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        row: Rect,
        id: EguiId,
    ) {
        let text = destination_text(doc.project(), track);
        let name = format!("{} output in the mixer: {text}", track.name);
        let response = row_button(
            ui,
            row,
            id,
            "output",
            &text,
            &name,
            "Where this track plays to",
        );
        widgets::Menu::below(&response).show(|ui| {
            ui.set_min_width(160.0);
            self.output_menu(ui, doc, track);
        });
    }

    /// The choices of where `track` plays to: inside Lumen, the master and
    /// the groups that would not feed back; then the device's output pairs
    /// (the owner's B6), the main pair marked, the cue pair greyed.
    fn output_menu(&mut self, ui: &mut Ui, doc: &mut Document, track: &Track) {
        let palette = *theme::p();
        let project = doc.project().clone();
        let current = rules::destination(&project, track);
        let master = track.kind == TrackKind::Master;
        let heading = |ui: &mut Ui, text: &str| {
            ui.label(egui::RichText::new(text).small().color(palette.text_hint));
        };
        let mut picked = None;
        let mut choose = |ui: &mut Ui, label: &str, on: bool, output: Output, tip: &str| {
            let item = ui.selectable_label(on, label);
            let item = if tip.is_empty() {
                item
            } else {
                item.on_hover_text(tip)
            };
            if item.clicked() {
                picked = Some(output);
                ui.close();
            }
        };
        if !master {
            heading(ui, &format!("Inside {PRODUCT_NAME}"));
            choose(
                ui,
                "Master",
                current == Destination::Track(project.master.id),
                Output::Master,
                "",
            );
            for target in project.tracks.iter().filter(|t| {
                t.kind == TrackKind::Group
                    && t.id != track.id
                    && !would_loop(&project, track.id, Output::Track(t.id))
            }) {
                choose(
                    ui,
                    &target.name,
                    current == Destination::Track(target.id),
                    Output::Track(target.id),
                    "",
                );
            }
            ui.separator();
        }
        heading(ui, "Audio outputs");
        // The device's outputs are looked up while a menu wants them.
        self.outputs.wanted = true;
        if let Some(status) = &self.outputs.status {
            ui.label(egui::RichText::new(status).small().color(palette.text_hint));
        }
        let skips = if master { "" } else { "Skips the master" };
        for (pair, names) in self.outputs.pairs.clone() {
            let single = !names.contains('\u{00B7}');
            let label = if single {
                format!("{}", 2 * pair + 1)
            } else {
                format!("{}/{}", 2 * pair + 1, 2 * pair + 2)
            };
            let main = self.outputs.main == Some(pair);
            if self.outputs.cue == Some(pair) {
                ui.add_enabled(
                    false,
                    egui::Button::selectable(false, format!("{label} Cue")),
                )
                .on_disabled_hover_text(
                    "Used for cue: choose another Cue output in Audio settings to use it here",
                );
                continue;
            }
            let on = match track.output {
                Output::Device(p) => p == pair,
                Output::Auto => master && main,
                _ => false,
            };
            let text = if main { format!("{label} Main") } else { label };
            // The master on the main pair follows it, whichever pair the
            // audio settings make main.
            let output = if master && main {
                Output::Auto
            } else {
                Output::Device(pair)
            };
            let tip = if skips.is_empty() {
                names
            } else {
                format!("{names} \u{00B7} {skips}")
            };
            choose(ui, &text, on, output, &tip);
        }
        if let Some(output) = picked
            && let Err(refused) = doc.set_output(track.id, output)
        {
            self.note = Some(refused.reason());
        }
    }
}
