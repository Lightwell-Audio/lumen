//! The loopback helper's card in the audio settings' Latency section (the
//! owner's A, 2026-10-29): the cable's ends, what came back of the clicks,
//! the round trip found, and the offset it asks for, offered with one click.

use eframe::egui::{self, RichText, Sense, Shape, Ui, pos2, vec2};
use lumen_engine::loopback::{CLICKS, Failure, RETURN_BEFORE};

use super::{Cable, Measurement, Outcome, Problem};
use crate::audio_settings::{hint, warning};
use crate::settings::MeasuredWith;
use crate::theme;
use crate::widgets::{Dropdown, raised};

/// The returned clicks' strip.
const TRACE_HEIGHT: f32 = theme::size::CONTROL_LARGE * 1.5;
/// The share of the row's room the output dropdown takes: an output pair's
/// label is about twice an input's.
const OUTPUT_SHARE: f32 = 0.58;

/// The card's own state: open or folded, and the cable's ends as chosen.
#[derive(Debug, Default)]
pub struct Card {
    pub open: bool,
    /// Scrolls the card into view on its next frame: the window can be
    /// shorter than the Audio page.
    pub reveal: bool,
    /// The chosen output pair and input, as the callback numbers them.
    output: Option<usize>,
    input: Option<usize>,
}

/// What the card shows, lent each frame.
pub struct Shown<'a> {
    pub measurement: &'a Measurement,
    /// The device channels open as outputs and inputs, in callback order.
    pub outputs: &'a [usize],
    pub inputs: &'a [usize],
    /// The device's channel names, by device channel.
    pub output_names: &'a [String],
    pub input_names: &'a [String],
    /// Why Measure cannot run now (playing, recording), if it cannot.
    pub held: Option<&'static str>,
    /// The recording offset in use.
    pub offset_ms: f32,
}

/// What the card asks for.
pub enum Asked {
    Measure(Cable),
    Use(f32, MeasuredWith),
}

/// One end the cable can be plugged into: (first callback channel, both of
/// a pair, label).
type End = (usize, bool, String);

/// An end's label as it reads in a sentence: "2 (In 2)" for "2  In 2".
fn in_words(label: &str) -> String {
    match label.split_once("  ") {
        Some((number, name)) => format!("{number} ({name})"),
        None => label.to_owned(),
    }
}

fn name(names: &[String], channel: usize, prefix: &str) -> String {
    match names.get(channel).map(|n| n.trim()) {
        Some(given) if !given.is_empty() => given.to_owned(),
        _ => format!("{prefix} {}", channel + 1),
    }
}

/// The open outputs as pairs (a last odd one alone), labelled by device
/// channel.
fn output_ends(outputs: &[usize], names: &[String]) -> Vec<End> {
    outputs
        .chunks(2)
        .enumerate()
        .map(|(i, pair)| match *pair {
            [a, b] => (
                i * 2,
                true,
                format!(
                    "{}/{}  {} / {}",
                    a + 1,
                    b + 1,
                    name(names, a, "Out"),
                    name(names, b, "Out")
                ),
            ),
            [a] => (
                i * 2,
                false,
                format!("{}  {}", a + 1, name(names, a, "Out")),
            ),
            _ => unreachable!("chunks of two"),
        })
        .collect()
}

fn input_ends(inputs: &[usize], names: &[String]) -> Vec<End> {
    inputs
        .iter()
        .enumerate()
        .map(|(i, &c)| (i, false, format!("{}  {}", c + 1, name(names, c, "In"))))
        .collect()
}

impl Card {
    pub fn show(&mut self, ui: &mut Ui, shown: Shown<'_>) -> Option<Asked> {
        let outputs = output_ends(shown.outputs, shown.output_names);
        let inputs = input_ends(shown.inputs, shown.input_names);
        // A choice the device no longer has falls back to the first.
        let output = self
            .output
            .and_then(|o| outputs.iter().position(|e| e.0 == o))
            .unwrap_or(0);
        let input = self
            .input
            .and_then(|i| inputs.iter().position(|e| e.0 == i))
            .unwrap_or(0);
        let (Some(out_end), Some(in_end)) = (outputs.get(output), inputs.get(input)) else {
            return None;
        };
        let busy = shown.measurement.busy();
        let outcome = shown.measurement.outcome();
        let mut asked = None;

        let back = [ui.painter().add(Shape::Noop), ui.painter().add(Shape::Noop)];
        // An indent under the offset it sets.
        let outer = egui::Margin {
            left: (2.0 * theme::space::L) as i8,
            top: theme::space::XS as i8,
            ..Default::default()
        };
        let frame =
            egui::Frame::NONE
                .inner_margin(theme::space::M)
                .outer_margin(outer)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = theme::space::S;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Cable from").weak());
                        // The two dropdowns share what is left of the row.
                        let to = ui
                            .painter()
                            .layout_no_wrap(
                                "to".into(),
                                egui::TextStyle::Body.resolve(ui.style()),
                                theme::p().text_secondary,
                            )
                            .size()
                            .x;
                        let room = (ui.available_width() - to - 2.0 * ui.spacing().item_spacing.x)
                            .max(0.0);
                        let labels: Vec<&str> = outputs.iter().map(|e| e.2.as_str()).collect();
                        let out_width = (room * OUTPUT_SHARE).floor();
                        if let Some(i) = Dropdown::new("Cable from", &out_end.2, out_width).show(
                            ui,
                            egui::Id::new("measure output"),
                            &labels,
                            Some(output),
                        ) {
                            self.output = Some(outputs[i].0);
                        }
                        ui.label(RichText::new("to").weak());
                        let labels: Vec<&str> = inputs.iter().map(|e| e.2.as_str()).collect();
                        if let Some(i) = Dropdown::new("Cable to", &in_end.2, room - out_width)
                            .show(ui, egui::Id::new("measure input"), &labels, Some(input))
                        {
                            self.input = Some(inputs[i].0);
                        }
                    });
                    trace(ui, busy, outcome);
                    if let Some(outcome) = outcome.filter(|_| !busy) {
                        asked = result(ui, outcome, shown.offset_ms);
                    }
                    ui.horizontal(|ui| {
                        let again = outcome.is_some();
                        let label = if again { "Measure again" } else { "Measure" };
                        let enabled = !busy && shown.held.is_none();
                        let draw = |ui: &mut Ui| {
                            if again {
                                raised::button(ui, label)
                            } else {
                                raised::primary_button(ui, label)
                            }
                        };
                        let button = ui
                            .add_enabled(enabled, draw)
                            .on_hover_text(
                                "Plays five clicks on the output and listens for them on the input",
                            )
                            .on_disabled_hover_text(shown.held.unwrap_or("Measuring…"));
                        if button.clicked() {
                            asked = Some(Asked::Measure(Cable {
                                output: out_end.0,
                                pair: out_end.1,
                                input: in_end.0,
                                output_name: in_words(&out_end.2),
                                input_name: in_words(&in_end.2),
                            }));
                        }
                        hint(ui, shown.held.unwrap_or("Turn speakers down first"));
                    });
                    hint(
                        ui,
                        "Use a cable between the jacks: an input that loops back inside the \
                     device skips its converters, so it measures too short.",
                    );
                });
        // The response holds the outer margin; the well sits inside it.
        let [well, lip] = raised::sunken_shapes(frame.response.rect - outer, theme::radius::LARGE);
        ui.painter().set(back[0], well);
        ui.painter().set(back[1], lip);
        if std::mem::take(&mut self.reveal) {
            ui.scroll_to_rect(frame.response.rect, None);
        }
        if busy {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        asked
    }
}

/// The strip that shows what came back of the clicks, laid over each
/// other: one sharp line when they agree.
fn trace(ui: &mut Ui, busy: bool, outcome: Option<&Outcome>) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, TRACE_HEIGHT), Sense::hover());
    let c = theme::p();
    let painter = ui.painter_at(rect);
    painter.hline(rect.x_range(), rect.center().y, (1.0, c.grid_bar));
    let returns = outcome
        .filter(|_| !busy)
        .and_then(|o| o.result.as_ref().ok())
        .map(|m| m.returns.as_slice());
    let described = match returns {
        Some(returns) if !returns.is_empty() => {
            let loudest = returns
                .iter()
                .flatten()
                .fold(0.0_f32, |m, s| m.max(s.abs()))
                .max(1e-6);
            let frames = returns.iter().map(Vec::len).max().unwrap_or(1).max(2);
            let x = |i: usize| rect.left() + rect.width() * i as f32 / (frames - 1) as f32;
            let y = |s: f32| rect.center().y - s / loudest * (rect.height() * 0.45);
            // The first on top, the others fainter under it.
            for (k, back) in returns.iter().enumerate().rev() {
                let colour = if k == 0 {
                    c.accent
                } else {
                    c.accent.gamma_multiply(0.35)
                };
                let points = back.iter().enumerate().map(|(i, s)| pos2(x(i), y(*s)));
                painter.add(Shape::line(points.collect(), (1.4, colour)));
            }
            let at = x(RETURN_BEFORE);
            painter.extend(Shape::dashed_line(
                &[pos2(at, rect.top()), pos2(at, rect.bottom())],
                (1.0, c.text_secondary),
                2.0,
                2.0,
            ));
            painter.text(
                pos2(
                    rect.right() - theme::space::XS,
                    rect.top() + theme::space::XXS,
                ),
                egui::Align2::RIGHT_TOP,
                format!("{CLICKS} of {CLICKS} agree"),
                egui::FontId::proportional(theme::size::MONO_SMALL),
                c.text_secondary,
            );
            format!("Returned clicks: {CLICKS} of {CLICKS} agree")
        }
        _ if busy => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Listening…",
                egui::FontId::proportional(theme::size::BODY),
                c.text_secondary,
            );
            "Listening for the clicks".to_owned()
        }
        _ => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "The clicks show here as they come back",
                egui::FontId::proportional(theme::size::MONO_SMALL),
                c.text_hint,
            );
            "No clicks measured yet".to_owned()
        }
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &described));
}

/// The round trip found and the offset it asks for, or what went wrong and
/// what to do.
fn result(ui: &mut Ui, outcome: &Outcome, offset_ms: f32) -> Option<Asked> {
    let ms = |frames: u32| f64::from(frames) * 1000.0 / f64::from(outcome.rate.max(1));
    let measured = match &outcome.result {
        Ok(measured) => measured,
        Err(problem) => {
            warning(ui, &problem_text(*problem, &outcome.cable), 0.0);
            return None;
        }
    };
    ui.horizontal_wrapped(|ui| {
        ui.label("Measured round trip");
        ui.label(RichText::new(format!("{:.1} ms", ms(measured.frames))).strong());
        hint(
            ui,
            &format!(
                "{} samples · driver says {:.1} ms",
                measured.frames,
                ms(outcome.reported)
            ),
        );
    });
    if measured.clipped {
        warning(
            ui,
            "The input clipped. The timing is still right, but turn its gain down before \
             recording.",
            0.0,
        );
    }
    if measured.inverted {
        hint(
            ui,
            "The clicks came back upside down (a polarity flip on the way); the timing is \
             still right.",
        );
    }
    let offset = outcome.offset_ms()?;
    let mut asked = None;
    ui.horizontal(|ui| {
        if (offset - offset_ms).abs() < 0.05 {
            ui.label(RichText::new(format!("The recording offset is {offset:+.1} ms")).weak());
        } else if raised::primary_button(ui, &format!("Use {offset:+.1} ms"))
            .on_hover_text(format!(
                "Sets the recording offset to {offset:+.1} ms (now {offset_ms:+.1} ms), so a \
                 take lands where it was played"
            ))
            .clicked()
        {
            asked = Some(Asked::Use(offset, outcome.with.clone()));
        }
    });
    asked
}

fn problem_text(problem: Problem, cable: &Cable) -> String {
    let (out, input) = (&cable.output_name, &cable.input_name);
    match problem {
        Problem::Failed(Failure::Silent) => format!(
            "Nothing came back on input {input}. Check that the cable runs from output {out} \
             to input {input} and that the input's gain is up, then measure again."
        ),
        Problem::Failed(Failure::Unclear) => format!(
            "The clicks did not stand out from the noise on input {input}. Raise the input's \
             gain, or turn down what else it hears, then measure again."
        ),
        Problem::Failed(Failure::Disagree { spread }) => format!(
            "The clicks came back up to {spread} samples apart, so the timing is not steady. \
             Close other audio programs or try a larger buffer, then measure again."
        ),
        Problem::Failed(Failure::RateChanged | Failure::NoChannel) | Problem::Stopped => {
            "The audio device changed or stopped while measuring. Measure again once it runs."
                .to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ends_are_named_by_device_channel() {
        let names: Vec<String> = ["Mon L", "Mon R", "", "Phones"].map(String::from).to_vec();
        let outs = output_ends(&[0, 1, 2, 3, 4], &names);
        assert_eq!(outs[0], (0, true, "1/2  Mon L / Mon R".into()));
        assert_eq!(outs[1], (2, true, "3/4  Out 3 / Phones".into()));
        assert_eq!(outs[2], (4, false, "5  Out 5".into()));
        let ins = input_ends(&[1], &["A".into(), "Guitar".into()]);
        assert_eq!(ins, vec![(0, false, "2  Guitar".into())]);
        assert_eq!(in_words("1/2  Mon L / Mon R"), "1/2 (Mon L / Mon R)");
        assert_eq!(in_words("5"), "5");
    }

    #[test]
    fn every_problem_says_what_to_do() {
        let cable = Cable {
            output: 0,
            pair: true,
            input: 1,
            output_name: "1/2".into(),
            input_name: "2".into(),
        };
        for problem in [
            Problem::Failed(Failure::Silent),
            Problem::Failed(Failure::Unclear),
            Problem::Failed(Failure::Disagree { spread: 9 }),
            Problem::Failed(Failure::RateChanged),
            Problem::Stopped,
        ] {
            let text = problem_text(problem, &cable);
            assert!(text.ends_with("measure again.") || text.ends_with("once it runs."));
        }
    }
}
