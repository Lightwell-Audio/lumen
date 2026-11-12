//! The browser's Project media tab (M9 slice R4b, ADR-0106): every audio file
//! the project holds, which ones are missing or unused, and the actions on
//! them. It decides nothing about files: `app::media_pool` does that, and
//! this draws the state and says what was clicked. What the app does with
//! what was clicked is `app::browser::media_tab` (audit M12).

use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
use eframe::egui::{self, Color32, FontId, Id, Painter, Rect, Sense, Ui, pos2, vec2};
use lumen_project::model::Id as MediaId;

use crate::app::media_pool::Filter;
use crate::pool::Row;
use crate::{fonts, motion, theme, widgets};

pub(crate) const ROW_H: f32 = 44.0;
/// Room inside a row, either side.
pub(crate) const PAD: f32 = 6.0;
/// What the strays card needs below the list.
pub(crate) const STRAYS_H: f32 = 84.0;

/// What the pointer or a key did in the list.
#[derive(Default)]
pub(crate) struct Out {
    pub(crate) select: Option<MediaId>,
    /// Double-click or Enter: show where it is used, or locate it.
    pub(crate) open: Option<MediaId>,
    pub(crate) locate: Option<MediaId>,
    pub(crate) reveal: Option<std::path::PathBuf>,
    pub(crate) remove: Option<MediaId>,
    pub(crate) select_clips: Option<MediaId>,
}

/// The words the empty list says: nothing in the project at all, nothing
/// that matches, and the words of a filter that has nothing to say.
pub(crate) fn empty_words(
    nothing_at_all: bool,
    filter: Filter,
    words_empty: bool,
) -> (&'static str, &'static str) {
    if nothing_at_all {
        (
            "No audio in this project",
            "Drop an audio file on the arrangement to add one",
        )
    } else if filter == Filter::Missing && words_empty {
        ("No file is missing", "Everything the project names is here")
    } else if filter == Filter::Unused && words_empty {
        (
            "Every file is used",
            "Nothing here is waiting to be cleared",
        )
    } else {
        ("No files match", "Try other words, or show all files")
    }
}

/// One row: the name and where it stands, then what it is.
pub(crate) fn media_row(
    ui: &mut Ui,
    rect: Rect,
    row: &Row,
    selected: bool,
    file: Option<std::path::PathBuf>,
    project_rate: u32,
    out: &mut Out,
) {
    let palette = *theme::p();
    let id = Id::new(("media row", row.id.0));
    let response = ui.interact(rect, id, Sense::click());
    let state = state_word(row);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            format!("{}, {state}", row.name),
        )
    });
    let hover = motion::hover(ui.ctx(), id.with("hover"), response.hovered());
    let response = response.on_hover_text(if row.missing {
        format!("Not found: {}", row.path)
    } else {
        row.path.clone()
    });
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, theme::radius::MEDIUM, palette.selected_row);
    }
    painter.rect_filled(
        rect,
        theme::radius::MEDIUM,
        palette.ember.gamma_multiply(hover),
    );

    let (tag, tag_ink) = if row.missing {
        ("Missing".to_owned(), palette.warning)
    } else if row.unused() {
        ("Unused".to_owned(), palette.text_hint)
    } else if row.uses == 1 {
        ("1 clip".to_owned(), palette.text_hint)
    } else {
        (format!("{} clips", row.uses), palette.text_hint)
    };
    let tag = painter.layout_no_wrap(tag, fonts::medium(theme::size::CAPTION), tag_ink);
    let right = rect.right() - PAD - tag.size().x;
    let left = rect.left() + PAD + theme::space::XS;
    let name = fit(
        painter,
        &row.name,
        fonts::medium(theme::size::BODY),
        if row.missing {
            palette.text_secondary
        } else {
            palette.text
        },
        right - left - theme::space::S,
    );
    let first = rect.top() + 7.0;
    painter.galley(pos2(left, first), name, palette.text);
    painter.galley(pos2(right, first + 1.0), tag, tag_ink);
    let second = fit(
        painter,
        &detail_line(row, project_rate),
        FontId::proportional(theme::size::CAPTION),
        palette.text_hint,
        rect.right() - PAD - left,
    );
    painter.galley(
        pos2(left, rect.bottom() - 7.0 - second.size().y),
        second,
        palette.text_hint,
    );
    widgets::raised::focus_ring(painter, &response, rect);

    if response.clicked() {
        out.select = Some(row.id);
    }
    if response.double_clicked() {
        out.open = Some(row.id);
    }
    widgets::Menu::context(&response).show(|ui| {
        out.select = Some(row.id);
        if row.missing && widgets::menu_item(ui, "Locate\u{2026}", None, true).clicked() {
            out.locate = Some(row.id);
            ui.close();
        }
        if row.uses > 0 && widgets::menu_item(ui, "Select its clips", None, true).clicked() {
            out.select_clips = Some(row.id);
            ui.close();
        }
        if let Some(file) = &file
            && !row.missing
            && widgets::menu_item(ui, "Show in folder", None, true).clicked()
        {
            out.reveal = Some(file.clone());
            ui.close();
        }
        if row.unused() && widgets::menu_item(ui, "Remove from project", None, true).clicked() {
            out.remove = Some(row.id);
            ui.close();
        }
    });
}

fn state_word(row: &Row) -> String {
    if row.missing {
        "missing".to_owned()
    } else if row.unused() {
        "unused".to_owned()
    } else {
        format!("used by {} clips", row.uses)
    }
}

/// The second line: where it lives and what it is. A missing file says where
/// the project looked.
fn detail_line(row: &Row, project_rate: u32) -> String {
    if row.missing {
        return format!("Was {}", row.path);
    }
    let seconds = row.frames as f64 / f64::from(row.sample_rate.max(1));
    let channels = match row.channels {
        1 => "mono".to_owned(),
        2 => "stereo".to_owned(),
        n => format!("{n} channels"),
    };
    let mut line = format!(
        "{} \u{00B7} {}:{:02} \u{00B7} {} \u{00B7} {}",
        row.home.label(),
        (seconds / 60.0) as u64,
        (seconds % 60.0) as u64,
        channels,
        crate::files::describe_size(row.size_bytes),
    );
    // The rate is said when it is not the project's: that is the file the
    // engine converts.
    if row.sample_rate != project_rate {
        let khz = f64::from(row.sample_rate) / 1000.0;
        line.push_str(&format!(" \u{00B7} {khz} kHz"));
    }
    line
}

/// A line of `text` cut with "…" to fit `width`.
fn fit(
    painter: &Painter,
    text: &str,
    font: FontId,
    colour: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, colour));
    job.wrap = TextWrapping::truncate_at_width(width.max(1.0));
    painter.layout_job(job)
}

/// A card on the pane's ground, the width of the pane.
pub(crate) fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::p().raised)
        .corner_radius(theme::radius::MEDIUM)
        .inner_margin(egui::Margin::same(theme::space::S as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(theme::space::XS, theme::space::XS);
            add(ui);
        });
    ui.add_space(theme::space::XS);
}

/// A sentence that wraps within the card.
pub(crate) fn wrapped(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme::size::CAPTION)
                .color(theme::p().text),
        )
        .wrap(),
    );
}

/// A quieter line under the sentence.
pub(crate) fn detail(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme::size::CAPTION)
                .color(theme::p().text_hint),
        )
        .wrap(),
    );
}

/// A thin bar: a sunk trough and the accent for how far.
pub(crate) fn bar(ui: &mut Ui, fraction: f32) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), theme::space::S), Sense::hover());
    widgets::raised::paint_field(ui.painter(), rect, theme::radius::SMALL, 0.0);
    let filled = Rect::from_min_size(
        rect.min,
        vec2(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
    );
    ui.painter()
        .rect_filled(filled, theme::radius::SMALL, theme::p().accent);
}

pub(crate) fn count_files(n: usize) -> String {
    if n == 1 {
        "1 file".to_owned()
    } else {
        format!("{n} files")
    }
}

/// The folder a found file is in, for the report.
pub(crate) fn parent_of(path: &std::path::Path) -> String {
    path.parent()
        .map_or_else(|| path.display().to_string(), |p| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_list_says_which_kind_of_nothing_it_is() {
        assert_eq!(
            empty_words(true, Filter::All, true).0,
            "No audio in this project"
        );
        // No words typed: the filter itself is what has nothing to show.
        assert_eq!(
            empty_words(false, Filter::Missing, true).0,
            "No file is missing"
        );
        assert_eq!(
            empty_words(false, Filter::Unused, true).0,
            "Every file is used"
        );
        // Words typed over a filter: they are what the person can clear.
        assert_eq!(
            empty_words(false, Filter::Unused, false).0,
            "No files match"
        );
    }
}
