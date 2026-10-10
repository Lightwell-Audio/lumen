//! The send knob (M4 slice 4, the owner's K1 "comet"): a ring with the
//! return's letter in the middle; the level is a trail of light in the
//! return's colour that brightens towards a glowing head. A dot at the top
//! marks a pre-fader send. Silent, it is only the ring and the letter.
//!
//! It acts as every value control does (DESIGN.md §7): drag up or right
//! (Shift for fine), double-click for silence, click then type a level
//! (Enter applies, Esc cancels), arrow keys when focused; the right-click
//! menu switches pre- and post-fader. The wheel turns it a dB a notch
//! (Shift a tenth) when its gesture starts on the knob (`wheel.rs`).

use std::f32::consts::PI;

use eframe::egui::{
    self, Align2, Color32, FontId, Id as EguiId, Key, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2,
    vec2,
};

use super::db_text;
use crate::{fonts, motion, theme};

/// The loudest send, in dB.
pub const MAX_DB: f32 = 6.0;
/// Quieter than this is silence.
const FLOOR_DB: f32 = -60.0;
/// Where 0 dB sits on the knob's travel (three quarters, as on a console).
const UNITY_AT: f32 = 0.75;
/// dB per decade of travel: 48 puts +6 dB at the top.
const TAPER: f32 = 48.0;

/// The ring's sweep: 270°, from lower left round to lower right.
const START: f32 = 0.75 * PI;
const SWEEP: f32 = 1.5 * PI;

/// Where `db` sits on the knob's travel, 0 to 1 (silence is 0).
pub fn position(db: f32) -> f32 {
    if !db.is_finite() || db <= FLOOR_DB {
        return 0.0;
    }
    (UNITY_AT * 10f32.powf(db.min(MAX_DB) / TAPER)).clamp(0.0, 1.0)
}

/// Where the travel turns to silence: the position of −60 dB.
fn floor() -> f32 {
    UNITY_AT * 10f32.powf(FLOOR_DB / TAPER)
}

/// The level at `position` on the travel: silence at the bottom. (Before
/// MT-MIX-4 run 1 the bottom was −80 dB: the floor was taken as 0.)
pub fn level(position: f32) -> f32 {
    if position <= floor() {
        return f32::NEG_INFINITY;
    }
    (TAPER * (position.min(1.0) / UNITY_AT).log10()).min(MAX_DB)
}

/// What a knob shows.
#[derive(Debug, Clone, Copy)]
pub struct SendKnob<'a> {
    /// The return's letter.
    pub letter: &'a str,
    /// The return's colour.
    pub color: Color32,
    /// In dB; `NEG_INFINITY` when silent or not there yet.
    pub db: f32,
    pub pre_fader: bool,
    /// The send is heard now (not muted or silenced by a solo).
    pub heard: bool,
    /// Its return is hovered somewhere: the knob lights up to match.
    pub linked: bool,
    /// On a selected row its ring and letter step up, as every control's
    /// colours do there (the owner's C4, MT-MIX-4 run 1).
    pub selected: bool,
    /// Shows its level in a chip above it while it moves. Off in lists,
    /// whose level column already follows (MT-MIX-4 run 3: the two drew
    /// over each other).
    pub live_value: bool,
    /// The accessible name: "Send Vocal to A Reverb".
    pub name: &'a str,
}

/// What happened to a knob this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct KnobOut {
    /// A new level, in dB (`NEG_INFINITY` for silence).
    pub db: Option<f32>,
    /// Pre- or post-fader chosen from the menu.
    pub pre_fader: Option<bool>,
    pub hovered: bool,
    /// Being set right now: dragged, focused or typed into.
    pub active: bool,
}

/// A send knob in `rect` (square). Returns what the user changed.
pub fn send_knob(ui: &mut Ui, rect: Rect, id: EguiId, knob: SendKnob<'_>) -> KnobOut {
    let palette = *theme::p();
    let mut out = KnobOut::default();
    let typing_id = id.with("typing");
    if let Some(mut text) = ui.data(|d| d.get_temp::<String>(typing_id)) {
        // Typing a level, in a field wide enough to read.
        let field = Rect::from_center_size(rect.center(), vec2(64.0, rect.height().max(18.0)));
        let edit = ui.put(
            field,
            egui::TextEdit::singleline(&mut text)
                .id(id.with("edit"))
                .font(FontId::proportional(theme::size::CAPTION))
                .horizontal_align(egui::Align::Center)
                .margin(vec2(2.0, 2.0)),
        );
        if !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        if cancel || edit.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(typing_id));
            if !cancel {
                out.db = parse(&text);
            }
        } else {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
        out.active = true;
        return out;
    }

    let response = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_text(
            "Send: drag up (Shift for fine), double-click for silence, or click and type. \
         Right-click for pre- or post-fader",
        );
    response.widget_info(|| {
        egui::WidgetInfo::slider(
            true,
            f64::from(position(knob.db)),
            format!("{}: {}", knob.name, db_text(knob.db.max(-60.0))),
        )
    });
    out.hovered = response.hovered() || response.dragged();
    out.active = response.dragged() || response.has_focus();
    let fine = ui.input(|i| i.modifiers.shift);
    let mut at = position(knob.db);
    let before = at;
    // A drag keeps its own position on the travel, from where it was
    // pressed: started again from the level each frame, slow moves below
    // the silence floor were rounded back to silence every frame and never
    // added up (owner, MT-MIX-4 run 5: hard to turn up from −∞).
    let drag_id = id.with("drag");
    if response.dragged()
        && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
    {
        let (mut travel, last) = ui
            .data(|d| d.get_temp::<(f32, egui::Pos2)>(drag_id))
            .unwrap_or_else(|| {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
                (position(knob.db), origin)
            });
        let d = pointer - last;
        travel = (travel + (d.x - d.y) / 160.0 * if fine { 0.1 } else { 1.0 }).clamp(0.0, 1.0);
        ui.data_mut(|data| data.insert_temp(drag_id, (travel, pointer)));
        at = travel;
    } else {
        ui.data_mut(|data| data.remove::<(f32, egui::Pos2)>(drag_id));
    }
    if response.clicked() {
        response.request_focus();
    }
    if response.has_focus() {
        let (down, up, typed) = ui.input(|i| {
            let typed = i.events.iter().find_map(|e| match e {
                egui::Event::Text(t)
                    if t.chars().all(|c| c.is_ascii_digit() || "-.,".contains(c)) =>
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
        let step = if fine { 0.005 } else { 0.03 };
        if down {
            at -= step;
        }
        if up {
            at += step;
        }
        if let Some(text) = typed {
            ui.data_mut(|d| d.insert_temp(typing_id, text));
        }
    }
    let at = at.clamp(0.0, 1.0);
    if at != before {
        out.db = Some(level(at));
    }
    // The wheel: a dB a notch, set as a level (not through the travel, where
    // the floor itself reads as silence); up from silence starts at −60 dB.
    let notches = super::wheel::steps(ui, id, rect);
    if notches != 0.0 {
        let step = notches * if fine { 0.1 } else { 1.0 };
        let db = out.db.unwrap_or(knob.db);
        let next = if db.is_finite() {
            db + step
        } else if step > 0.0 {
            FLOOR_DB
        } else {
            db
        };
        out.db = Some(if next < FLOOR_DB {
            f32::NEG_INFINITY
        } else {
            next.min(MAX_DB)
        });
    }
    if response.double_clicked() {
        out.db = Some(f32::NEG_INFINITY);
    }
    response.context_menu(|ui| {
        if ui
            .radio(knob.pre_fader, "Pre-fader")
            .on_hover_text("Taken before the track's volume and pan")
            .clicked()
        {
            out.pre_fader = Some(true);
            ui.close();
        }
        if ui
            .radio(!knob.pre_fader, "Post-fader")
            .on_hover_text("Taken after the track's volume and pan")
            .clicked()
        {
            out.pre_fader = Some(false);
            ui.close();
        }
        ui.separator();
        if ui.button("Silence the send").clicked() {
            out.db = Some(f32::NEG_INFINITY);
            ui.close();
        }
    });

    let shown = out.db.map_or(at, position);
    let hot = motion::hover(ui.ctx(), id.with("hot"), out.hovered || knob.linked);
    draw(ui.painter(), rect, &knob, shown, hot);
    if response.has_focus() {
        ui.painter().circle_stroke(
            rect.center(),
            rect.width() / 2.0,
            Stroke::new(1.5, palette.accent),
        );
    }
    if knob.live_value && (response.dragged() || response.has_focus() && out.db.is_some()) {
        // The level, live, in a chip above the knob, over everything: beside
        // it, it ran over the next knob (MT-MIX-4 run 3).
        let text = db_text(level(shown).max(-60.0));
        let layer = egui::LayerId::new(egui::Order::Tooltip, id.with("live value"));
        let painter = ui.ctx().layer_painter(layer);
        let bounds = ui.ctx().content_rect();
        let chip = painter.layout_no_wrap(
            text.clone(),
            FontId::proportional(theme::size::CAPTION),
            palette.text,
        );
        let width = chip.size().x + 2.0 * theme::space::XS + 2.0;
        let at = pos2(rect.center().x - width / 2.0, rect.top() - 11.0);
        super::draw_hint(&painter, at, &text, bounds);
    }
    out
}

/// Reads a typed level: "-6", "−6 dB", "-inf"; below −60 is silence.
fn parse(text: &str) -> Option<f32> {
    let text = text.trim().replace('\u{2212}', "-").to_ascii_lowercase();
    let text = text.trim_end_matches("db").trim();
    if text == "-inf" || text == "-\u{221E}" || text == "off" {
        return Some(f32::NEG_INFINITY);
    }
    let db: f32 = text.replace(',', ".").parse().ok()?;
    Some(if db <= FLOOR_DB {
        f32::NEG_INFINITY
    } else {
        db.min(MAX_DB)
    })
}

/// The point at `t` (0 to 1) along the ring of radius `r`.
fn on_ring(center: Pos2, r: f32, t: f32) -> Pos2 {
    let angle = START + SWEEP * t;
    center + vec2(angle.cos(), angle.sin()) * r
}

/// Paints the knob: the ring, the comet's trail and head, the letter and
/// the pre-fader dot. `hot` (0 to 1) brightens the head and rings the knob
/// in its colour.
fn draw(painter: &egui::Painter, rect: Rect, knob: &SendKnob<'_>, at: f32, hot: f32) {
    let palette = *theme::p();
    let look = theme::controls(knob.selected);
    let center = rect.center();
    let r = rect.width() / 2.0 - 2.0;
    let width = (r * 0.28).max(1.8);
    // The ring, all the way round its sweep: two steps up on a selected
    // row, where the plain control colour vanishes into the row.
    let ring_color = if knob.selected {
        // A quarter of the way to the text colour: lighter on dark, darker
        // on light, clear of the selected row either way.
        look.fill.lerp_to_gamma(palette.text, 0.25)
    } else {
        look.fill
    };
    let ring: Vec<Pos2> = (0..=32)
        .map(|i| on_ring(center, r, i as f32 / 32.0))
        .collect();
    painter.add(Shape::line(ring, Stroke::new(width, ring_color)));
    let strength = if knob.heard { 1.0 } else { 0.4 };
    if at > 0.0 {
        // The trail: faint at the start, full at the head, and thicker
        // towards it.
        let pieces = 24;
        for i in 0..pieces {
            let (a, b) = (i as f32 / pieces as f32, (i + 1) as f32 / pieces as f32);
            let alpha = (0.18 + 0.82 * b * b) * strength;
            let points = vec![on_ring(center, r, a * at), on_ring(center, r, b * at)];
            painter.add(Shape::line(
                points,
                Stroke::new(width * (0.7 + 0.6 * b), knob.color.gamma_multiply(alpha)),
            ));
        }
        // The head: a soft glow, then a bright core in the colour.
        let head = on_ring(center, r, at);
        let glow = 1.0 + hot;
        for (radius, alpha) in [(width * 2.4 * glow, 0.14), (width * 1.6 * glow, 0.3)] {
            painter.circle_filled(head, radius, knob.color.gamma_multiply(alpha * strength));
        }
        painter.circle_filled(head, width * 1.0, Color32::WHITE.gamma_multiply(strength));
        painter.circle_filled(head, width * 0.62, knob.color);
    }
    if hot > 0.0 {
        painter.circle_stroke(
            center,
            r + width + 1.5,
            Stroke::new(1.0, knob.color.gamma_multiply(0.5 * hot)),
        );
    }
    let ink = match (at > 0.0, knob.selected) {
        (true, _) => palette.text,
        (false, true) => palette.text_secondary,
        (false, false) => palette.text_hint,
    };
    painter.text(
        center,
        Align2::CENTER_CENTER,
        knob.letter,
        fonts::medium(theme::size::CAPTION - 1.0),
        ink,
    );
    if knob.pre_fader {
        painter.circle_filled(
            pos2(center.x, center.y - r - width),
            1.6,
            palette.text_secondary,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_travel_puts_unity_at_three_quarters_and_silence_at_the_bottom() {
        assert_eq!(position(f32::NEG_INFINITY), 0.0);
        assert_eq!(position(-80.0), 0.0);
        assert!((position(0.0) - 0.75).abs() < 1e-6);
        assert!((position(MAX_DB) - 1.0).abs() < 1e-4);
        assert_eq!(level(0.0), f32::NEG_INFINITY);
        assert_eq!(level(floor()), f32::NEG_INFINITY, "the bottom is silence");
        assert!(level(floor() + 1e-4) > -60.1);
        for db in [-48.0, -24.0, -12.0, -6.0, 0.0, 3.0, 6.0] {
            assert!((level(position(db)) - db).abs() < 0.01, "{db}");
        }
        // Monotonic.
        let mut last = f32::NEG_INFINITY;
        for i in 0..=100 {
            let db = level(i as f32 / 100.0);
            assert!(db >= last);
            last = db;
        }
    }

    #[test]
    fn typed_levels_are_understood() {
        assert_eq!(parse("-6"), Some(-6.0));
        assert_eq!(parse("\u{2212}12 dB"), Some(-12.0));
        assert_eq!(parse("-inf"), Some(f32::NEG_INFINITY));
        assert_eq!(parse("-70"), Some(f32::NEG_INFINITY));
        assert_eq!(parse("12"), Some(MAX_DB));
        assert_eq!(parse("loud"), None);
    }
}
