//! One row of the file list (M9 R7c): a sound with its play button,
//! picture (the owner's R B) and length, a project or a folder; its menu;
//! and the row picture's painting, which the preview strip shares.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Rect, Sense, Ui, pos2, vec2};

use super::list::{Dragged, Out};
use super::saved;
use super::thumbs::Picture;
use super::walk::{Entry, Kind};
use crate::icons::{self, Icon};
use crate::{App, fonts, motion, theme, widgets};

/// The picture on a sound's row.
pub(super) const PICTURE: egui::Vec2 = egui::vec2(46.0, 16.0);
/// The play button on a sound's row, and an icon's box.
pub(super) const PLAY: f32 = 18.0;
pub(crate) const ICON: f32 = 14.0;

impl App {
    /// One row: a sound (play, name, picture, length), a project, or a
    /// folder.
    pub(crate) fn file_row(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        entry: &Entry,
        look: &RowLook,
        out: &mut Out,
    ) {
        let ctx = ui.ctx().clone();
        let palette = *theme::p();
        let id = row_id(&entry.path);
        let response = ui.interact(rect, id, Sense::click_and_drag());
        let thumb = self.browser.files.thumbs.get(&entry.path).cloned();
        let bad = matches!(thumb, Some(Err(_)));
        let said = match (&thumb, entry.kind) {
            (Some(Ok(t)), Kind::Audio) => {
                format!("{}, {}, {}", entry.name, entry.kind.said(), t.said())
            }
            (Some(Err(_)), _) => format!("{}, {}, cannot be read", entry.name, entry.kind.said()),
            _ if !look.there => format!("{}, place, not there", entry.name),
            _ => format!("{}, {}", entry.name, entry.kind.said()),
        };
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                look.selected,
                &said,
            )
        });
        // A sound or a project can be dragged onto the arrangement.
        if matches!(entry.kind, Kind::Audio | Kind::Midi) && !bad {
            response.dnd_set_drag_payload(Dragged(vec![entry.path.clone()]));
            if response.dragged() {
                crate::chain::paint_drag_ghost(&ctx, &entry.file_name());
            }
        }
        let playing = self
            .browser
            .files
            .preview
            .as_ref()
            .is_some_and(|p| p.is_playing(&entry.path));
        // The play button, before the name.
        let play_rect = Rect::from_center_size(
            pos2(rect.left() + theme::space::S + PLAY / 2.0, rect.center().y),
            vec2(PLAY, PLAY),
        );
        let play = (entry.kind == Kind::Audio && !bad).then(|| {
            let r = ui.interact(play_rect, id.with("play"), Sense::click());
            r.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    if playing { "Stop preview" } else { "Preview" },
                )
            });
            r
        });
        if let Some(play) = &play
            && play.clicked()
        {
            out.preview = Some(entry.path.clone());
        } else if response.double_clicked() {
            out.open = Some(entry.clone());
        } else if response.clicked() {
            out.select = Some(entry.path.clone());
        }
        widgets::Menu::context(&response).show(|ui| self.row_menu(ui, entry, look, out));

        let painter = ui.painter_at(rect);
        let hover = motion::hover(&ctx, id.with("hover"), response.hovered());
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
                let hovered = play.as_ref().is_some_and(egui::Response::hovered);
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
                if play.as_ref().is_some_and(|p| p.has_focus()) {
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
                    Kind::Folder if entry.path == Path::new(saved::COMPUTER) => Icon::Rack,
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
        let detail = match (&thumb, entry.kind) {
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
            match thumb
                .as_ref()
                .and_then(|t| t.as_ref().ok())
                .map(|t| &t.picture)
            {
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
        if response.has_focus() {
            widgets::raised::paint_focus(&painter, rect, theme::radius::MEDIUM);
        }
        if response.hovered() && !look.there {
            response.on_hover_text("This place is not there: an unplugged drive, or a folder that moved. Right-click to find it or remove it.");
        }
    }

    /// A row's menu.
    fn row_menu(&mut self, ui: &mut Ui, entry: &Entry, look: &RowLook, out: &mut Out) {
        let computer = entry.path == Path::new(saved::COMPUTER);
        if look.place {
            if !look.there && widgets::menu_item(ui, "Find it\u{2026}", None, true).clicked() {
                out.find_place = Some(entry.path.clone());
                ui.close();
            }
            if let Some(place) = self
                .settings
                .browser
                .places
                .iter_mut()
                .find(|p| Path::new(&p.path) == entry.path)
            {
                widgets::menu_caption(ui, "Name");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut place.name)
                        .desired_width(160.0)
                        .hint_text("Rename"),
                );
                if field.changed() {
                    self.settings_changed = Some(std::time::Instant::now());
                }
            }
        }
        if !computer
            && look.there
            && widgets::menu_item(ui, "Reveal in the file manager", None, true).clicked()
        {
            out.reveal = Some(entry.path.clone());
            ui.close();
        }
        if entry.kind == Kind::Folder
            && !look.place
            && !computer
            && widgets::menu_item(ui, "Add as a place", None, true).clicked()
        {
            out.add_place = Some(entry.path.clone());
            ui.close();
        }
        if look.place && widgets::menu_item(ui, "Remove", None, true).clicked() {
            out.remove_place = Some(entry.path.clone());
            ui.close();
        }
        if entry.kind == Kind::Audio && widgets::menu_item(ui, "Preview", None, true).clicked() {
            out.preview = Some(entry.path.clone());
            ui.close();
        }
        if matches!(entry.kind, Kind::Audio | Kind::Midi)
            && widgets::menu_item(ui, "Add to the arrangement", None, true).clicked()
        {
            out.open = Some(entry.clone());
            ui.close();
        }
        if entry.kind == Kind::Project && widgets::menu_item(ui, "Open", None, true).clicked() {
            out.open = Some(entry.clone());
            ui.close();
        }
    }
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
}
