//! The browser's list: vendor headers, device rows and the blocked section,
//! painted and answered row by row. Only the rows in view are laid out, so
//! a thousand plugins cost what a dozen do. It decides nothing: what was
//! clicked goes back in an [`Out`] for the app to apply.
//!
//! Rows are clickable but not Tab stops: a library of hundreds would take
//! hundreds of Tabs to cross. The list is one stop, the pane's own (F6), and
//! the arrows move through it (`panel.rs`).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
use eframe::egui::{
    self, Color32, FontId, Galley, Id, Painter, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};
use lumen_plugin_catalog::blocklist::WHEN_RETRIED;
use lumen_plugin_catalog::catalog::{Blocked, Catalog, Device, Kind, Source};

use super::rows::{Row, Rows, selection_of};
use super::view::Selection;
use crate::icons::{self, Icon};
use crate::{fonts, motion, theme, widgets};

const DEVICE_H: f32 = 28.0;
const VENDOR_H: f32 = 26.0;
/// Room inside a row, either side.
const PAD: f32 = 6.0;
/// Below this width a row drops its category tag (the header says it).
const TAG_MIN_WIDTH: f32 = 200.0;
const ICON: f32 = 16.0;

/// What the list is showing.
pub(super) struct Input<'a> {
    pub catalog: &'a Catalog,
    pub rows: &'a Rows,
    pub selected: Option<&'a Selection>,
    pub favorites: &'a HashSet<String>,
    /// One flat list (Recent): rows say their vendor, as no header does.
    pub flat: bool,
    /// Bring the selected row into view this frame (a key moved it).
    pub scroll_to_selected: bool,
    /// What choosing an instrument and an effect does, as a menu row
    /// ("Add to Bass"), in that order.
    pub adds: [&'a str; 2],
}

/// What the pointer did.
#[derive(Default)]
pub(super) struct Out {
    pub select: Option<Selection>,
    /// Double-clicked or Enter on a device: choose it.
    pub choose: Option<String>,
    pub toggle_fold: Option<String>,
    pub toggle_blocked: bool,
    pub favorite: Option<String>,
    /// A module to forget and scan again.
    pub retry: Option<String>,
    pub reveal: Option<PathBuf>,
    pub copy: Option<String>,
    /// What the pointer is over, for the info line.
    pub hover: Option<Hover>,
}

pub(super) enum Hover {
    Device(usize),
    Vendor(String),
    Blocked(usize),
}

fn height(row: &Row, note: f32) -> f32 {
    match row {
        Row::Device(_) | Row::Blocked(_) => DEVICE_H,
        Row::Vendor { .. } | Row::BlockedHeader { .. } => VENDOR_H,
        Row::BlockedNote(_) => note,
    }
}

/// A line of `text` cut with "…" to fit `width`.
fn fit(painter: &Painter, text: &str, font: FontId, colour: Color32, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, colour));
    job.wrap = TextWrapping::truncate_at_width(width.max(1.0));
    painter.layout_job(job)
}

/// The words on a blocked module's card: what happened, what it printed,
/// and what Lumen will do.
fn note_text(blocked: &Blocked) -> (String, Option<String>) {
    (
        blocked.reason.headline(),
        blocked.reason.detail().map(str::to_owned),
    )
}

/// How tall the card under a blocked module is at `width`.
fn note_height(painter: &Painter, blocked: &Blocked, width: f32) -> f32 {
    let wrap = width - 2.0 * theme::space::M;
    let body = FontId::proportional(theme::size::CAPTION);
    let ink = theme::p().text;
    let (headline, detail) = note_text(blocked);
    let mut text = painter.layout(headline, body.clone(), ink, wrap).size().y;
    if let Some(detail) = detail {
        text += painter.layout(detail, body.clone(), ink, wrap).size().y + theme::space::XS;
    }
    text += painter
        .layout(WHEN_RETRIED.into(), body, ink, wrap)
        .size()
        .y;
    // Padding, the gaps between paragraphs, and the buttons.
    text + 2.0 * theme::space::S + theme::space::XS + theme::size::CONTROL + theme::space::S
}

/// Draws the list in `rect` and returns what happened.
pub(super) fn show(ui: &mut Ui, rect: Rect, input: &Input<'_>) -> Out {
    let mut out = Out::default();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let width = rect.width();
    let selected_row = input.rows.list.iter().position(|r| {
        selection_of(input.catalog, r).as_ref() == input.selected && input.selected.is_some()
    });

    // Heights, so the rows in view can be found without laying out the rest.
    let painter = child.painter().clone();
    let note = |row: &Row| match row {
        Row::BlockedNote(i) => {
            note_height(&painter, &input.catalog.blocked()[*i], width - 2.0 * PAD)
        }
        _ => 0.0,
    };
    let mut tops = Vec::with_capacity(input.rows.list.len());
    let mut total = 0.0;
    for row in &input.rows.list {
        tops.push(total);
        total += height(row, note(row));
    }

    egui::ScrollArea::vertical()
        .id_salt("browser list")
        .auto_shrink([false, false])
        .show_viewport(&mut child, |ui, viewport| {
            let origin = ui.cursor().min;
            ui.allocate_space(vec2(width, total));
            for (i, row) in input.rows.list.iter().enumerate() {
                let h = height(row, note(row));
                let (top, bottom) = (tops[i], tops[i] + h);
                let wanted = input.scroll_to_selected && Some(i) == selected_row;
                if !wanted && (bottom < viewport.top() || top > viewport.bottom()) {
                    continue;
                }
                let at = Rect::from_min_size(
                    pos2(origin.x + PAD, origin.y + top),
                    vec2(width - 2.0 * PAD - scroll_room(), h),
                );
                if wanted {
                    ui.scroll_to_rect(at, None);
                }
                draw_row(ui, at, row, input, &mut out);
            }
        });
    out
}

/// Room kept at the right for the scroll bar.
fn scroll_room() -> f32 {
    theme::space::XS
}

fn draw_row(ui: &mut Ui, rect: Rect, row: &Row, input: &Input<'_>, out: &mut Out) {
    match row {
        Row::Vendor {
            name,
            count,
            folded,
        } => draw_header(
            ui,
            Header {
                rect,
                id: Id::new(("browser vendor", name.to_lowercase())),
                name,
                count: *count,
                folded: *folded,
                selected: input.selected == Some(&Selection::Vendor(name.to_lowercase())),
                select: Selection::Vendor(name.to_lowercase()),
                hover: Hover::Vendor(name.clone()),
            },
            |out| out.toggle_fold = Some(name.clone()),
            out,
        ),
        Row::BlockedHeader { count, folded } => draw_header(
            ui,
            Header {
                rect,
                id: Id::new("browser blocked header"),
                name: "Blocked",
                count: *count,
                folded: *folded,
                selected: input.selected == Some(&Selection::BlockedHeader),
                select: Selection::BlockedHeader,
                hover: Hover::Vendor("blocked".into()),
            },
            |out| out.toggle_blocked = true,
            out,
        ),
        Row::Device(i) => draw_device(ui, rect, &input.catalog.devices()[*i], *i, input, out),
        Row::Blocked(i) => draw_blocked(ui, rect, &input.catalog.blocked()[*i], *i, input, out),
        Row::BlockedNote(i) => draw_note(ui, rect, &input.catalog.blocked()[*i], out),
    }
}

/// What a header shows and what choosing it means.
struct Header<'a> {
    rect: Rect,
    id: Id,
    name: &'a str,
    count: usize,
    folded: bool,
    selected: bool,
    select: Selection,
    hover: Hover,
}

/// A header the pointer folds: an arrow that turns, a name, a count.
fn draw_header(ui: &mut Ui, header: Header<'_>, fold: impl FnOnce(&mut Out), out: &mut Out) {
    let Header {
        rect,
        id,
        name,
        count,
        folded,
        selected,
        select,
        hover,
    } = header;
    let palette = *theme::p();
    let response = ui.interact(rect, id, Sense::CLICK);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!(
                "{name}, {count} {}, {}",
                if count == 1 { "device" } else { "devices" },
                if folded { "folded" } else { "open" }
            ),
        )
    });
    let hover_t = motion::hover(ui.ctx(), id.with("hover"), response.hovered());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::radius::MEDIUM, palette.selected_row);
    }
    painter.rect_filled(
        rect,
        theme::radius::MEDIUM,
        palette.ember.gamma_multiply(hover_t),
    );
    let open = motion::glide(
        ui.ctx(),
        id.with("fold"),
        if folded { 0.0 } else { 1.0 },
        true,
    );
    widgets::draw_arrow(
        painter,
        pos2(rect.left() + PAD + 4.0, rect.center().y),
        open,
        palette
            .text_hint
            .lerp_to_gamma(palette.text_secondary, hover_t),
    );
    let count_text = count.to_string();
    let count_galley = painter.layout_no_wrap(
        count_text,
        FontId::monospace(theme::size::MONO_SMALL),
        palette.text_hint,
    );
    let right = rect.right() - PAD - count_galley.size().x;
    let label_left = rect.left() + PAD + 16.0;
    let label = fit(
        painter,
        name,
        fonts::medium(theme::size::CAPTION),
        palette.text_secondary,
        right - label_left - theme::space::S,
    );
    painter.galley(
        pos2(label_left, rect.center().y - label.size().y / 2.0),
        label,
        palette.text_secondary,
    );
    painter.galley(
        pos2(right, rect.center().y - count_galley.size().y / 2.0),
        count_galley,
        palette.text_hint,
    );
    if response.hovered() {
        out.hover = Some(hover);
    }
    if response.clicked() {
        fold(out);
        out.select = Some(select);
    }
}

fn draw_device(
    ui: &mut Ui,
    rect: Rect,
    device: &Device,
    device_index: usize,
    input: &Input<'_>,
    out: &mut Out,
) {
    let palette = *theme::p();
    let id = Id::new(("browser device", &device.id));
    let selected = input.selected == Some(&Selection::Device(device.id.clone()));
    let favorite = input.favorites.contains(&device.id);
    let response = ui.interact(rect, id, Sense::click_and_drag());
    // Dragged onto the Devices tab it is added where the line shows.
    response.dnd_set_drag_payload(crate::chain::Dragged(device.id.clone()));
    if response.dragged() {
        crate::chain::paint_drag_ghost(ui.ctx(), &device.name);
    }
    let kind = match device.kind {
        Kind::Instrument => "instrument",
        Kind::Effect => "effect",
    };
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            format!("{}, {}, {kind}", device.name, device.vendor),
        )
    });
    // The star sits over the row's right end and takes its own clicks.
    let star = Rect::from_center_size(
        pos2(rect.right() - PAD - ICON / 2.0, rect.center().y),
        vec2(ICON + 8.0, DEVICE_H),
    );
    let star_response = ui.interact(star, id.with("star"), Sense::CLICK);
    star_response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            favorite,
            format!("Favorite {}", device.name),
        )
    });
    let star_response = widgets::tip(
        star_response,
        if favorite {
            "Remove from favorites"
        } else {
            "Add to favorites"
        },
        None,
        None,
    );

    let hovered = response.hovered() || star_response.hovered();
    let hover_t = motion::hover(ui.ctx(), id.with("hover"), hovered);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::radius::MEDIUM, palette.selected_row);
    }
    painter.rect_filled(
        rect,
        theme::radius::MEDIUM,
        palette.ember.gamma_multiply(hover_t),
    );
    let ink = if selected {
        palette.text
    } else {
        palette.text_secondary
    };

    let icon_rect = Rect::from_center_size(
        pos2(rect.left() + PAD + ICON / 2.0, rect.center().y),
        vec2(ICON, ICON),
    );
    painter.extend(icons::shapes(
        match device.kind {
            Kind::Instrument => Icon::Keys,
            Kind::Effect => Icon::Knob,
        },
        icon_rect,
        ink,
    ));

    // Right to left: the star, then what the row says of itself.
    let star_ink = if favorite {
        palette.accent
    } else if hovered || selected {
        palette.text_secondary
    } else {
        palette.text_disabled
    };
    painter.extend(icons::shapes(
        if favorite {
            Icon::StarFilled
        } else {
            Icon::Star
        },
        Rect::from_center_size(star.center(), vec2(ICON, ICON)),
        star_ink,
    ));
    let mut right = star.left() - theme::space::XS;
    let caption = if input.flat {
        Some(device.vendor.as_str())
    } else if rect.width() >= TAG_MIN_WIDTH {
        device.categories.first().map(String::as_str)
    } else {
        None
    };
    if let Some(caption) = caption {
        right = tag(
            painter,
            caption,
            right,
            rect.center().y,
            input.flat,
            &palette,
        );
    }
    let name_left = icon_rect.right() + theme::space::S;
    let name = fit(
        painter,
        &device.name,
        FontId::proportional(theme::size::BODY),
        palette.text,
        right - name_left - theme::space::S,
    );
    painter.galley(
        pos2(name_left, rect.center().y - name.size().y / 2.0),
        name,
        palette.text,
    );

    if response.hovered() || star_response.hovered() {
        out.hover = Some(Hover::Device(device_index));
    }
    if star_response.clicked() {
        out.favorite = Some(device.id.clone());
        out.select = Some(Selection::Device(device.id.clone()));
    } else if response.clicked() {
        out.select = Some(Selection::Device(device.id.clone()));
    }
    if response.double_clicked() && !star_response.hovered() {
        out.choose = Some(device.id.clone());
    }
    // The owner's A (the interface audit's B7): what choosing it does
    // first, then favouring it, then its file.
    widgets::Menu::context(&response).show(|ui| {
        if crate::chain::placeable(device) {
            let add = match device.kind {
                Kind::Instrument => input.adds[0],
                Kind::Effect => input.adds[1],
            };
            if widgets::menu_item(ui, add, None, true).clicked() {
                out.choose = Some(device.id.clone());
            }
            ui.separator();
        }
        let label = if favorite {
            "Remove from favorites"
        } else {
            "Add to favorites"
        };
        if widgets::menu_item(ui, label, None, true).clicked() {
            out.favorite = Some(device.id.clone());
        }
        if let (Source::Vst3, Some(module)) = (device.source, &device.module) {
            ui.separator();
            if widgets::menu_item(ui, "Rescan this plugin", None, true).clicked() {
                out.retry = Some(module.to_string_lossy().into_owned());
            }
            if widgets::menu_item(ui, "Show in folder", None, true).clicked() {
                out.reveal = Some(module.clone());
            }
            if widgets::menu_item(ui, "Copy path", None, true).clicked() {
                out.copy = Some(module.to_string_lossy().into_owned());
            }
        }
    });
}

/// A small outlined word at the row's right, ending at `right`; returns
/// where it starts.
fn tag(
    painter: &Painter,
    text: &str,
    right: f32,
    y: f32,
    plain: bool,
    palette: &theme::Palette,
) -> f32 {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        FontId::proportional(theme::size::SCALE),
        palette.text_hint,
    );
    let pad = 4.0;
    let width = galley.size().x + 2.0 * pad;
    let rect = Rect::from_min_size(pos2(right - width, y - 7.0), vec2(width, 14.0));
    if !plain {
        painter.rect_stroke(
            rect,
            theme::radius::SMALL,
            Stroke::new(theme::stroke::HAIRLINE, palette.border_strong),
            StrokeKind::Inside,
        );
    }
    painter.galley(
        pos2(rect.left() + pad, y - galley.size().y / 2.0),
        galley,
        palette.text_hint,
    );
    rect.left() - theme::space::XS
}

fn draw_blocked(
    ui: &mut Ui,
    rect: Rect,
    blocked: &Blocked,
    index: usize,
    input: &Input<'_>,
    out: &mut Out,
) {
    let palette = *theme::p();
    let path = blocked.path.to_string_lossy().into_owned();
    let id = Id::new(("browser blocked", &path));
    let selected = input.selected == Some(&Selection::Blocked(path.clone()));
    let response = ui.interact(rect, id, Sense::CLICK);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            format!("{}, blocked", blocked.name),
        )
    });
    let hover_t = motion::hover(ui.ctx(), id.with("hover"), response.hovered());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::radius::MEDIUM, palette.selected_row);
    }
    painter.rect_filled(
        rect,
        theme::radius::MEDIUM,
        palette.ember.gamma_multiply(hover_t),
    );
    let icon_rect = Rect::from_center_size(
        pos2(rect.left() + PAD + ICON / 2.0, rect.center().y),
        vec2(ICON, ICON),
    );
    painter.extend(icons::shapes(Icon::Warning, icon_rect, palette.warning));
    let right = tag_blocked(painter, rect, &palette);
    let name_left = icon_rect.right() + theme::space::S;
    let name = fit(
        painter,
        &blocked.name,
        FontId::proportional(theme::size::BODY),
        palette.text_secondary,
        right - name_left - theme::space::S,
    );
    painter.galley(
        pos2(name_left, rect.center().y - name.size().y / 2.0),
        name,
        palette.text_secondary,
    );
    if response.hovered() {
        out.hover = Some(Hover::Blocked(index));
    }
    if response.clicked() {
        out.select = Some(Selection::Blocked(path));
    }
    widgets::Menu::context(&response).show(|ui| {
        if widgets::menu_item(ui, "Retry", None, true).clicked() {
            out.retry = Some(blocked.path.to_string_lossy().into_owned());
        }
        if widgets::menu_item(ui, "Show in folder", None, true).clicked() {
            out.reveal = Some(blocked.path.clone());
        }
        if widgets::menu_item(ui, "Copy path", None, true).clicked() {
            out.copy = Some(blocked.path.to_string_lossy().into_owned());
        }
    });
}

/// The "Blocked" word at the row's right, in the warning colour; returns
/// where it starts. The colour never stands alone: the word says it.
fn tag_blocked(painter: &Painter, rect: Rect, palette: &theme::Palette) -> f32 {
    let galley = painter.layout_no_wrap(
        "Blocked".into(),
        FontId::proportional(theme::size::SCALE),
        palette.warning,
    );
    let pad = 4.0;
    let width = galley.size().x + 2.0 * pad;
    let tag = Rect::from_min_size(
        pos2(rect.right() - PAD - width, rect.center().y - 7.0),
        vec2(width, 14.0),
    );
    painter.rect_stroke(
        tag,
        theme::radius::SMALL,
        Stroke::new(theme::stroke::HAIRLINE, palette.warning.gamma_multiply(0.6)),
        StrokeKind::Inside,
    );
    painter.galley(
        pos2(tag.left() + pad, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.warning,
    );
    tag.left() - theme::space::XS
}

/// The card under the selected blocked module: why, what Lumen does, and
/// the two things to do about it.
fn draw_note(ui: &mut Ui, rect: Rect, blocked: &Blocked, out: &mut Out) {
    let palette = *theme::p();
    let card = rect.shrink2(vec2(0.0, theme::space::XS));
    // The card's words are painted, which assistive technology cannot read:
    // the card says them as one labelled thing.
    let (headline, detail) = note_text(blocked);
    let spoken = match &detail {
        Some(detail) => format!("{headline} {detail} {WHEN_RETRIED}"),
        None => format!("{headline} {WHEN_RETRIED}"),
    };
    let id = Id::new((
        "browser blocked note",
        blocked.path.to_string_lossy().into_owned(),
    ));
    ui.interact(card, id, Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, spoken.clone()));
    let painter = ui.painter().clone();
    painter.rect(
        card,
        theme::radius::LARGE,
        palette.raised,
        Stroke::new(theme::stroke::HAIRLINE, palette.border),
        StrokeKind::Inside,
    );
    let inner = card.shrink(theme::space::S);
    let wrap = inner.width() - theme::space::XS;
    let body = FontId::proportional(theme::size::CAPTION);
    let mut y = inner.top();
    let paragraph = |text: String, colour: Color32, y: &mut f32| {
        let galley = painter.layout(text, body.clone(), colour, wrap);
        let height = galley.size().y;
        painter.galley(pos2(inner.left(), *y), galley, colour);
        *y += height;
    };
    paragraph(headline, palette.text, &mut y);
    if let Some(detail) = detail {
        y += theme::space::XS;
        paragraph(detail, palette.text_hint, &mut y);
    }
    y += theme::space::XS;
    paragraph(WHEN_RETRIED.into(), palette.text_secondary, &mut y);
    y += theme::space::S;
    let buttons = Rect::from_min_size(
        pos2(inner.left(), y),
        vec2(inner.width(), theme::size::CONTROL),
    );
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(buttons)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.spacing_mut().item_spacing.x = theme::space::XS;
    if widgets::raised::button(&mut row, "Retry").clicked() {
        out.retry = Some(blocked.path.to_string_lossy().into_owned());
    }
    if widgets::raised::button(&mut row, "Show in folder").clicked() {
        out.reveal = Some(blocked.path.clone());
    }
}
