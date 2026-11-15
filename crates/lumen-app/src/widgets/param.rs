use eframe::egui::{
    self, Align2, Color32, FontId, Id as EguiId, Key, Pos2, Rect, Response, Sense, Shape, Stroke,
    Ui, Vec2, pos2, vec2,
};

use super::knob::{SendKnob, draw};
use super::raised;
use crate::{motion, theme};

#[derive(Debug, Clone, Copy)]
pub struct ParamLook<'a> {
    pub name: &'a str,
    pub text: &'a str,
    pub value: f64,
    pub default: f64,
    pub steps: i32,
    pub read_only: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ParamOut {
    pub value: Option<f64>,
    pub hovered: bool,
    pub active: bool,
    pub typed: Option<String>,
    pub typing: bool,
}

pub fn start_typing(ui: &Ui, id: EguiId, text: &str) {
    let text = text.to_owned();
    ui.data_mut(|d| {
        d.insert_temp(id.with("typing"), text);
        d.insert_temp(id.with("select"), true);
    });
    ui.ctx().request_repaint();
}

fn starts_typing(c: char) -> bool {
    c.is_ascii_digit() || "-+.,".contains(c)
}

fn typing_field(
    ui: &mut Ui,
    field: Rect,
    id: EguiId,
    name: &str,
    out: &mut ParamOut,
) -> Option<Response> {
    let typing_id = id.with("typing");
    let mut text = ui.data(|d| d.get_temp::<String>(typing_id))?;
    let edit_id = id.with("edit");
    let edit = ui.put(
        field,
        egui::TextEdit::singleline(&mut text)
            .id(edit_id)
            .font(FontId::monospace(theme::size::MONO_SMALL))
            .horizontal_align(egui::Align::Center)
            .margin(vec2(2.0, 2.0)),
    );
    edit.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::TextEdit,
            true,
            format!("{name}: type a value"),
        )
    });
    if edit.has_focus()
        && ui
            .data_mut(|d| d.remove_temp::<bool>(id.with("select")))
            .unwrap_or(false)
    {
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), edit_id).unwrap_or_default();
        let end = egui::text::CCursor::new(text.chars().count());
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                end,
            )));
        state.store(ui.ctx(), edit_id);
        ui.ctx().request_repaint();
    }
    if !edit.has_focus() && !edit.lost_focus() {
        edit.request_focus();
    }
    let cancel = ui.input(|i| i.key_pressed(Key::Escape));
    if cancel || edit.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(typing_id));
        if !cancel && !text.trim().is_empty() {
            out.typed = Some(text.trim().to_owned());
        }
    } else {
        ui.data_mut(|d| d.insert_temp(typing_id, text));
        out.typing = true;
    }
    out.active = true;
    Some(edit)
}

pub fn snap(value: f64, steps: i32) -> f64 {
    let v = if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if steps > 0 {
        (v * f64::from(steps)).round() / f64::from(steps)
    } else {
        v
    }
}

pub fn step_by(value: f64, notches: f64, fine: bool, steps: i32) -> f64 {
    if steps > 0 {
        let at = (snap(value, steps) * f64::from(steps)).round() + notches.signum();
        (at / f64::from(steps)).clamp(0.0, 1.0)
    } else {
        let per = if fine { 0.002 } else { 0.02 };
        (value + notches * per).clamp(0.0, 1.0)
    }
}

pub fn travel_after(travel: f64, delta: f64, span: f64, fine: bool) -> f64 {
    (travel + delta / span.max(1.0) * if fine { 0.1 } else { 1.0 }).clamp(0.0, 1.0)
}

struct Feel<'a> {
    field: Rect,
    span: f64,
    along: fn(Vec2) -> f32,
    hint: &'a str,
}

fn behave(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    look: &ParamLook<'_>,
    feel: Feel<'_>,
) -> (Response, ParamOut) {
    let Feel {
        field,
        span,
        along,
        hint,
    } = feel;
    let mut out = ParamOut::default();
    if !look.read_only
        && let Some(edit) = typing_field(ui, field, id, look.name, &mut out)
        && out.typing
    {
        return (edit, out);
    }
    let sense = if look.read_only {
        Sense::hover()
    } else {
        Sense::click_and_drag()
    };
    let response = ui.interact(rect, id, sense);
    response.widget_info(|| {
        egui::WidgetInfo::slider(
            !look.read_only,
            look.value,
            format!("{}, parameter: {}", look.name, look.text),
        )
    });
    out.hovered = response.hovered() || response.dragged();
    if look.read_only {
        let response = response.on_hover_text(format!("{}: {} (read only)", look.name, look.text));
        return (response, out);
    }
    out.active = response.dragged() || response.has_focus();
    let fine = ui.input(|i| i.modifiers.shift);
    let drag_id = id.with("drag");
    let mut next = None;
    if response.dragged()
        && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
    {
        let (travel, last) = ui
            .data(|d| d.get_temp::<(f64, Pos2)>(drag_id))
            .unwrap_or_else(|| {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
                (look.value.clamp(0.0, 1.0), origin)
            });
        let travel = travel_after(travel, f64::from(along(pointer - last)), span, fine);
        ui.data_mut(|d| d.insert_temp(drag_id, (travel, pointer)));
        next = Some(snap(travel, look.steps));
    } else {
        ui.data_mut(|d| d.remove::<(f64, Pos2)>(drag_id));
    }
    if response.clicked() {
        response.request_focus();
    }
    if response.has_focus() {
        let (down, up, opened, typed) = ui.input(|i| {
            let typed: String = i
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(t) if t.chars().all(starts_typing) => Some(t.as_str()),
                    _ => None,
                })
                .collect();
            (
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::Enter) || i.key_pressed(Key::F2),
                typed,
            )
        });
        if !typed.is_empty() {
            ui.data_mut(|d| d.insert_temp(id.with("typing"), typed));
            ui.ctx().request_repaint();
        } else if opened {
            start_typing(ui, id, look.text);
        }
        let base = next.unwrap_or(look.value);
        if down {
            next = Some(step_by(base, -1.0, fine, look.steps));
        }
        if up {
            next = Some(step_by(base, 1.0, fine, look.steps));
        }
    }
    let notches = super::wheel::steps(ui, id, rect);
    if notches != 0.0 {
        let base = next.unwrap_or(look.value);
        next = Some(step_by(base, f64::from(notches), fine, look.steps));
    }
    if response.double_clicked() {
        next = Some(snap(look.default, look.steps));
    }
    out.value = next.filter(|v| (*v - look.value).abs() > 1e-9);
    let response = response.on_hover_text(format!("{}: {}. {hint}", look.name, look.text));
    (response, out)
}

#[derive(Debug, Clone, Copy)]
pub struct DialLook {
    pub hue: Color32,
    pub radius: f32,
}

pub fn param_dial(
    ui: &mut Ui,
    centre: Pos2,
    id: EguiId,
    look: ParamLook<'_>,
    dial: DialLook,
) -> (Response, ParamOut) {
    let rect = Rect::from_center_size(centre, Vec2::splat(2.0 * dial.radius + 4.0));
    let (response, out) = behave(
        ui,
        rect,
        id,
        &look,
        Feel {
            field: Rect::from_center_size(centre, vec2(60.0, 18.0)),
            span: 160.0,
            along: |d| d.x - d.y,
            hint: "Drag up or right (Shift for fine), double-click for the default, or click and type",
        },
    );
    if out.typing {
        return (response, out);
    }
    let shown = out.value.unwrap_or(look.value) as f32;
    let hot = motion::hover(ui.ctx(), id.with("hot"), out.hovered);
    let held = motion::hover(ui.ctx(), id.with("held"), response.dragged());
    paint_dial(
        ui.painter(),
        centre,
        dial,
        (shown, hot, held),
        look.read_only,
    );
    if response.has_focus() {
        raised::paint_focus(ui.painter(), rect, theme::radius::FULL);
    }
    (response, out)
}

fn paint_dial(
    painter: &egui::Painter,
    centre: Pos2,
    dial: DialLook,
    (at, hot, held): (f32, f32, f32),
    off: bool,
) {
    use std::f32::consts::PI;
    use theme::personality::card as c;
    let (start, sweep) = (0.75 * PI, 1.5 * PI);
    let r = dial.radius;
    let on_ring = |t: f32, radius: f32| {
        let angle = start + sweep * t;
        centre + vec2(angle.cos(), angle.sin()) * radius
    };
    let arc = |to: f32| -> Vec<Pos2> {
        let pieces = ((to * 48.0).ceil() as usize).max(1);
        (0..=pieces)
            .map(|i| on_ring(to * i as f32 / pieces as f32, r))
            .collect()
    };
    let strength = if off { 0.4 } else { 1.0 };
    let (inner, rim) = if theme::is_light() {
        c::DIAL_DISC_LIGHT
    } else {
        c::DIAL_DISC_DARK
    };
    let disc = r - c::DIAL_DISC_INSET;
    let focus = centre + vec2(-0.2 * disc, -0.4 * disc);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(focus, inner);
    const SIDES: u32 = 28;
    for i in 0..SIDES {
        let a = i as f32 / SIDES as f32 * 2.0 * PI;
        let at_rim = centre + vec2(a.cos(), a.sin()) * disc;
        let reach = ((at_rim - focus).length() / (1.4 * disc)).clamp(0.0, 1.0);
        mesh.colored_vertex(at_rim, inner.lerp_to_gamma(rim, reach));
    }
    for i in 0..SIDES {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % SIDES);
    }
    painter.add(Shape::mesh(mesh));
    painter.circle_stroke(
        centre,
        disc,
        Stroke::new(
            theme::stroke::HAIRLINE,
            Color32::BLACK.gamma_multiply(if theme::is_light() { 0.12 } else { 0.4 }),
        ),
    );
    painter.add(Shape::line(
        arc(1.0),
        Stroke::new(
            c::DIAL_TRACK,
            dial.hue.gamma_multiply(c::DIAL_TRACK_ALPHA * strength),
        ),
    ));
    if at > 0.0 {
        let lit = arc(at);
        for (width, alpha) in c::DIAL_GLOW {
            let grow = 1.0 + c::DIAL_HELD_GROWTH * held;
            painter.add(Shape::line(
                lit.clone(),
                Stroke::new(
                    width * grow,
                    dial.hue.gamma_multiply((alpha * grow).min(1.0) * strength),
                ),
            ));
        }
        painter.add(Shape::line(
            lit,
            Stroke::new(c::DIAL_LIT, dial.hue.gamma_multiply(strength)),
        ));
    }
    painter.circle_filled(
        on_ring(at, r),
        c::DIAL_HEAD,
        Color32::WHITE.gamma_multiply(strength),
    );
    let ink = if theme::is_light() {
        c::DIAL_POINTER_LIGHT
    } else {
        c::DIAL_POINTER_DARK
    };
    painter.line_segment(
        [centre, on_ring(at, r - c::DIAL_POINTER_INSET)],
        Stroke::new(c::DIAL_POINTER, ink.gamma_multiply(strength)),
    );
    if hot > 0.0 {
        painter.circle_stroke(
            centre,
            r + c::DIAL_HEAD + 2.0,
            Stroke::new(
                theme::stroke::HAIRLINE,
                dial.hue.gamma_multiply(theme::alpha::EDGE * hot),
            ),
        );
    }
}

pub fn param_knob(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    look: ParamLook<'_>,
) -> (Response, ParamOut) {
    let (response, out) = behave(
        ui,
        rect,
        id,
        &look,
        Feel {
            field: Rect::from_center_size(rect.center(), vec2(60.0, 18.0)),
            span: 160.0,
            along: |d| d.x - d.y,
            hint: "Drag up or right (Shift for fine), double-click for the default, or click and type",
        },
    );
    if out.typing {
        return (response, out);
    }
    let shown = out.value.unwrap_or(look.value);
    let hot = motion::hover(ui.ctx(), id.with("hot"), out.hovered);
    let palette = *theme::p();
    let knob = SendKnob {
        style: super::knob::Style::Ring,
        letter: "",
        color: palette.accent,
        db: 0.0,
        pre_fader: false,
        heard: !look.read_only,
        linked: false,
        selected: look.selected,
        live_value: false,
        name: look.name,
        mapped: None,
        waiting: false,
    };
    draw(ui.painter(), rect, &knob, shown as f32, hot);
    if response.has_focus() {
        raised::paint_focus(ui.painter(), rect, theme::radius::FULL);
    }
    (response, out)
}

pub fn param_bar(ui: &mut Ui, rect: Rect, id: EguiId, look: ParamLook<'_>) -> (Response, ParamOut) {
    let (response, out) = behave(
        ui,
        rect,
        id,
        &look,
        Feel {
            field: rect,
            span: f64::from(rect.width()),
            along: |d| d.x,
            hint: "Drag (Shift for fine), double-click for the default, or click and type",
        },
    );
    if out.typing {
        return (response, out);
    }
    let shown = out.value.unwrap_or(look.value).clamp(0.0, 1.0);
    let palette = *theme::p();
    let radius = theme::radius::for_height(rect.height());
    raised::paint_sunken(ui.painter(), rect, radius);
    let fill = Rect::from_min_max(
        rect.min,
        pos2(rect.left() + rect.width() * shown as f32, rect.bottom()),
    );
    let strength = if look.read_only { 0.25 } else { 0.55 };
    ui.painter().rect_filled(
        fill.shrink2(vec2(0.0, 1.0)),
        f32::from(radius),
        palette.control_press.gamma_multiply(strength),
    );
    let hot = motion::hover(ui.ctx(), id.with("hot"), out.hovered);
    ui.painter().rect_filled(
        rect,
        f32::from(radius),
        palette
            .text
            .gamma_multiply(crate::light::EMBER_WASH * hot * theme::alpha::EDGE),
    );
    let font = FontId::monospace(theme::size::MONO_SMALL);
    let text = super::fit(ui.painter(), look.text, font.clone(), rect.width() - 10.0);
    ui.painter().text(
        pos2(rect.right() - 6.0, rect.center().y),
        Align2::RIGHT_CENTER,
        text,
        font,
        if look.read_only {
            palette.text_hint
        } else {
            palette.text
        },
    );
    if response.has_focus() {
        raised::paint_focus(ui.painter(), rect, radius);
    }
    (response, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stepped_parameter_snaps_to_its_positions() {
        assert_eq!(snap(0.4, 0), 0.4);
        assert_eq!(snap(0.4, 1), 0.0);
        assert_eq!(snap(0.6, 1), 1.0);
        assert_eq!(snap(0.5, 4), 0.5);
        assert_eq!(snap(0.3, 4), 0.25);
        assert_eq!(snap(7.0, 2), 1.0);
        assert_eq!(snap(-1.0, 0), 0.0);
        assert_eq!(snap(f64::NAN, 3), 0.0);
    }

    #[test]
    fn a_key_or_a_notch_moves_two_percent_or_one_step() {
        assert!((step_by(0.5, 1.0, false, 0) - 0.52).abs() < 1e-12);
        assert!((step_by(0.5, -1.0, true, 0) - 0.498).abs() < 1e-12);
        assert_eq!(step_by(0.99, 5.0, false, 0), 1.0);
        assert_eq!(step_by(0.0, -1.0, false, 0), 0.0);
        assert_eq!(step_by(0.0, 1.0, false, 2), 0.5);
        assert_eq!(
            step_by(0.5, 3.0, false, 2),
            1.0,
            "a step whatever the notches"
        );
        assert_eq!(step_by(1.0, 1.0, false, 2), 1.0);
        assert_eq!(
            step_by(0.5, -1.0, true, 2),
            0.0,
            "fine does not make a step smaller"
        );
    }

    #[test]
    fn a_slow_drag_on_a_stepped_parameter_adds_up() {
        let mut travel = 0.0;
        let mut shown = 0.0;
        for _ in 0..200 {
            travel = travel_after(travel, 0.4, 160.0, false);
            shown = snap(travel, 2);
        }
        assert_eq!(shown, 0.5, "{travel}");
        assert!(travel > 0.45 && travel < 0.55);
    }

    #[test]
    fn fine_travel_is_a_tenth() {
        let fine = travel_after(0.5, 16.0, 160.0, true);
        let coarse = travel_after(0.5, 16.0, 160.0, false);
        assert!((fine - 0.51).abs() < 1e-12);
        assert!((coarse - 0.6).abs() < 1e-12);
        assert_eq!(travel_after(0.99, 1e6, 160.0, false), 1.0);
        assert_eq!(travel_after(0.01, -1e6, 160.0, false), 0.0);
    }
}
