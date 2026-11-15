use eframe::egui::{
    self, Align2, CursorIcon, FontId, Id as EguiId, Rangef, Rect, Sense, Ui, pos2, vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{AudioInput, Id, Monitor, Track, TrackKind};

use super::Arrangement;
use super::strip_output::row_button;
use crate::document::Document;
use crate::meters::{FLOOR_DB, Reading};
use crate::recording::audio_input;
use crate::theme::Tone;
use crate::widgets::raised;
use crate::{light, motion, theme, widgets};

const BAR: f32 = 1.5;
const BAR_INSET: f32 = 1.5;
const TEXT_PAD: f32 = theme::space::XS;
const VALUE_GAP: f32 = theme::space::XS;
const CARET: egui::Vec2 = vec2(5.0, 3.0);

#[derive(Debug, Clone, Default)]
pub(crate) struct InputChoices {
    pub open: Vec<usize>,
    pub names: Vec<String>,
    pub status: Option<String>,
    pub wanted: bool,
}

impl InputChoices {
    pub fn name(&self, channel: u16) -> String {
        match self.names.get(usize::from(channel)) {
            Some(name) if !name.trim().is_empty() => name.clone(),
            _ => format!("Input {}", channel + 1),
        }
    }

    pub fn pairs(&self) -> Vec<u16> {
        audio_input::pairs(&self.open)
    }

    pub fn has(&self, input: AudioInput) -> bool {
        audio_input::place(input, &self.open).is_some()
    }
}

pub(super) fn chip_text(input: Option<AudioInput>) -> String {
    match input {
        None => "None".to_owned(),
        Some(i) if i.stereo => format!("{}+{}", i.channel + 1, i.channel + 2),
        Some(i) => (i.channel + 1).to_string(),
    }
}

pub(super) fn long_text(input: Option<AudioInput>, choices: &InputChoices) -> String {
    match input {
        None => "None".to_owned(),
        Some(i) if i.stereo => format!(
            "{} + {}",
            choices.name(i.channel),
            choices.name(i.channel + 1)
        ),
        Some(i) => choices.name(i.channel),
    }
}

fn problem(track: &Track, choices: &InputChoices) -> Option<&'static str> {
    audio_input::unavailable(track, &choices.open)
}

pub(super) fn menu(
    ui: &mut Ui,
    doc: &mut Document,
    audio: &[Id],
    current: Option<AudioInput>,
    choices: &mut InputChoices,
) {
    choices.wanted = true;
    let set = |doc: &mut Document, ui: &mut Ui, input: Option<AudioInput>| {
        doc.set_audio_input(audio, input);
        ui.close();
    };
    if widgets::menu_toggle(ui, "None", None, current.is_none()).clicked() {
        set(doc, ui, None);
    }
    if choices.open.is_empty() {
        ui.separator();
        widgets::menu_item(ui, "The audio device has no inputs open", None, false);
        return;
    }
    if let Some(i) = current.filter(|&i| !choices.has(i)) {
        widgets::menu_toggle(
            ui,
            &format!("{} (not on this device)", long_text(Some(i), choices)),
            None,
            true,
        );
    }
    let pairs = choices.pairs();
    if !pairs.is_empty() {
        ui.separator();
        widgets::menu_caption(ui, "Stereo");
        for channel in pairs {
            let input = AudioInput::pair(channel);
            let text = format!(
                "{}  {}",
                chip_text(Some(input)),
                long_text(Some(input), choices)
            );
            if widgets::menu_toggle(ui, &text, None, current == Some(input)).clicked() {
                set(doc, ui, Some(input));
            }
        }
    }
    ui.separator();
    widgets::menu_caption(ui, "Mono");
    for channel in choices.open.clone() {
        let input = AudioInput::mono(channel as u16);
        let text = format!(
            "{}  {}",
            chip_text(Some(input)),
            long_text(Some(input), choices)
        );
        if widgets::menu_toggle(ui, &text, None, current == Some(input)).clicked() {
            set(doc, ui, Some(input));
        }
    }
    if let Some(why) = &choices.status {
        ui.separator();
        widgets::menu_caption(ui, why);
    }
}

fn monitor_row(ui: &mut Ui, doc: &mut Document, ids: &[Id], current: Monitor) {
    super::midi_io::monitor_row(ui, doc, ids, current);
}

fn reading(arrangement: &Arrangement, input: AudioInput) -> Reading {
    let first = arrangement.meters.input_reading(usize::from(input.channel));
    if !input.stereo {
        return first;
    }
    let second = arrangement
        .meters
        .input_reading(usize::from(input.channel) + 1);
    Reading {
        peak: [first.peak[0], second.peak[0]],
        rms: [first.rms[0], second.rms[0]],
        hold: [first.hold[0], second.hold[0]],
        clipped: first.clipped || second.clipped,
    }
}

fn x_at(span: Rangef, db: f32) -> f32 {
    span.min + (db.clamp(FLOOR_DB, 0.0) - FLOOR_DB) / -FLOOR_DB * span.span()
}

fn draw_level(ui: &Ui, body: Rect, reading: &Reading, base: egui::Color32) {
    if reading.at_rest() {
        return;
    }
    let span = Rangef::new(body.left() + 3.0, body.right() - 3.0);
    let ys = Rangef::new(body.bottom() - BAR_INSET - BAR, body.bottom() - BAR_INSET);
    let painter = ui.painter_at(body);
    let peak = reading.peak[0].max(reading.peak[1]);
    let rms = reading.rms[0].max(reading.rms[1]);
    for (db, alpha) in [(peak, theme::METER_PEAK_ALPHA), (rms, 1.0)] {
        for (from, to, color) in widgets::meter::zones(db, base) {
            let rect = Rect::from_x_y_ranges(x_at(span, from)..=x_at(span, to), ys);
            painter.rect_filled(rect, 0.0, color.gamma_multiply(alpha));
        }
    }
}

impl Arrangement {
    pub(super) fn input_chip(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        rect: Rect,
        id: EguiId,
    ) {
        let palette = *theme::p();
        let trouble = problem(track, &self.inputs);
        let words = long_text(track.input, &self.inputs);
        let name = format!("{} input: {words}", track.name);
        let tip = match (trouble, track.input) {
            (Some(why), _) => format!("Input: {words} \u{b7} {why}"),
            (None, Some(_)) => format!("Input: {words}"),
            (None, None) => "Input: none \u{b7} choose what this track records".to_owned(),
        };
        let response = ui.interact(rect, id.with("input chip"), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, name.clone())
        });
        let response = widgets::tip(response, &tip, None, Some("Click to choose"));
        let hover = motion::hover(ui.ctx(), id.with("input hover"), response.hovered());
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        let lit = motion::hover(ui.ctx(), id.with("input open"), open);
        let face = raised::Face::new(theme::controls(false))
            .hover(hover)
            .pressed(response.is_pointer_button_down_on())
            .lit(lit, Tone::On);
        let rect = rect.round_to_pixels(ui.pixels_per_point());
        let body = raised::paint(
            ui.painter(),
            rect,
            theme::radius::for_height(rect.height()),
            &face,
        );
        let ink = if trouble.is_some() {
            palette.warning
        } else if track.input.is_some() {
            palette
                .text
                .lerp_to_gamma(theme::lit(Tone::On).text, face.lit)
        } else {
            palette.text_hint
        };
        let font = FontId::proportional(theme::size::CAPTION);
        let painter = ui.painter();
        let label = painter.text(
            pos2(body.left() + TEXT_PAD, body.center().y),
            Align2::LEFT_CENTER,
            "In",
            font.clone(),
            palette.text_hint,
        );
        let caret = pos2(body.right() - TEXT_PAD - CARET.x / 2.0, body.center().y);
        painter.add(egui::Shape::convex_polygon(
            vec![
                caret + vec2(-CARET.x / 2.0, -CARET.y / 2.0),
                caret + vec2(CARET.x / 2.0, -CARET.y / 2.0),
                caret + vec2(0.0, CARET.y / 2.0),
            ],
            if hover > 0.5 || open {
                palette.text_secondary
            } else {
                palette.text_hint
            },
            egui::Stroke::NONE,
        ));
        let value = match trouble {
            Some(_) => format!("\u{25B2}{}", chip_text(track.input)),
            None => chip_text(track.input),
        };
        let from = label.right() + VALUE_GAP;
        let room = caret.x - CARET.x / 2.0 - VALUE_GAP - from;
        painter.text(
            pos2(from, body.center().y),
            Align2::LEFT_CENTER,
            widgets::fit(painter, &value, font.clone(), room),
            font,
            ink,
        );
        raised::focus_ring(ui.painter(), &response, rect);
        let reading = match (track.input, trouble) {
            (Some(input), None) => Some(reading(self, input)),
            _ => None,
        };
        if let Some(reading) = &reading {
            draw_level(ui, body, reading, palette.accent);
            if reading.clipped {
                self.clip_pip(ui, body, track, id);
            }
        }
        let audio = self.audio_targets(doc, track.id);
        let (input, monitor) = (track.input, track.monitor);
        let mut choices = std::mem::take(&mut self.inputs);
        widgets::Menu::below(&response).show(|ui| {
            ui.set_min_width(180.0);
            menu(ui, doc, &audio, input, &mut choices);
            ui.separator();
            monitor_row(ui, doc, &audio, monitor);
        });
        self.inputs = choices;
    }

    fn clip_pip(&mut self, ui: &mut Ui, body: Rect, track: &Track, id: EguiId) {
        let palette = theme::p();
        let mark =
            Rect::from_center_size(pos2(body.right() - 5.0, body.top() + 5.0), vec2(4.0, 4.0));
        light::halo_around(ui.painter(), light::FLARE, mark, 1.0, palette.record);
        ui.painter().rect_filled(mark, 1.0, palette.record);
        let area = mark.expand(3.0);
        let response = ui
            .interact(area, id.with("input clip"), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        let name = format!("Clear the input clip light of {}", track.name);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        if response.clicked() {
            if ui.input(|i| i.modifiers.alt) {
                self.meters.clear_all_clips();
            } else if let Some(input) = track.input {
                for channel in input.channels() {
                    self.meters.clear_input_clip(usize::from(channel));
                }
            }
        }
    }

    pub(super) fn audio_input_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        row: Rect,
        id: EguiId,
    ) {
        let text = long_text(track.input, &self.inputs);
        let name = format!("{} input in the mixer: {text}", track.name);
        let tip = problem(track, &self.inputs).unwrap_or("What this track records");
        let response = row_button(ui, row, id, "audio in", &format!("In  {text}"), &name, tip);
        if let Some(input) = track.input.filter(|&i| self.inputs.has(i)) {
            let level = reading(self, input);
            draw_level(ui, row, &level, theme::p().accent);
        }
        let audio = self.audio_targets(doc, track.id);
        let (input, monitor) = (track.input, track.monitor);
        let mut choices = std::mem::take(&mut self.inputs);
        widgets::Menu::below(&response).show(|ui| {
            ui.set_min_width(180.0);
            menu(ui, doc, &audio, input, &mut choices);
            ui.separator();
            monitor_row(ui, doc, &audio, monitor);
        });
        self.inputs = choices;
    }

    pub(super) fn audio_monitor_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        row: Rect,
        id: EguiId,
    ) {
        let word = match track.monitor {
            Monitor::In => "In",
            Monitor::Auto => "Auto",
            Monitor::Off => "Off",
        };
        let name = format!("{} monitor in the mixer: {word}", track.name);
        let response = row_button(
            ui,
            row,
            id,
            "audio monitor",
            &format!("Mon  {word}"),
            &name,
            "When you hear this track's input",
        );
        let audio = self.audio_targets(doc, track.id);
        let current = track.monitor;
        widgets::Menu::below(&response).show(|ui| {
            ui.set_min_width(160.0);
            monitor_row(ui, doc, &audio, current);
        });
    }

    pub(super) fn audio_targets(&self, doc: &Document, id: Id) -> Vec<Id> {
        let targets = self.targets(id);
        doc.project()
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio && targets.contains(&t.id))
            .map(|t| t.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices(open: &[usize]) -> InputChoices {
        InputChoices {
            open: open.to_vec(),
            ..InputChoices::default()
        }
    }

    #[test]
    fn the_chip_names_inputs_by_device_channel_number() {
        assert_eq!(chip_text(None), "None");
        assert_eq!(chip_text(Some(AudioInput::mono(2))), "3");
        assert_eq!(chip_text(Some(AudioInput::pair(0))), "1+2");
    }

    #[test]
    fn pairs_are_two_open_channels_side_by_side_starting_on_an_odd_number() {
        assert_eq!(choices(&[0, 1, 2, 3]).pairs(), vec![0, 2]);
        assert_eq!(choices(&[1, 2]).pairs(), Vec::<u16>::new());
        assert_eq!(choices(&[0, 1, 3]).pairs(), vec![0]);
        assert!(choices(&[]).pairs().is_empty());
    }

    #[test]
    fn a_channel_without_a_name_is_input_n() {
        let mut c = choices(&[0, 1]);
        assert_eq!(c.name(1), "Input 2");
        c.names = vec!["Mic".into(), " ".into()];
        assert_eq!(c.name(0), "Mic");
        assert_eq!(c.name(1), "Input 2");
        assert_eq!(long_text(Some(AudioInput::pair(0)), &c), "Mic + Input 2");
    }

    #[test]
    fn the_level_scale_runs_from_the_floor_to_full_scale() {
        let span = Rangef::new(0.0, 60.0);
        assert_eq!(x_at(span, FLOOR_DB), 0.0);
        assert_eq!(x_at(span, 0.0), 60.0);
        assert_eq!(x_at(span, 6.0), 60.0);
    }
}
