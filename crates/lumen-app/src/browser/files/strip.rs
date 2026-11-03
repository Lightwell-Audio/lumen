//! The preview strip under the file list (M9 R7c): the chosen file, its
//! picture with where the preview is (click to play from there), Auto, the
//! level, and the pair it is heard on (the owner's O A).

use std::path::{Path, PathBuf};

use eframe::egui::{self, Rect, Sense, Ui, pos2, vec2};

use super::row::paint_picture;
use super::thumbs::Picture;
use super::walk::Kind;
use crate::browser::Tab;
use crate::{App, fonts, theme, widgets};

/// A line of the strip, and the picture's well.
const LINE: f32 = 24.0;
const WELL: f32 = 40.0;
/// The ids of the strip's controls that use the arrow keys, which the
/// list leaves to them while they have the keyboard.
pub(super) const WELL_ID: &str = "browser preview position";
pub(super) const LEVEL_ID: &str = "browser preview level";

/// The level slider's width at most and at least, and Auto's room.
const LEVEL: f32 = 104.0;
const LEVEL_LEAST: f32 = 48.0;
const AUTO: f32 = 76.0;

impl App {
    /// The file the strip is about: the open tab's chosen file, else the
    /// one playing.
    fn strip_subject(&self, tab: Tab) -> Option<(PathBuf, Kind)> {
        let chosen = self
            .browser
            .files
            .selected
            .get(&tab)
            .filter(|p| Kind::of_file(p).is_some_and(|k| k != Kind::Folder))
            .cloned();
        let path = chosen.or_else(|| {
            let preview = self.browser.files.preview.as_ref()?;
            preview.playing.as_ref().map(|p| p.path.clone())
        })?;
        let kind = Kind::of_file(&path)?;
        Some((path, kind))
    }

    /// What the strip says under its title, and whether it is a warning:
    /// a sound that cannot play, a MIDI file, a project's details.
    fn strip_message(&self, path: &Path, kind: Kind) -> Option<(String, bool)> {
        match kind {
            Kind::Audio => self
                .browser
                .files
                .preview
                .as_ref()
                .and_then(|p| p.failed.as_ref())
                .filter(|(p, _)| p == path)
                .map(|(_, why)| (format!("This file cannot be played: {why}"), true)),
            Kind::Midi => Some((
                "MIDI files are not previewed. Drag it onto a track, or press Enter to add it."
                    .to_owned(),
                false,
            )),
            Kind::Project => Some(match self.browser.files.abouts.get(path) {
                Some(Ok(about)) => (
                    format!(
                        "{} BPM \u{b7} {} \u{b7} {} {}",
                        trim_bpm(about.bpm),
                        about.meter,
                        about.tracks,
                        if about.tracks == 1 { "track" } else { "tracks" }
                    ),
                    false,
                ),
                Some(Err(error)) => (format!("This project cannot be read: {error}"), true),
                None => ("Reading the project\u{2026}".to_owned(), false),
            }),
            Kind::Folder => None,
        }
    }

    /// How tall the strip is in `tab` when it is `width` wide (0: there is
    /// none). Its message is measured, so a long one is read in full.
    pub(super) fn strip_height(&self, ui: &Ui, tab: Tab, width: f32) -> f32 {
        let Some((path, kind)) = self.strip_subject(tab) else {
            return 0.0;
        };
        let inner = width - 2.0 * (theme::space::M - theme::space::XS);
        let message = self.strip_message(&path, kind);
        let text = |width: f32| {
            message
                .as_ref()
                .map_or(0.0, |(text, _)| message_height(ui, text, width))
        };
        let edges = theme::space::S * 2.0;
        // The layout puts its item spacing after each row but the last.
        let gap = ui.spacing().item_spacing.y;
        match kind {
            Kind::Audio => {
                edges
                    + LINE * 2.0
                    + well_height(text(inner - 2.0 * theme::space::S))
                    + theme::space::XS * 2.0
                    + gap * 2.0
            }
            Kind::Project => edges + LINE * 2.0 + text(inner) + theme::space::XS + gap * 2.0,
            Kind::Midi => edges + LINE + text(inner) + gap,
            Kind::Folder => 0.0,
        }
    }

    /// Draws the strip in `rect`.
    pub(super) fn files_strip(&mut self, ui: &mut Ui, rect: Rect, tab: Tab) {
        let Some((path, kind)) = self.strip_subject(tab) else {
            return;
        };
        let palette = *theme::p();
        ui.painter().line_segment(
            [rect.left_top(), rect.right_top()],
            egui::Stroke::new(theme::stroke::HAIRLINE, palette.border),
        );
        let inner = rect.shrink2(vec2(theme::space::M - theme::space::XS, theme::space::S));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        let ui = &mut child;
        self.strip_title(ui, &path, kind);
        match kind {
            Kind::Audio => {
                ui.add_space(theme::space::XS);
                self.strip_well(ui, &path);
                ui.add_space(theme::space::XS);
                self.strip_controls(ui);
            }
            Kind::Midi => {
                if let Some((note, bad)) = self.strip_message(&path, kind) {
                    strip_note(ui, &note, bad);
                }
            }
            Kind::Project => {
                if let Some((note, bad)) = self.strip_message(&path, kind) {
                    strip_note(ui, &note, bad);
                }
                ui.add_space(theme::space::XS);
                let open = widgets::raised::button(ui, "Open");
                let open = widgets::tip(
                    open,
                    "Open the project",
                    None,
                    Some("Asks first when this one has unsaved changes"),
                );
                if open.clicked() {
                    let ctx = ui.ctx().clone();
                    self.open_row(&super::walk::Entry::new(path.clone(), Kind::Project), &ctx);
                }
            }
            Kind::Folder => {}
        }
    }

    /// The first line: play or stop, the name, what the file is.
    fn strip_title(&mut self, ui: &mut Ui, path: &Path, kind: Kind) {
        let palette = *theme::p();
        let playing = self
            .browser
            .files
            .preview
            .as_ref()
            .is_some_and(|p| p.is_playing(path));
        let thumb = self.browser.files.thumbs.get(path).cloned();
        let info = match &thumb {
            Some(Ok(t)) if kind != Kind::Project => t.summary(),
            _ => String::new(),
        };
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE), Sense::hover());
        let mut left = rect.left();
        if kind == Kind::Audio {
            let at = Rect::from_min_size(rect.min, vec2(LINE, LINE));
            let mut button = ui.new_child(egui::UiBuilder::new().max_rect(at));
            let label = if playing { "Stop preview" } else { "Preview" };
            let response = widgets::raised::icon_button(&mut button, label, move |p, c, ink| {
                if playing {
                    p.rect_filled(Rect::from_center_size(c, vec2(7.0, 7.0)), 1.0, ink);
                } else {
                    p.add(egui::Shape::convex_polygon(
                        vec![
                            c + vec2(-2.5, -4.0),
                            c + vec2(4.0, 0.0),
                            c + vec2(-2.5, 4.0),
                        ],
                        ink,
                        egui::Stroke::NONE,
                    ));
                }
            });
            let keys = self.keymap.keys(if playing {
                crate::commands::Command::StopPreview
            } else {
                crate::commands::Command::PreviewFile
            });
            if widgets::tip(response, label, keys.as_ref(), None).clicked() {
                if playing {
                    self.stop_preview();
                } else {
                    self.preview_file(path, 0.0);
                }
            }
            left += LINE + theme::space::S;
        }
        let painter = ui.painter_at(rect);
        let mut right = rect.right();
        if !info.is_empty() {
            // The name comes first: what the file is takes at most a third.
            let mut job = egui::text::LayoutJob::simple_singleline(
                info,
                egui::FontId::proportional(theme::size::CAPTION),
                palette.text_secondary,
            );
            job.wrap.max_width = ((right - left) / 3.0).max(1.0);
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            let galley = painter.layout_job(job);
            right -= galley.size().x;
            painter.galley(
                pos2(right, rect.center().y - galley.size().y / 2.0),
                galley,
                palette.text_secondary,
            );
            right -= theme::space::S;
        }
        let mut job = egui::text::LayoutJob::simple_singleline(
            name,
            fonts::medium(theme::size::BODY),
            palette.text,
        );
        job.wrap.max_width = (right - left).max(1.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let galley = painter.layout_job(job);
        painter.galley(
            pos2(left, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.text,
        );
    }

    /// The picture, with the head where the preview is; a click plays
    /// from there. A file that cannot play says why, in full.
    fn strip_well(&mut self, ui: &mut Ui, path: &Path) {
        let palette = *theme::p();
        let failed = self.strip_message(path, Kind::Audio).map(|(text, _)| text);
        let thumb = self
            .browser
            .files
            .thumbs
            .get(path)
            .and_then(|t| t.as_ref().ok())
            .cloned();
        // A message that does not fit the well makes it taller.
        let width = ui.available_width();
        let message = failed.as_ref().map_or(0.0, |text| {
            message_height(ui, text, width - 2.0 * theme::space::S)
        });
        let (rect, _) = ui.allocate_exact_size(vec2(width, well_height(message)), Sense::hover());
        let response = ui.interact(rect, WELL_ID.into(), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Preview position")
        });
        widgets::raised::paint_sunken(ui.painter(), rect, theme::radius::MEDIUM);
        let painter = ui.painter_at(rect);
        let inside = rect.shrink2(vec2(theme::space::S, theme::space::XS));
        if let Some(text) = failed {
            let mut job = egui::text::LayoutJob::simple(
                text,
                egui::FontId::proportional(theme::size::CAPTION),
                palette.warning,
                inside.width(),
            );
            job.halign = egui::Align::LEFT;
            let galley = painter.layout_job(job);
            painter.galley(inside.left_top(), galley, palette.warning);
            return;
        }
        let picture = thumb.as_ref().map_or(&Picture::Flat, |t| &t.picture);
        paint_picture(&painter, inside, picture, palette.text_secondary);
        let seconds = thumb.as_ref().and_then(|t| t.seconds).filter(|s| *s > 0.0);
        let position = self
            .browser
            .files
            .preview
            .as_ref()
            .filter(|p| p.is_playing(path))
            .and_then(|p| p.position);
        if let (Some(seconds), Some(at)) = (seconds, position) {
            let x = inside.left() + (at / seconds).clamp(0.0, 1.0) as f32 * inside.width();
            painter.line_segment(
                [pos2(x, rect.top() + 2.0), pos2(x, rect.bottom() - 2.0)],
                egui::Stroke::new(theme::stroke::LIT, palette.accent),
            );
        }
        if let Some(seconds) = seconds {
            let hover = response.hover_pos().map(|p| p.x);
            if let Some(x) = hover {
                painter.line_segment(
                    [pos2(x, rect.top() + 2.0), pos2(x, rect.bottom() - 2.0)],
                    egui::Stroke::new(theme::stroke::HAIRLINE, palette.text_hint),
                );
            }
            let response = response.on_hover_text(
                "Click to play from here; with the keyboard, Left and Right move by a tenth, Home goes to the start",
            );
            if response.clicked()
                && let Some(at) = response.interact_pointer_pos()
            {
                let share = f64::from(((at.x - inside.left()) / inside.width()).clamp(0.0, 1.0));
                self.preview_file(path, share * seconds);
            }
            if response.has_focus() {
                widgets::raised::paint_focus(ui.painter(), rect, theme::radius::MEDIUM);
                let key = |k| ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, k));
                let now = position.unwrap_or(0.0);
                let step = seconds / 10.0;
                let to = if key(egui::Key::ArrowRight) {
                    Some((now + step).min(seconds))
                } else if key(egui::Key::ArrowLeft) {
                    Some((now - step).max(0.0))
                } else if key(egui::Key::Home) {
                    Some(0.0)
                } else {
                    None
                };
                if let Some(to) = to {
                    self.preview_file(path, to);
                }
            }
        }
    }

    /// Auto, the level and the pair the preview is heard on.
    fn strip_controls(&mut self, ui: &mut Ui) {
        let palette = *theme::p();
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE), Sense::hover());
        // Auto, at the left.
        let auto_rect = Rect::from_min_size(rect.min, vec2(AUTO.min(rect.width()), LINE));
        let mut auto_ui = ui.new_child(egui::UiBuilder::new().max_rect(auto_rect));
        let mut auto = self.settings.browser.auto_preview;
        let response = widgets::switch(&mut auto_ui, &mut auto, "Auto", true);
        widgets::tip(
            response,
            "Auto preview",
            None,
            Some("Plays a sound when it is chosen with a click or the arrows"),
        );
        if auto != self.settings.browser.auto_preview {
            self.settings.browser.auto_preview = auto;
            self.settings_changed = Some(std::time::Instant::now());
        }
        // The pair, at the right; the level before it.
        let cue = self.transport.audio.preview_channel() > 0;
        let pair = if cue { "Cue" } else { "Main" };
        let painter = ui.painter_at(rect);
        let galley = painter.layout_no_wrap(
            pair.to_owned(),
            egui::FontId::proportional(theme::size::CAPTION),
            palette.text_secondary,
        );
        let pair_rect = Rect::from_min_size(
            pos2(
                rect.right() - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley.size(),
        );
        painter.galley(pair_rect.min, galley, palette.text_secondary);
        let pair_response = ui.interact(pair_rect, ui.id().with("preview pair"), Sense::hover());
        pair_response.on_hover_text(if cue {
            "Previews play on the cue pair, so the song's output is not interrupted"
        } else {
            "Previews play on the main output. Choose a cue pair in Audio settings to hear them apart"
        });
        let room = pair_rect.left() - theme::space::S - auto_rect.right() - theme::space::S;
        let width = room.min(LEVEL);
        let level_rect = Rect::from_min_size(
            pos2(pair_rect.left() - theme::space::S - width, rect.top() + 2.0),
            vec2(width, LINE - 4.0),
        );
        if width >= LEVEL_LEAST {
            let db = self.settings.browser.preview_db;
            let new = widgets::value_slider(
                ui,
                level_rect,
                egui::Id::new(LEVEL_ID),
                db,
                widgets::SliderKind::Volume,
                "Preview level",
                theme::controls(false),
            );
            if (new - db).abs() > f32::EPSILON {
                self.settings.browser.preview_db = new;
                self.settings_changed = Some(std::time::Instant::now());
            }
        }
    }
}

/// How tall `text` is in the strip's caption type, wrapped at `width`.
fn message_height(ui: &Ui, text: &str, width: f32) -> f32 {
    ui.fonts_mut(|f| {
        f.layout(
            text.to_owned(),
            egui::FontId::proportional(theme::size::CAPTION),
            theme::p().text,
            width.max(1.0),
        )
    })
    .size()
    .y
}

/// The picture's well: its usual height, or taller for a message
/// `text` points tall.
fn well_height(text: f32) -> f32 {
    WELL.max(text + 2.0 * theme::space::XS)
}

/// A quiet line under the title, wrapped (a warning when `bad`).
fn strip_note(ui: &mut Ui, text: &str, bad: bool) {
    let palette = theme::p();
    let colour = if bad {
        palette.warning
    } else {
        palette.text_secondary
    };
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme::size::CAPTION)
                .color(colour),
        )
        .wrap()
        .selectable(false),
    );
}

/// "120" or "92.5".
fn trim_bpm(bpm: f64) -> String {
    let text = format!("{bpm:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempos_drop_their_zeros() {
        assert_eq!(trim_bpm(120.0), "120");
        assert_eq!(trim_bpm(92.5), "92.5");
    }
}
