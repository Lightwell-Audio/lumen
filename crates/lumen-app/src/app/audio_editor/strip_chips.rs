//! The two chips of the audio editor's strip (M9.5 S3): the Clip chip at the
//! left, which opens Name and Color, and the File chip at the right, which
//! opens what is known about the clip's file (§10.5). Both are ordinary
//! controls: Tab stops with accessible names, opened by a click, Enter or
//! Space, closed by Esc.

use eframe::egui::{self, Color32, FontId, Key, Rect, RichText, Sense, Ui, pos2, vec2};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{Clip, Id};

use crate::app::clip_card::CARD_WIDTH;
use crate::app::clip_file::ClipFile;
use crate::audio_editor::AudioEditor;
use crate::icons::{self, Icon};
use crate::widgets::{self, Menu, raised};
use crate::{App, lanes, theme};

/// The Clip chip's colour dot.
pub(super) const DOT: f32 = 10.0;
/// The room of a chip's chevron.
pub(super) const CHEVRON: f32 = 9.0;
/// The File chip's icon.
const FILE_ICON: f32 = 13.0;
/// The most of the clip's name the chip shows.
pub(super) const CHIP_NAME: f32 = 120.0;

/// A chevron pointing down, centred on `c`.
pub(super) fn chevron(painter: &egui::Painter, c: egui::Pos2, ink: Color32) {
    let stroke = egui::Stroke::new(1.5, ink);
    painter.line_segment([c + vec2(-3.0, -1.5), c + vec2(0.0, 1.5)], stroke);
    painter.line_segment([c + vec2(0.0, 1.5), c + vec2(3.0, -1.5)], stroke);
}

/// The width of `text` in `font`.
pub(super) fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
        .size()
        .x
}

/// How wide the Clip chip is with the name, and as its dot alone.
pub(super) fn clip_chip_widths(ui: &Ui, name: &str) -> (f32, f32) {
    let name = text_width(ui, name, FontId::proportional(theme::size::BODY)).min(CHIP_NAME);
    let s = theme::space::S;
    (s + DOT + s + name + s + CHEVRON + s, theme::size::CONTROL)
}

/// How wide the File chip is when it can be: its icon and the file's name,
/// at most [`theme::size::STRIP_FILE`].
pub(super) fn file_chip_width(ui: &Ui, name: &str) -> f32 {
    let s = theme::space::S;
    let name = text_width(ui, name, FontId::proportional(theme::size::BODY));
    (s + FILE_ICON + s + name + s + CHEVRON + s).min(theme::size::STRIP_FILE)
}

impl App {
    /// The Clip chip: the clip's colour dot and name (the dot alone when
    /// `named` is false), opening Name and Color for the opened clip.
    pub(super) fn clip_chip(
        &mut self,
        row: &mut Ui,
        (clip, colour): (&Clip, Color32),
        named: bool,
        ed: &mut AudioEditor,
    ) {
        let palette = *theme::p();
        let (full, dot_only) = clip_chip_widths(row, &clip.name);
        let width = if named { full } else { dot_only };
        let (rect, response) =
            row.allocate_exact_size(vec2(width, theme::size::CONTROL), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Clip {}: name and color", clip.name),
            )
        });
        if row.is_rect_visible(rect) {
            let face = raised::face_of(row, &response);
            let rect = rect.round_to_pixels(row.pixels_per_point());
            let body = raised::paint(
                row.painter(),
                rect,
                theme::radius::for_height(rect.height()),
                &face,
            );
            let mid = body.center().y.round();
            let painter = row.painter();
            if named {
                let s = theme::space::S;
                let dot = pos2((body.left() + s + DOT / 2.0).round(), mid);
                painter.circle_filled(dot, DOT / 2.0, colour);
                let room = (body.width() - 3.0 * s - DOT - CHEVRON - s).max(0.0);
                let font = FontId::proportional(theme::size::BODY);
                let shown = widgets::fit(painter, &clip.name, font.clone(), room);
                let galley = painter.layout_no_wrap(shown, font, palette.text);
                let x = (dot.x + DOT / 2.0 + s).round();
                painter.galley(
                    pos2(x, mid - (galley.size().y / 2.0).round()),
                    galley,
                    palette.text,
                );
                chevron(
                    painter,
                    pos2((body.right() - s - CHEVRON / 2.0).round(), mid),
                    palette.text_hint,
                );
            } else {
                painter.circle_filled(body.center().round(), DOT / 2.0, colour);
            }
            raised::focus_ring(painter, &response, rect);
        }
        let response = widgets::tip(
            response,
            "Name and color",
            None,
            Some("Rename this clip or give it a color of its own"),
        );
        let ctx = row.ctx().clone();
        let was = Menu::is_open_for(&ctx, response.id);
        let track = colour;
        Menu::below(&response).staying().show(|ui| {
            ui.set_width(CARD_WIDTH);
            self.name_and_color_rows(ui, clip, track, "chip");
        });
        let open = Menu::is_open_for(&ctx, response.id);
        if open && !was {
            // The name is where the keyboard starts.
            self.clip_props_focus_name();
        }
        if ed.chip_was_open && !open {
            // Closed while a name was being typed: Esc takes it back, any
            // other way of leaving applies it, as leaving the field does.
            let cancelled = ctx.input(|i| i.key_pressed(Key::Escape));
            self.apply_pending_clip_name(clip, cancelled);
        }
        ed.chip_was_open = open;
    }

    /// The File chip in `rect`: the file's icon and name, "Missing" in the
    /// warning colour when it cannot be found. It opens the file's facts, the
    /// path, Reveal and, for a missing file, Relink.
    pub(super) fn file_chip(&mut self, ui: &mut Ui, rect: Rect, media: Id) {
        let palette = *theme::p();
        let state = self.clip_file_state(media);
        let name = self.clip_file_name(media);
        let missing = state == ClipFile::Missing;
        let response = ui.interact(
            rect,
            ui.id().with(("audio strip file", media.0)),
            Sense::click(),
        );
        response.widget_info(|| {
            let what = if missing { "File missing" } else { "File" };
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{what}: {name}"))
        });
        if ui.is_rect_visible(rect) {
            let face = raised::face_of(ui, &response);
            let rect = rect.round_to_pixels(ui.pixels_per_point());
            let body = raised::paint(
                ui.painter(),
                rect,
                theme::radius::for_height(rect.height()),
                &face,
            );
            let mid = body.center().y.round();
            let painter = ui.painter();
            let ink = if missing {
                palette.warning
            } else {
                face.text()
            };
            let s = theme::space::S;
            let icon_only = body.width() < theme::size::CONTROL + 1.0;
            let icon_at = if icon_only {
                body.center().round()
            } else {
                pos2((body.left() + s + FILE_ICON / 2.0).round(), mid)
            };
            let icon = if missing { Icon::Warning } else { Icon::Wave };
            painter.extend(icons::shapes(
                icon,
                Rect::from_center_size(icon_at, vec2(FILE_ICON, FILE_ICON)),
                ink,
            ));
            if !icon_only {
                let room =
                    (body.right() - s - CHEVRON - s - (icon_at.x + FILE_ICON / 2.0 + s)).max(0.0);
                let font = FontId::proportional(theme::size::BODY);
                let text = if missing { "Missing" } else { name.as_str() };
                let shown = widgets::fit(painter, text, font.clone(), room);
                let galley = painter.layout_no_wrap(shown, font, ink);
                painter.galley(
                    pos2(
                        (icon_at.x + FILE_ICON / 2.0 + s).round(),
                        mid - (galley.size().y / 2.0).round(),
                    ),
                    galley,
                    ink,
                );
                chevron(
                    painter,
                    pos2((body.right() - s - CHEVRON / 2.0).round(), mid),
                    palette.text_hint,
                );
            }
            raised::focus_ring(painter, &response, rect);
        }
        let response = widgets::tip(
            response,
            "The clip's file",
            None,
            Some("Its facts, where it is, and Show in file manager"),
        );
        Menu::below(&response).staying().show(|ui| {
            ui.set_width(CARD_WIDTH);
            self.file_popover(ui, media, &state, &name);
        });
    }

    /// What the File chip opens.
    fn file_popover(&mut self, ui: &mut Ui, media: Id, state: &ClipFile, name: &str) {
        let palette = *theme::p();
        ui.label(RichText::new(name).strong());
        match state {
            ClipFile::Missing => {
                ui.add(
                    egui::Label::new(
                        RichText::new("This file cannot be found").color(palette.warning),
                    )
                    .wrap(),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "It may have moved or been renamed. Relink points the project at the file that stands in for it.",
                        )
                        .small()
                        .color(palette.text_secondary),
                    )
                    .wrap(),
                );
            }
            ClipFile::Reading => {
                ui.label(RichText::new("Reading\u{2026}").color(palette.text_secondary));
            }
            ClipFile::Ready | ClipFile::Failed(_) => {
                if let Some(item) = self.lanes.library.get(media.0) {
                    lanes::file_facts(ui, item);
                }
            }
        }
        ui.add_space(theme::space::S);
        ui.horizontal(|ui| {
            let ctx = ui.ctx().clone();
            if *state == ClipFile::Missing
                && raised::primary_button(ui, "Relink\u{2026}")
                    .on_hover_text("Choose the file that stands in for this one")
                    .clicked()
            {
                self.relink_clip_file(&ctx, media);
                ui.close();
            }
            let there = state.is_there();
            let keys = self.keymap.keys(crate::commands::Command::RevealClipFile);
            let show = ui.add_enabled_ui(there, |ui| raised::button(ui, "Show in file manager"));
            let show = widgets::tip(
                show.inner,
                "Show the file in the file manager",
                keys.as_ref(),
                None,
            );
            if !there {
                show.clone()
                    .on_disabled_hover_text("The file is missing. Relink it first");
            }
            if show.clicked() && there {
                self.reveal_media(media);
                ui.close();
            }
        });
    }

    /// The words on the File chip: the file's name, or "Missing" when it
    /// cannot be found.
    pub(super) fn file_chip_text(&self, media: Id) -> String {
        if self.clip_file_state(media) == ClipFile::Missing {
            "Missing".to_owned()
        } else {
            self.clip_file_name(media)
        }
    }

    /// The name of the file of `media`, from the library, or from the
    /// project when the library does not hold it (a missing file).
    pub(super) fn clip_file_name(&self, media: Id) -> String {
        if let Some(item) = self.lanes.library.get(media.0) {
            return item.name.clone();
        }
        self.document
            .project()
            .media
            .iter()
            .find(|m| m.id == media)
            .map(|m| m.path.rsplit('/').next().unwrap_or(&m.path).to_owned())
            .unwrap_or_default()
    }
}
