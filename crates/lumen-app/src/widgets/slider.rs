//! The value slider and the value field (DESIGN §5): drag, Shift for
//! fine, double-click to reset, click and type, arrow keys.

use eframe::egui::{
    self, Align2, FontId, Id as EguiId, Key, Painter, Rangef, Rect, Sense, Stroke, Ui, pos2, vec2,
};

use super::*;
use crate::{motion, theme};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SliderKind {
    /// −60 to +6 dB, filled from the left; resets to 0 dB.
    Volume,
    /// −1 (left) to +1 (right), filled from the centre; resets to centre.
    Pan,
    /// A clip's gain (slice 6e): −60 to +12 dB, filled from the left;
    /// resets to 0 dB.
    ClipGain,
    /// A length in milliseconds, 0 to `max_ms`, filled from the left;
    /// resets to 0 (a fade's length, slice 6e).
    Millis { max_ms: f32 },
    /// A channel's input trim (M4 slice 5): −24 to +24 dB, filled from the
    /// centre; resets to 0 dB.
    Trim,
    /// A level as a share, 0 to 1 shown as 0–100%, filled from the left;
    /// resets to `default` (the metronome's volume, M4.5).
    Percent { default: f32 },
    /// A time offset in whole milliseconds, earlier (negative) or later,
    /// from `min_ms` to `max_ms`; resets to 0 (a MIDI clock's delay, M5
    /// slice 7).
    Offset { min_ms: f32, max_ms: f32 },
}

impl SliderKind {
    /// What a double-click sets.
    pub fn reset(self) -> f32 {
        match self {
            SliderKind::Percent { default } => default,
            _ => 0.0,
        }
    }

    pub fn range(self) -> (f32, f32) {
        match self {
            SliderKind::Volume => (-60.0, 6.0),
            SliderKind::Pan => (-1.0, 1.0),
            SliderKind::ClipGain => (-60.0, 12.0),
            SliderKind::Millis { max_ms } => (0.0, max_ms.max(0.0)),
            SliderKind::Trim => (-24.0, 24.0),
            SliderKind::Percent { .. } => (0.0, 1.0),
            SliderKind::Offset { min_ms, max_ms } => (min_ms, max_ms),
        }
    }

    /// What a notch of the wheel changes (Shift: a tenth of it): a dB on
    /// levels, 5% on pan, a hundredth of a length.
    pub fn wheel_step(self) -> f32 {
        match self {
            SliderKind::Volume | SliderKind::ClipGain | SliderKind::Trim => 1.0,
            SliderKind::Percent { .. } => 0.05,
            SliderKind::Pan => 0.05,
            SliderKind::Millis { max_ms } => (max_ms / 100.0).max(1.0),
            SliderKind::Offset { .. } => 1.0,
        }
    }

    pub fn text(self, value: f32) -> String {
        match self {
            SliderKind::Volume | SliderKind::ClipGain | SliderKind::Trim => db_text(value),
            SliderKind::Percent { .. } => format!("{:.0}%", value * 100.0),
            SliderKind::Pan => pan_text(value),
            SliderKind::Millis { .. } => ms_text(value),
            SliderKind::Offset { .. } => match value.round() as i32 {
                0 => "0 ms".to_owned(),
                ms if ms > 0 => format!("+{ms} ms"),
                ms => format!("\u{2212}{} ms", -ms),
            },
        }
    }

    /// Reads typed text: "-6", "−6 dB"; "C", "L 30", "R30", "-30".
    pub fn parse(self, text: &str) -> Option<f32> {
        let text = text.trim().replace('\u{2212}', "-").to_ascii_uppercase();
        match self {
            SliderKind::Volume | SliderKind::ClipGain | SliderKind::Trim => {
                text.trim_end_matches("DB").trim().parse().ok()
            }
            SliderKind::Percent { .. } => text
                .trim_end_matches('%')
                .trim()
                .parse::<f32>()
                .ok()
                .map(|p| p / 100.0),
            SliderKind::Offset { .. } => text
                .trim_end_matches("MS")
                .trim()
                .trim_start_matches('+')
                .replace(',', ".")
                .parse::<f32>()
                .ok()
                .map(f32::round),
            SliderKind::Millis { .. } => {
                let text = text.replace(',', ".");
                if let Some(ms) = text.strip_suffix("MS") {
                    ms.trim().parse().ok()
                } else if let Some(s) = text.strip_suffix('S') {
                    s.trim().parse::<f32>().ok().map(|s| s * 1000.0)
                } else {
                    text.parse().ok()
                }
            }
            SliderKind::Pan => {
                if text == "C" {
                    return Some(0.0);
                }
                let number = |s: &str| s.trim().parse::<f32>().ok().map(|n| n / 100.0);
                if let Some(rest) = text.strip_prefix('L') {
                    number(rest).map(|n| -n)
                } else if let Some(rest) = text.strip_prefix('R') {
                    number(rest)
                } else {
                    number(&text)
                }
            }
        }
    }
}

/// A slider with its value inside it. Drag (Shift for fine steps),
/// double-click to reset, click then type a value (Enter applies, Esc
/// cancels), or use the arrow keys when it has focus. Returns the value.
pub fn value_slider(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    value: f32,
    kind: SliderKind,
    name: &str,
    look: theme::Controls,
) -> f32 {
    value_slider_styled(
        ui,
        rect,
        id,
        value,
        kind,
        name,
        SliderStyle { look, comet: false },
    )
}

/// How a [`value_slider_styled`] is drawn.
#[derive(Clone, Copy)]
pub struct SliderStyle {
    /// Its colours, from [`theme::controls`].
    pub look: theme::Controls,
    /// Draw the filled part as the send knob's comet (M5 slice 5, the
    /// owner's request): a trail of the accent that brightens towards a
    /// glowing head, in place of the grey fill and the line handle. Only
    /// the one-sided kinds (not Pan or Trim) take it.
    pub comet: bool,
}

/// [`value_slider`] drawn as a comet.
pub fn comet_slider(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    value: f32,
    kind: SliderKind,
    name: &str,
    look: theme::Controls,
) -> f32 {
    value_slider_styled(
        ui,
        rect,
        id,
        value,
        kind,
        name,
        SliderStyle { look, comet: true },
    )
}

/// [`value_slider`] drawn with `style`.
pub fn value_slider_styled(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    value: f32,
    kind: SliderKind,
    name: &str,
    style: SliderStyle,
) -> f32 {
    let SliderStyle { look, comet } = style;
    let palette = *theme::p();
    let (lo, hi) = kind.range();
    let typing_id = id.with("typing");
    let typing: Option<String> = ui.data(|d| d.get_temp(typing_id));
    if let Some(mut text) = typing {
        // Typing a value.
        let edit = ui.put(
            rect,
            egui::TextEdit::singleline(&mut text)
                .id(id.with("edit"))
                .font(FontId::monospace(theme::size::MONO_SMALL))
                .horizontal_align(egui::Align::Center)
                .margin(vec2(2.0, 2.0)),
        );
        if !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        if cancel || edit.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(typing_id));
            if !cancel && let Some(v) = kind.parse(&text) {
                return v.clamp(lo, hi);
            }
        } else {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
        return value;
    }

    let response = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_text(match kind {
            SliderKind::Volume => {
                "Volume: drag or wheel (Shift for fine), double-click for 0 dB, or click and type"
            }
            SliderKind::Pan => {
                "Pan: drag or wheel (Shift for fine), double-click to center, or click and type L 30 or R 30"
            }
            SliderKind::ClipGain => {
                "Gain: drag (Shift for fine), double-click for 0 dB, or click and type"
            }
            SliderKind::Millis { .. } => {
                "Length: drag (Shift for fine), double-click for none, or click and type 40 ms or 1.2 s"
            }
            SliderKind::Trim => {
                "Input trim: drag (Shift for fine), double-click for 0 dB, or click and type"
            }
            SliderKind::Percent { .. } => {
                "Drag or wheel (Shift for fine), double-click for the default, or click and type"
            }
            SliderKind::Offset { .. } => {
                "Delay: drag or wheel (Shift for fine), double-click for none, or click and type"
            }
        });
    response.widget_info(|| {
        egui::WidgetInfo::slider(
            true,
            f64::from(value),
            format!("{name}: {}", kind.text(value)),
        )
    });
    let mut out = value;
    let fine = ui.input(|i| i.modifiers.shift);
    if response.dragged() {
        let dx = response.drag_delta().x;
        out += dx / rect.width() * (hi - lo) * if fine { 0.1 } else { 1.0 };
    }
    if response.clicked() {
        response.request_focus();
    }
    if response.double_clicked() {
        out = kind.reset();
    }
    if response.has_focus() {
        let (left, right, typed) = ui.input(|i| {
            let typed = i.events.iter().find_map(|e| match e {
                egui::Event::Text(t)
                    if t.chars()
                        .all(|c| c.is_ascii_digit() || "-.,lLrRcCsS".contains(c)) =>
                {
                    Some(t.clone())
                }
                _ => None,
            });
            (
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowUp),
                typed,
            )
        });
        let step = (hi - lo) / if fine { 660.0 } else { 66.0 };
        if left {
            out -= step;
        }
        if right {
            out += step;
        }
        if let Some(text) = typed {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
    }
    // The wheel, when its gesture started here (the owner's B3).
    let notches = wheel::steps(ui, id, rect);
    if notches != 0.0 {
        out += notches * kind.wheel_step() * if fine { 0.1 } else { 1.0 };
    }
    let out = out.clamp(lo, hi);

    // Drawing.
    let p = ui.painter();
    let hover = motion::hover(
        ui.ctx(),
        id.with("hover"),
        response.hovered() || response.dragged(),
    );
    // The ground is a well (the Field card, ADR-0052; it supersedes the
    // owner's T1, which filled it with the control colour), Ember under
    // the pointer; the filled part is drawn over it.
    let radius = theme::radius::for_height(rect.height());
    raised::paint_sunken(p, rect, radius);
    if hover > 0.0 {
        p.rect_filled(rect, f32::from(radius), palette.ember.gamma_multiply(hover));
    }
    let x_at = |v: f32| rect.left() + (v - lo) / (hi - lo) * rect.width();
    let text = kind.text(out);
    // Numbers are mono (`.lm-slider .val`); a trim's "dB" is smaller and
    // grey, so the number has room in a strip's narrow field (the interface
    // audit's X1–X4 B).
    let galley = match text
        .strip_suffix(" dB")
        .filter(|_| kind == SliderKind::Trim)
    {
        Some(number) => {
            let mut job = egui::text::LayoutJob::default();
            let mono = FontId::monospace(theme::size::MONO_SMALL);
            job.append(number, 0.0, egui::TextFormat::simple(mono, palette.text));
            job.append(
                "dB",
                theme::space::XXS,
                egui::TextFormat {
                    font_id: FontId::proportional(theme::size::UNIT),
                    color: palette.text_hint,
                    valign: egui::Align::Center,
                    ..Default::default()
                },
            );
            p.layout_job(job)
        }
        None => p.layout_no_wrap(
            text,
            FontId::monospace(theme::size::MONO_SMALL),
            palette.text,
        ),
    };
    let text_rect = Align2::CENTER_CENTER.anchor_size(rect.center(), galley.size());
    match kind {
        SliderKind::Volume
        | SliderKind::ClipGain
        | SliderKind::Millis { .. }
        | SliderKind::Percent { .. }
        | SliderKind::Offset { .. } => {
            let x = x_at(out);
            if comet {
                draw_comet(p, rect, x, hover.max(f32::from(response.dragged())));
            } else {
                // `press` to `fill`, cut off square at the value; the
                // well's rounded corners shape its left end.
                raised::gradient(
                    &p.with_clip_rect(Rect::from_min_max(rect.min, pos2(x, rect.bottom()))),
                    rect,
                    f32::from(radius),
                    look.press,
                    look.fill,
                );
                draw_handle(p, x, rect.shrink2(vec2(0.0, 3.0)).y_range(), text_rect);
            }
        }
        SliderKind::Pan | SliderKind::Trim => {
            let center = rect.center().x;
            let x = x_at(out);
            // Grey at the centre, the accent at the edges (owner request).
            let strength = (out / hi).abs().powf(1.2);
            let fill = look.press.lerp_to_gamma(palette.accent, strength);
            p.rect_filled(
                Rect::from_x_y_ranges(center.min(x)..=center.max(x), rect.y_range()),
                0.0,
                fill,
            );
            // The centre mark: short ticks at the edges, clear of the value
            // text (a full line ran through the "C").
            for y in [
                rect.top()..=rect.top() + 3.0,
                rect.bottom() - 3.0..=rect.bottom(),
            ] {
                p.vline(center, y, Stroke::new(1.0, palette.text_hint));
            }
            // The fill's moving edge, as the volume's handle.
            if (x - center).abs() >= 1.0 {
                draw_handle(p, x, rect.shrink2(vec2(0.0, 3.0)).y_range(), text_rect);
            }
        }
    }
    p.galley(text_rect.min, galley, palette.text);
    if response.has_focus() {
        raised::paint_focus(p, rect, radius);
    }
    out
}

/// The comet (the send knob's look, laid flat): a trail of the accent,
/// faint at the left and full at the head, a soft glow round the head and
/// a bright core. `hot` (0 to 1) swells the glow while the pointer is on
/// the slider.
fn draw_comet(p: &Painter, rect: Rect, x: f32, hot: f32) {
    let palette = *theme::p();
    let inner = rect.shrink2(vec2(2.0, 4.0));
    let r = inner.height() / 2.0;
    // The head stays inside the slider at both ends, and the trail ends at
    // it (at 100% the trail ran on past the head to the edge).
    let x = x.clamp(inner.left() + r, inner.right() - r);
    if x - inner.left() >= 1.0 {
        let pieces = 24;
        let width = x - inner.left();
        for i in 0..pieces {
            let (a, b) = (i as f32 / pieces as f32, (i + 1) as f32 / pieces as f32);
            let alpha = 0.18 + 0.82 * b * b;
            let piece = Rect::from_x_y_ranges(
                inner.left() + width * a..=inner.left() + width * b + 0.5,
                inner.y_range(),
            );
            p.rect_filled(piece, 0.0, palette.accent.gamma_multiply(alpha));
        }
    }
    let head = pos2(x, rect.center().y);
    // A restrained glow (owner: too much): one soft ring, a little more
    // while the pointer is on it.
    let glow = 1.0 + 0.25 * hot;
    for (radius, alpha) in [(r * 1.9 * glow, 0.10), (r * 1.4 * glow, 0.22)] {
        p.circle_filled(head, radius, palette.accent.gamma_multiply(alpha));
    }
    p.circle_filled(head, r, palette.core);
    p.circle_filled(head, r * 0.62, palette.accent);
}

/// How far from the value's text a slider's handle starts to open, in
/// points.
const HANDLE_OPENS_WITHIN: f32 = 8.0;

/// A slider's handle at `x` over `ys`: a 2 pt `text` line that opens around the
/// value's `text` as it comes near, smoothly with the distance, until only
/// a tick above and below it remains (the owner's choice C, the interface
/// audit's S2: the line ran through the value). Nothing animates by
/// itself: the opening follows the handle.
fn draw_handle(p: &Painter, x: f32, ys: Rangef, text: Rect) {
    let stroke = Stroke::new(2.0, theme::p().text);
    for span in handle_spans(x, ys, text) {
        p.vline(x, span, stroke);
    }
}

/// Half the height a handle opens to around the value: from the capitals'
/// tops to the baseline (about 0.36 of the size each way from the line's
/// middle), with 1.5 pt to spare; not the whole line, which fills the
/// slider. The value is in `MONO_SMALL`.
const HANDLE_GAP_HALF: f32 = 0.36 * theme::size::MONO_SMALL + 1.5;

/// The parts of a handle at `x` over `ys` left drawn around `text`.
fn handle_spans(x: f32, ys: Rangef, text: Rect) -> Vec<Rangef> {
    let away = (x - text.center().x).abs() - text.width() / 2.0 - 3.0;
    let near = (1.0 - away.max(0.0) / HANDLE_OPENS_WITHIN).clamp(0.0, 1.0);
    let open = near * near * (3.0 - 2.0 * near);
    let half = open * HANDLE_GAP_HALF;
    let gap = Rangef::new(text.center().y - half, text.center().y + half);
    if gap.span() < 0.5 {
        return vec![ys];
    }
    [
        Rangef::new(ys.min, gap.min.min(ys.max)),
        Rangef::new(gap.max.max(ys.min), ys.max),
    ]
    .into_iter()
    .filter(|s| s.span() >= 0.5)
    .collect()
}

/// A value shown as text in a field (a strip's volume under its fader, M4
/// slice 5): click to type (Enter applies, Esc cancels), double-click to
/// reset. A field of the design system: a well with a border that
/// strengthens on hover, the digits in the monospace. It is a well on
/// every surface, so the surface's `_controls` are not needed. Returns the
/// value.
pub fn value_field(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    value: f32,
    kind: SliderKind,
    name: &str,
    _controls: theme::Controls,
) -> f32 {
    let palette = *theme::p();
    let (lo, hi) = kind.range();
    let typing_id = id.with("typing");
    if let Some(mut text) = ui.data(|d| d.get_temp::<String>(typing_id)) {
        let edit = ui.put(
            rect,
            egui::TextEdit::singleline(&mut text)
                .id(id.with("edit"))
                .font(FontId::monospace(theme::size::CAPTION))
                .horizontal_align(egui::Align::Center)
                .margin(vec2(2.0, 2.0)),
        );
        if !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        if cancel || edit.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(typing_id));
            if !cancel && let Some(v) = kind.parse(&text) {
                return v.clamp(lo, hi);
            }
        } else {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
        return value;
    }
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::Text)
        .on_hover_text("Click and type a value; double-click to reset");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::TextEdit,
            true,
            format!("{name}: {}", kind.text(value)),
        )
    });
    let mut out = value;
    let notches = wheel::steps(ui, id, rect);
    if notches != 0.0 {
        let fine = ui.input(|i| i.modifiers.shift);
        out = (out + notches * kind.wheel_step() * if fine { 0.1 } else { 1.0 }).clamp(lo, hi);
    }
    if response.double_clicked() {
        ui.data_mut(|d| d.remove::<String>(typing_id));
        out = kind.reset();
    } else if response.clicked() {
        ui.data_mut(|d| d.insert_temp(typing_id, kind.text(value).replace(" dB", "")));
    }
    let hover = motion::hover(ui.ctx(), id.with("hover"), response.hovered());
    // A field, not a button: sunk in its well, with a `border` that
    // strengthens to `border_strong` on hover (the Field card, ADR-0052).
    let rect = rect.round_to_pixels(ui.pixels_per_point());
    let radius = theme::radius::for_height(rect.height());
    let p = ui.painter();
    raised::paint_field(p, rect, radius, hover);
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        kind.text(out).replace(" dB", ""),
        FontId::monospace(theme::size::CAPTION),
        palette.text,
    );
    raised::focus_ring(p, &response, rect);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offset_reads_and_writes_signed_milliseconds() {
        let kind = SliderKind::Offset {
            min_ms: -20.0,
            max_ms: 50.0,
        };
        assert_eq!(kind.text(0.0), "0 ms");
        assert_eq!(kind.text(12.0), "+12 ms");
        assert_eq!(kind.text(-7.0), "\u{2212}7 ms");
        for typed in ["-7", "\u{2212}7 ms", "-7ms"] {
            assert_eq!(kind.parse(typed), Some(-7.0), "{typed}");
        }
        assert_eq!(kind.parse("+12 ms"), Some(12.0));
        assert_eq!(kind.parse("3,6"), Some(4.0), "whole milliseconds");
        assert_eq!(kind.range(), (-20.0, 50.0));
    }

    #[test]
    fn a_handle_opens_around_the_value_as_it_nears_it() {
        let ys = Rangef::new(3.0, 17.0);
        // A value centred in a 20 pt slider, as on a strip.
        let text = Rect::from_min_max(pos2(40.0, 4.5), pos2(80.0, 15.5));
        // Far away: one solid line.
        assert_eq!(handle_spans(10.0, ys, text), vec![ys]);
        // Over the text: a tick above and below, clear of the letters.
        let over = handle_spans(60.0, ys, text);
        assert_eq!(over.len(), 2, "{over:?}");
        let middle = text.center().y;
        assert!(
            over[0].max < middle - 4.0 && over[1].min > middle + 4.0,
            "{over:?}"
        );
        assert!(over[0].span() >= 1.5 && over[1].span() >= 1.5, "{over:?}");
        // Coming near, the gap grows with the closeness, never jumps.
        let gap = |x: f32| {
            let spans = handle_spans(x, ys, text);
            if spans.len() == 2 {
                spans[1].min - spans[0].max
            } else {
                0.0
            }
        };
        let mut last = 0.0;
        for i in 0..=40 {
            let g = gap(25.0 + i as f32 * 0.5);
            assert!(g >= last - 1e-4, "{g} after {last}");
            last = g;
        }
        assert!(gap(36.0) > 0.0 && gap(36.0) < gap(37.0));
    }

    #[test]
    fn values_read_naturally() {
        assert_eq!(db_text(0.0), "0.0 dB");
        assert_eq!(db_text(-3.0), "\u{2212}3.0 dB");
        assert_eq!(db_text(6.0), "+6.0 dB");
        assert_eq!(db_text(-60.0), "\u{2212}\u{221E} dB");
        assert_eq!(pan_text(0.0), "C");
        assert_eq!(pan_text(-0.3), "L 30");
        assert_eq!(pan_text(1.0), "R 100");
    }

    #[test]
    fn readouts_fit_a_strip_box() {
        assert_eq!(readout_db(-3.0), "\u{2212}3.0");
        assert_eq!(readout_db(6.0), "+6.0");
        assert_eq!(readout_db(-9.9), "\u{2212}9.9");
        assert_eq!(readout_db(-12.3), "\u{2212}12");
        assert_eq!(readout_db(9.96), "+10");
        assert_eq!(readout_db(-48.0), "\u{2212}48");
    }

    #[test]
    fn typed_values_are_understood() {
        let v = SliderKind::Volume;
        assert_eq!(v.parse("-6"), Some(-6.0));
        assert_eq!(v.parse("\u{2212}4.5 dB"), Some(-4.5));
        assert_eq!(v.parse("loud"), None);
        let p = SliderKind::Pan;
        assert_eq!(p.parse("c"), Some(0.0));
        assert_eq!(p.parse("L 30"), Some(-0.3));
        assert_eq!(p.parse("r100"), Some(1.0));
        assert_eq!(p.parse("-50"), Some(-0.5));
    }
}
