//! Where an audio track gets its input (M9 slice R1, the owner's R1 A, ADR-0100):
//! the In chip on the track header and the same choice as a row on the mixer
//! strip, and the menu both open. The chip names the input by its device
//! channels ("1", "1+2"), carries the input's level along its bottom and a
//! clip light that stays until clicked. Arming an unrouted track picks the
//! first free input (`Document::set_armed`), so most tracks never need the menu.

use eframe::egui::{
    self, Align2, CursorIcon, FontId, Id as EguiId, Rangef, Rect, Sense, Ui, pos2, vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{AudioInput, Id, Monitor, Track, TrackKind};

use super::Arrangement;
use super::strip_output::row_button;
use crate::audio_input;
use crate::document::Document;
use crate::meters::{FLOOR_DB, Reading};
use crate::theme::Tone;
use crate::widgets::raised;
use crate::{light, motion, theme, widgets};

/// The level bar along the chip's bottom, thin and low like the volume
/// field's, so the text sits exactly centred.
const BAR: f32 = 1.5;
const BAR_INSET: f32 = 1.5;

/// The chip's size on a header: as tall as the volume field beside it.
pub(super) fn chip_size() -> egui::Vec2 {
    vec2(40.0, 20.0)
}

/// The device's inputs, as the menus offer them, lent by the app each frame.
#[derive(Debug, Clone, Default)]
pub(crate) struct InputChoices {
    /// The device channels open as inputs, in the order the engine sees them;
    /// empty when the device is not running or has none.
    pub open: Vec<usize>,
    /// The device's input channel names (by device channel); empty until they
    /// are looked up, and for a device that does not name them.
    pub names: Vec<String>,
    /// Why the names are not known (a lookup that failed).
    pub status: Option<String>,
    /// A menu asked for the names: the app looks them up.
    pub wanted: bool,
}

impl InputChoices {
    /// Channel `channel`'s name: the device's, or "Input N".
    pub fn name(&self, channel: u16) -> String {
        match self.names.get(usize::from(channel)) {
            Some(name) if !name.trim().is_empty() => name.clone(),
            _ => format!("Input {}", channel + 1),
        }
    }

    /// The first channels of the pairs the device offers: two open channels
    /// side by side, the first even (1+2, 3+4, ...).
    pub fn pairs(&self) -> Vec<u16> {
        audio_input::pairs(&self.open)
    }

    /// Whether the device has `input` open.
    pub fn has(&self, input: AudioInput) -> bool {
        audio_input::place(input, &self.open).is_some()
    }
}

/// An input as the chip shows it: "In" for none, "3" or "1+2" by device
/// channel number.
pub(super) fn chip_text(input: Option<AudioInput>) -> String {
    match input {
        None => "In".to_owned(),
        Some(i) if i.stereo => format!("{}+{}", i.channel + 1, i.channel + 2),
        Some(i) => (i.channel + 1).to_string(),
    }
}

/// An input in words, for a strip, a tooltip and an accessible name.
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

/// Why `track`'s input cannot be heard now, if it cannot.
fn problem(track: &Track, choices: &InputChoices) -> Option<&'static str> {
    audio_input::unavailable(track, &choices.open)
}

/// The choices of what `audio` tracks listen to: none, each stereo pair and
/// each mono input of the device by name, and the input the tracks name when
/// the device does not have it.
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

/// The Monitor row of an audio track's menu: a caption and In, Auto, Off.
fn monitor_row(ui: &mut Ui, doc: &mut Document, ids: &[Id], current: Monitor) {
    super::midi_io::monitor_row(ui, doc, ids, current);
}

/// The level and clip light of what `input` reads: one side per channel for a
/// pair, both alike for a mono input.
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

/// Where `db` falls along `span` (the floor to full scale).
fn x_at(span: Rangef, db: f32) -> f32 {
    span.min + (db.clamp(FLOOR_DB, 0.0) - FLOOR_DB) / -FLOOR_DB * span.span()
}

/// The level bar along `body`'s bottom: the louder side's peak, with its RMS
/// solid over it.
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
    /// The In chip of an audio track's header in `rect`: a click opens the
    /// choices; the clip light, lit until clicked, clears itself.
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
                .text_secondary
                .lerp_to_gamma(theme::lit(Tone::On).text, face.lit)
        } else {
            palette.text_hint
        };
        let text = if trouble.is_some() {
            format!("\u{25B2}{}", chip_text(track.input))
        } else {
            chip_text(track.input)
        };
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            widgets::fit(
                ui.painter(),
                &text,
                FontId::proportional(theme::size::CAPTION),
                body.width() - 6.0,
            ),
            FontId::proportional(theme::size::CAPTION),
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

    /// The red mark on a lit chip: a click clears the input's clip light,
    /// Alt+click every one.
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

    /// An audio strip's input row (the same control as the header's chip);
    /// a click opens the choices.
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

    /// An audio strip's Monitor row; a click opens In, Auto and Off.
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
            &format!("Monitor  {word}"),
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

    /// The audio tracks a change on `id` applies to: the selection when `id`
    /// is in it, otherwise `id`.
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
        assert_eq!(chip_text(None), "In");
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
