//! A card's parameter strip, footer words and rows.

use super::*;

/// The strip's knobs, or the message of a card that is not running; returns
/// the button pressed, if any, and what the knobs asked for.
pub(super) fn paint_strip(
    ui: &mut Ui,
    area: Rect,
    id: egui::Id,
    look: &Look<'_>,
    key_room: f32,
) -> (Option<Action>, Vec<strip::Output>) {
    let palette = *theme::p();
    let no_strip = |action| (action, Vec::new());
    match look.face {
        Face::Loading => {
            // Waiting: the empty troughs the knobs will stand in.
            let painter = ui.painter().with_clip_rect(area.expand(2.0));
            let gap = 6.0;
            let row = ((area.height() - gap * (ROWS as f32 - 1.0)) / ROWS as f32)
                .clamp(0.0, theme::size::CONTROL);
            for i in 0..ROWS {
                let top = area.top() + i as f32 * (row + gap);
                let r = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), row));
                raised::paint_sunken(&painter, r, theme::radius::SMALL);
            }
            no_strip(None)
        }
        Face::Ready if look.slots.is_empty() && look.parameters == 0 => {
            let words = if look.reports_latency {
                "This plugin lists no parameters. Its own window has them."
            } else {
                "The sound is chosen from the track's Instrument menu."
            };
            let painter = ui.painter().with_clip_rect(area);
            let font = FontId::proportional(theme::size::CAPTION);
            let galley = painter.layout(
                words.to_owned(),
                font,
                palette.text_hint,
                area.width() - 2.0 * theme::space::S,
            );
            painter.galley(
                area.left_top() + vec2(theme::space::S, 0.0),
                galley,
                palette.text_hint,
            );
            // A plugin has its own window; the synth has none.
            let outputs = if look.reports_latency {
                let row =
                    Rect::from_min_max(pos2(area.left(), area.bottom() - strip::BUTTONS), area.max);
                let buttons = strip::Buttons {
                    editor: look.editor,
                    learning: look.learning,
                    parameters: 0,
                };
                strip::buttons(ui, row, id, buttons)
            } else {
                Vec::new()
            };
            (None, outputs)
        }
        Face::Ready => (
            None,
            strip::paint(
                ui,
                area,
                id,
                look.slots,
                look.selected,
                strip::Buttons {
                    editor: look.editor,
                    learning: look.learning,
                    parameters: look.parameters,
                },
                key_room,
            ),
        ),
        Face::Failed(ref failure) => {
            let (title, words, buttons): (_, String, Vec<(&str, Action)>) = if failure.keeps {
                (
                    "Keeps stopping",
                    format!(
                        "{} It has stopped three times in two minutes. Reload starts it again from {}. Reload without sound starts it clean; the saved sound stays until you save.",
                        failure.words, failure.from
                    ),
                    vec![
                        ("Reload", Action::Reload),
                        ("Reload without sound", Action::ReloadClean),
                    ],
                )
            } else {
                (
                    "Stopped responding",
                    format!(
                        "{} Reload starts it again from {}.",
                        failure.words, failure.from
                    ),
                    vec![("Reload", Action::Reload)],
                )
            };
            no_strip(message(
                ui,
                area,
                id,
                title,
                palette.warning,
                &words,
                &buttons,
            ))
        }
        Face::Missing { saved } => no_strip(message(
            ui,
            area,
            id,
            "Not installed",
            palette.text,
            &format!("Settings kept ({}).", kilobytes(saved)),
            &[("Rescan plugins", Action::Rescan)],
        )),
        Face::Safe { saved } => {
            // The card keeps its shape: the knobs stand there, greyed, and
            // the words and the one button take their place under them.
            let painter = ui.painter().with_clip_rect(area);
            let ghost = palette.control_press.gamma_multiply(0.5);
            let radius = theme::size::KNOB / 2.0;
            let step = area.width() / 4.0;
            for i in 0..4 {
                let centre = pos2(
                    area.left() + step * (i as f32 + 0.5),
                    area.top() + radius + 2.0,
                );
                painter.circle_stroke(centre, radius - 2.0, Stroke::new(3.0, ghost));
            }
            let below = Rect::from_min_max(
                pos2(area.left(), area.top() + 2.0 * radius + 10.0),
                area.max,
            );
            no_strip(message(
                ui,
                below,
                id,
                "Not loaded \u{00B7} safe mode",
                palette.text,
                &format!("Its settings ({}) are kept.", kilobytes(saved)),
                &[("Load this plugin", Action::LoadOne)],
            ))
        }
    }
}

pub(super) fn kilobytes(bytes: usize) -> String {
    format!("{} KB", bytes.div_ceil(1024))
}

/// A title, the words and buttons, in the strip's place. The buttons are
/// pinned to the strip's bottom, one above the other with the first on top;
/// the words take the room between, end in "\u{2026}" when they do not fit, and
/// are whole in the tooltip. Returns the action of the button pressed.
pub(super) fn message(
    ui: &mut Ui,
    area: Rect,
    id: egui::Id,
    title: &str,
    title_colour: Color32,
    words: &str,
    buttons: &[(&str, Action)],
) -> Option<Action> {
    let palette = *theme::p();
    let painter = ui.painter().with_clip_rect(area);
    painter.text(
        area.left_top(),
        Align2::LEFT_TOP,
        title,
        fonts::medium(theme::size::BODY),
        title_colour,
    );
    let gap = theme::space::XS;
    let height = theme::size::CONTROL;
    let top_of_buttons = area.bottom()
        - buttons.len() as f32 * height
        - buttons.len().saturating_sub(1) as f32 * gap;

    // The words, in the room above the buttons.
    let font = FontId::proportional(theme::size::CAPTION);
    let words_top = area.top() + 20.0;
    let room = top_of_buttons - theme::space::XS - words_top;
    let line = painter.fonts_mut(|f| f.row_height(&font));
    let mut job =
        egui::text::LayoutJob::simple(words.to_owned(), font, palette.text_secondary, area.width());
    job.wrap.max_rows = ((room / line).floor() as usize).max(1);
    let galley = painter.layout_job(job);
    let words_rect = Rect::from_min_size(pos2(area.left(), words_top), galley.size());
    painter.galley(words_rect.min, galley, palette.text_secondary);
    let hover = ui.interact(
        Rect::from_min_max(
            pos2(area.left(), area.top()),
            pos2(area.right(), top_of_buttons),
        ),
        id.with("words"),
        Sense::hover(),
    );
    hover.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{title}. {words}"))
    });
    hover.on_hover_text(words);

    let mut pressed = None;
    for (i, (label, action)) in buttons.iter().enumerate() {
        let size = vec2((label.len() as f32 * 7.0 + 28.0).min(area.width()), height);
        let top = top_of_buttons + i as f32 * (height + gap);
        let button_rect = Rect::from_min_size(pos2(area.left(), top), size)
            .round_to_pixels(ui.pixels_per_point());
        let response = ui.interact(button_rect, id.with(label), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, *label));
        let face = raised::face_of(ui, &response);
        let body = raised::paint(
            ui.painter(),
            button_rect,
            theme::radius::for_height(button_rect.height()),
            &face,
        );
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(theme::size::BODY),
            face.text(),
        );
        raised::focus_ring(ui.painter(), &response, button_rect);
        if response.clicked() {
            pressed = Some(*action);
        }
    }
    pressed
}

/// The preset field in the card's second header row: an arrow back, the name
/// of the sound with a dot when it was changed, an arrow forward. The field
/// opens the menu. Returns the field's response and the arrow pressed.
pub(super) fn paint_field(
    ui: &mut Ui,
    row: Rect,
    id: egui::Id,
    look: &Look<'_>,
    field: &Field<'_>,
) -> (Option<Response>, Option<bool>) {
    let palette = *theme::p();
    let height = theme::size::CONTROL;
    let inner = Rect::from_center_size(
        row.center(),
        vec2(row.width() - 2.0 * theme::space::M, height),
    )
    .round_to_pixels(ui.pixels_per_point());
    let arrow = theme::size::CONTROL;
    let previous = Rect::from_min_size(inner.min, vec2(arrow, height));
    let next = Rect::from_min_size(
        pos2(inner.right() - arrow, inner.top()),
        vec2(arrow, height),
    );
    let middle = Rect::from_min_max(
        pos2(previous.right() + 4.0, inner.top()),
        pos2(next.left() - 4.0, inner.bottom()),
    );

    let mut step = None;
    for (rect, forward) in [(previous, false), (next, true)] {
        let name = format!(
            "{} preset of {}",
            if forward { "Next" } else { "Previous" },
            look.name
        );
        let response = ui.interact(rect, id.with(("preset step", forward)), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        let face = raised::face_of(ui, &response);
        let body = raised::paint(
            ui.painter(),
            rect,
            theme::radius::for_height(rect.height()),
            &face,
        );
        let c = body.center();
        let (dx, tip) = if forward { (-2.0, 3.0) } else { (2.0, -3.0) };
        ui.painter().add(egui::Shape::line(
            vec![
                pos2(c.x + dx, c.y - 4.0),
                pos2(c.x + dx + tip, c.y),
                pos2(c.x + dx, c.y + 4.0),
            ],
            Stroke::new(1.5, face.text()),
        ));
        raised::focus_ring(ui.painter(), &response, rect);
        if response
            .on_hover_text(if forward {
                "Next preset"
            } else {
                "Previous preset"
            })
            .clicked()
        {
            step = Some(forward);
        }
    }

    let response = ui.interact(middle, id.with("preset field"), Sense::click());
    let label = format!("Preset of {}", look.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let open = widgets::Menu::is_open_for(ui.ctx(), response.id);
    let hover = motion::hover(
        ui.ctx(),
        id.with("preset hover"),
        response.hovered() || open,
    );
    raised::paint_field(
        ui.painter(),
        middle,
        theme::radius::for_height(middle.height()),
        hover,
    );
    let painter = ui.painter().with_clip_rect(middle);
    // The chevron at the right, the dot and the name from the left.
    let c = pos2(middle.right() - 12.0, middle.center().y);
    painter.add(egui::Shape::line(
        vec![
            pos2(c.x - 3.5, c.y - 1.5),
            pos2(c.x, c.y + 2.0),
            pos2(c.x + 3.5, c.y - 1.5),
        ],
        Stroke::new(1.5, palette.text_secondary),
    ));
    let mut left = middle.left() + theme::space::S;
    if field.changed {
        painter.circle_filled(pos2(left + 3.0, middle.center().y), 3.0, palette.accent);
        left += 11.0;
    }
    let font = FontId::proportional(theme::size::CAPTION);
    let room = c.x - 10.0 - left;
    let (text, ink) = match field.name {
        Some(name) => (name, palette.text),
        None => ("Preset", palette.text_hint),
    };
    let shown = widgets::fit(&painter, text, font.clone(), room);
    painter.text(
        pos2(left, middle.center().y),
        Align2::LEFT_CENTER,
        shown,
        font,
        ink,
    );
    raised::focus_ring(ui.painter(), &response, middle);
    let response = response.on_hover_text(match (field.name, field.changed) {
        (Some(name), true) => format!("{name} (changed). Choose a preset"),
        (Some(name), false) => format!("{name}. Choose a preset"),
        (None, _) => "Choose a preset".to_owned(),
    });
    (Some(response), step)
}
