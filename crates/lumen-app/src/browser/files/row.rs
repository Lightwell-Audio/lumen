//! One row of the file list (M9 R7c): a sound with its play button,
//! picture (the owner's R B) and length, a project or a folder; its menu;
//! and the row picture's painting, which the preview strip shares. What the
//! pointer did with a row, and its menu, are `app::browser::files::row`
//! (audit M12).

use std::path::{Path, PathBuf};

use eframe::egui::{self, Rect, Ui, pos2, vec2};

use super::thumbs::{Picture, Thumb};
use super::walk::{Entry, Kind};
use crate::icons::{self, Icon};
use crate::{fonts, motion, theme, widgets};

/// The picture on a sound's row.
const PICTURE: egui::Vec2 = egui::vec2(46.0, 16.0);
/// The play button on a sound's row, and an icon's box.
const PLAY: f32 = 18.0;
pub(crate) const ICON: f32 = 14.0;

/// What a row is drawn from this frame: the picture read for it, whether it
/// is playing, and the two responses its states come from.
pub(crate) struct Drawn<'a> {
    pub thumb: Option<&'a Result<Thumb, String>>,
    pub playing: bool,
    pub response: &'a egui::Response,
    /// The play button, on a sound's row that can be heard.
    pub play: Option<&'a egui::Response>,
}

/// How a row is drawn.
pub(crate) struct RowLook {
    pub selected: bool,
    pub two: bool,
    pub pictures: bool,
    /// A place that is there (a missing one shows the warning shape).
    pub there: bool,
    pub place: bool,
    pub folder_line: String,
}

/// Paints one row: what leads it, the name, the folder under it, the length
/// and the picture.
pub(crate) fn paint_row(ui: &Ui, rect: Rect, entry: &Entry, look: &RowLook, drawn: &Drawn<'_>) {
    let palette = *theme::p();
    let id = row_id(&entry.path);
    let thumb = drawn.thumb;
    let play = drawn.play;
    let playing = drawn.playing;
    let bad = matches!(thumb, Some(Err(_)));
    let play_rect = play_rect(rect);
    let painter = ui.painter_at(rect);
    let hover = motion::hover(ui.ctx(), id.with("hover"), drawn.response.hovered());
    if look.selected {
        painter.rect_filled(rect, theme::radius::MEDIUM, palette.selected_row);
    }
    painter.rect_filled(
        rect,
        theme::radius::MEDIUM,
        palette.ember.gamma_multiply(hover),
    );
    let ink = if !look.there || bad {
        palette.text_secondary
    } else {
        palette.text
    };
    let dim = if look.selected {
        palette.text_secondary
    } else {
        palette.text_hint
    };
    // What leads the row.
    let lead = play_rect.center();
    match entry.kind {
        Kind::Audio if !bad => {
            let face = theme::lit(theme::Tone::Accent);
            let hovered = play.is_some_and(egui::Response::hovered);
            let fill = if playing {
                face.bottom
            } else if hovered {
                palette.control_hover
            } else {
                palette.control
            };
            painter.circle_filled(lead, PLAY / 2.0, fill);
            if !playing {
                painter.circle_stroke(
                    lead,
                    PLAY / 2.0,
                    egui::Stroke::new(theme::stroke::HAIRLINE, palette.border),
                );
            }
            let c = if playing {
                face.text
            } else {
                palette.text_secondary
            };
            if playing {
                painter.rect_filled(Rect::from_center_size(lead, vec2(6.0, 6.0)), 1.0, c);
            } else {
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        lead + vec2(-2.0, -3.5),
                        lead + vec2(3.5, 0.0),
                        lead + vec2(-2.0, 3.5),
                    ],
                    c,
                    egui::Stroke::NONE,
                ));
            }
            if play.is_some_and(|p| p.has_focus()) {
                widgets::raised::paint_focus(&painter, play_rect, (PLAY / 2.0) as u8);
            }
        }
        Kind::Midi => {
            let font = egui::FontId::proportional(theme::size::BODY);
            painter.text(lead, egui::Align2::CENTER_CENTER, "\u{266a}", font, dim);
        }
        _ => {
            let icon = match entry.kind {
                Kind::Project => Icon::Document,
                Kind::Folder if entry.path == Path::new(super::saved::COMPUTER) => Icon::Rack,
                Kind::Folder => Icon::Folder,
                _ => Icon::Warning,
            };
            let icon = if !look.there || bad {
                Icon::Warning
            } else {
                icon
            };
            let tint = if !look.there || bad {
                palette.warning
            } else {
                palette.text_secondary
            };
            let r = Rect::from_center_size(lead, vec2(ICON, ICON));
            painter.extend(icons::shapes(icon, r, tint));
        }
    }
    // The right side: the length, and the picture before it.
    let mut right = rect.right() - theme::space::S;
    let detail = match (thumb, entry.kind) {
        (Some(Ok(t)), Kind::Audio | Kind::Midi) => t.seconds.map(super::thumbs::length),
        (Some(Err(_)), _) => Some("Cannot be read".to_owned()),
        _ if !look.there => Some("Not there".to_owned()),
        _ => None,
    };
    if let Some(detail) = detail {
        let galley = painter.layout_no_wrap(
            detail,
            egui::FontId::proportional(theme::size::CAPTION),
            dim,
        );
        right -= galley.size().x;
        painter.galley(
            pos2(right, rect.center().y - galley.size().y / 2.0),
            galley,
            dim,
        );
        right -= theme::space::S;
    }
    if look.pictures && matches!(entry.kind, Kind::Audio | Kind::Midi) && !bad {
        let at = Rect::from_min_size(
            pos2(right - PICTURE.x, rect.center().y - PICTURE.y / 2.0),
            PICTURE,
        );
        let tint = if look.selected {
            palette.text
        } else {
            palette.text_secondary
        };
        match thumb.and_then(|t| t.as_ref().ok()).map(|t| &t.picture) {
            Some(picture) => paint_picture(&painter, at, picture, tint),
            None => paint_picture(
                &painter,
                at,
                &Picture::Flat,
                dim.gamma_multiply(theme::alpha::EDGE),
            ),
        }
        right = at.left() - theme::space::S;
    }
    // The name, and under it the folder.
    let left = play_rect.right() + theme::space::S;
    let width = (right - left).max(0.0);
    let name = fit(
        &painter,
        &entry.name,
        fonts::medium(theme::size::BODY),
        width,
        ink,
    );
    let mut y = rect.center().y - name.size().y / 2.0;
    let second = look
        .two
        .then_some(look.folder_line.as_str())
        .filter(|s| !s.is_empty());
    if second.is_some() {
        y = rect.top() + theme::space::XS + 2.0;
    }
    painter.galley(pos2(left, y), name, ink);
    if let Some(folder) = second {
        let line = fit(
            &painter,
            folder,
            egui::FontId::proportional(theme::size::CAPTION),
            width,
            dim,
        );
        painter.galley(
            pos2(left, rect.bottom() - line.size().y - theme::space::XS - 2.0),
            line,
            dim,
        );
    }
}

/// Where a row's play button sits.
pub(crate) fn play_rect(rect: Rect) -> Rect {
    Rect::from_center_size(
        pos2(rect.left() + theme::space::S + PLAY / 2.0, rect.center().y),
        vec2(PLAY, PLAY),
    )
}

/// A row's id: its path.
pub(crate) fn row_id(path: &Path) -> egui::Id {
    egui::Id::new(("file row", path))
}

/// The folder a row is in, from the sample folder or place it is under.
pub(crate) fn folder_line(entry: &Entry, roots: &[PathBuf]) -> String {
    let Some(parent) = entry.path.parent() else {
        return String::new();
    };
    roots
        .iter()
        .find_map(|r| {
            let rest = parent.strip_prefix(r).ok()?;
            let base = r.file_name().map(|n| n.to_string_lossy().into_owned())?;
            Some(
                std::iter::once(base)
                    .chain(rest.iter().map(|p| p.to_string_lossy().into_owned()))
                    .collect::<Vec<_>>()
                    .join(" \u{203a} "),
            )
        })
        .unwrap_or_else(|| parent.display().to_string())
}

pub(crate) fn heading(ui: &Ui, rect: Rect, text: &str) {
    let palette = theme::p();
    ui.painter().text(
        pos2(rect.left() + theme::space::S, rect.center().y + 2.0),
        egui::Align2::LEFT_CENTER,
        text,
        fonts::medium(theme::size::CAPTION),
        palette.text_secondary,
    );
}

/// `text` in one line of at most `width`, cut with "…" (a label may be).
fn fit(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    width: f32,
    colour: egui::Color32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap.max_width = width.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    painter.layout_job(job)
}

/// A row's picture in `rect`: the waveform mirrored about its middle, the
/// notes as bars, or a flat line.
pub(crate) fn paint_picture(
    painter: &egui::Painter,
    rect: Rect,
    picture: &Picture,
    tint: egui::Color32,
) {
    match picture {
        Picture::Wave(columns) if !columns.is_empty() => {
            let step = rect.width() / columns.len() as f32;
            let mid = rect.center().y;
            let half = rect.height() / 2.0;
            for (i, level) in columns.iter().enumerate() {
                let h = (f32::from(*level) / 255.0 * half).max(0.5);
                let x = rect.left() + i as f32 * step;
                painter.rect_filled(
                    Rect::from_min_max(pos2(x, mid - h), pos2(x + step.max(0.5), mid + h)),
                    0.0,
                    tint,
                );
            }
        }
        Picture::Notes(notes) => {
            let bar = 2.0;
            for (start, length, pitch) in notes {
                let x = rect.left() + start * rect.width();
                let w = (length * rect.width()).max(1.5);
                let y = rect.bottom() - bar - pitch * (rect.height() - bar);
                painter.rect_filled(
                    Rect::from_min_size(pos2(x, y), vec2(w.min(rect.right() - x), bar)),
                    0.5,
                    tint,
                );
            }
        }
        _ => {
            painter.line_segment(
                [
                    pos2(rect.left(), rect.center().y),
                    pos2(rect.right(), rect.center().y),
                ],
                egui::Stroke::new(theme::stroke::HAIRLINE, tint),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_says_the_folder_it_is_in_from_its_sample_folder() {
        let root = PathBuf::from("/x/Samples");
        let entry = Entry::new(root.join("Drums/Kicks/Kick.wav"), Kind::Audio);
        assert_eq!(
            folder_line(&entry, &[root]),
            "Samples \u{203a} Drums \u{203a} Kicks"
        );
    }

    #[test]
    fn the_play_button_sits_after_the_rows_inside_room() {
        let row = Rect::from_min_size(pos2(0.0, 10.0), vec2(300.0, 28.0));
        let play = play_rect(row);
        assert_eq!(play.center().y, row.center().y);
        assert_eq!(play.left(), theme::space::S);
        assert_eq!(play.width(), PLAY);
    }
}
