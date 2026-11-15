use std::sync::Arc;

use eframe::egui::{self, FontId, Key, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use eframe::egui::{pos2, vec2};
use lumen_core::time::TempoMap;
use lumen_project::edit::Edge;
use lumen_project::model::{Clip, ClipContent, Color, Id, Project};

use crate::app::clip_controls::fade_label;
use crate::arrangement::{TITLE, color_from, color_of};
use crate::commands::Command;
use crate::widgets;
use crate::{App, lanes, theme};

pub(crate) const CARD_WIDTH: f32 = 272.0;
const LABEL_WIDTH: f32 = theme::size::LABEL_COLUMN_NARROW;
const ROW: f32 = theme::size::CONTROL_SMALL;
const SWATCH: f32 = 14.0;
const FADE_SHAPE: f32 = 76.0;
const RENAME_MIN_WIDTH: f32 = 120.0;

#[derive(Debug, Default)]
pub(crate) struct ClipProps {
    pinned: Option<Id>,
    name: Option<String>,
    focus_name: bool,
    size: Option<Vec2>,
    renaming: Option<(Id, String)>,
}

impl ClipProps {
    pub(crate) fn pinned(&self) -> Option<Id> {
        self.pinned
    }
}

fn find(project: &Project, id: Id) -> Option<(&Arc<Clip>, egui::Color32)> {
    project.tracks.iter().find_map(|t| {
        t.clips
            .iter()
            .find(|c| c.id == id)
            .map(|c| (c, color_of(t.color)))
    })
}

pub(crate) fn card_place(clip: Option<Rect>, canvas: Rect, screen: Rect, size: Vec2) -> Pos2 {
    let gap = theme::space::S;
    let Some(clip) = clip
        .map(|c| c.intersect(canvas))
        .filter(|c| c.is_positive())
    else {
        return pos2(canvas.center().x - size.x / 2.0, canvas.top() + gap);
    };
    let x = if clip.right() + gap + size.x <= canvas.right() {
        clip.right() + gap
    } else if clip.left() - gap - size.x >= canvas.left() {
        clip.left() - gap - size.x
    } else {
        canvas.right() - gap - size.x
    };
    let lowest = (screen.bottom() - gap - size.y).max(screen.top() + gap);
    pos2(
        x.max(canvas.left() + gap),
        clip.top().max(canvas.top() + gap).min(lowest),
    )
}

pub(crate) fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            vec2(LABEL_WIDTH, ROW),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(LABEL_WIDTH);
                ui.label(RichText::new(label).color(theme::p().text_secondary));
            },
        );
        add(ui);
    });
}

fn swatch(
    ui: &mut egui::Ui,
    fill: egui::Color32,
    on: bool,
    round: bool,
    name: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, name));
    let response = response.on_hover_text(name);
    let palette = theme::p();
    let radius = if round {
        (SWATCH / 2.0) as u8
    } else {
        theme::radius::SMALL
    };
    ui.painter().rect_filled(rect, radius, fill);
    let hot = response.hovered() || response.has_focus();
    if on || hot {
        let stroke = Stroke::new(
            theme::stroke::LIT,
            if on {
                palette.accent
            } else {
                palette.text_secondary
            },
        );
        ui.painter().rect_stroke(
            rect.expand(2.0),
            radius.saturating_add(2),
            stroke,
            egui::StrokeKind::Inside,
        );
    }
    response
}

fn pill(ui: &mut egui::Ui, colour: egui::Color32, label: &str, on: bool) -> egui::Response {
    let palette = theme::p();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme::size::CAPTION),
        palette.text,
    );
    let dot = SWATCH - 2.0;
    let size = vec2(
        theme::space::XS + dot + theme::space::XS + galley.size().x + theme::space::S,
        theme::size::CONTROL_SMALL,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, label));
    let hover = crate::motion::hover(ui.ctx(), response.id.with("hover"), response.hovered());
    let controls = theme::controls(false);
    let painter = ui.painter();
    let round = theme::radius::FULL;
    painter.rect_filled(
        rect,
        round,
        controls.fill.lerp_to_gamma(controls.hover, hover),
    );
    let centre = pos2(rect.left() + theme::space::XS + dot / 2.0, rect.center().y);
    painter.circle_filled(centre, dot / 2.0, colour);
    painter.galley(
        pos2(
            centre.x + dot / 2.0 + theme::space::XS,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        palette.text,
    );
    if on || response.has_focus() {
        painter.rect_stroke(
            rect,
            round,
            Stroke::new(theme::stroke::LIT, palette.accent),
            egui::StrokeKind::Inside,
        );
    }
    response
}

#[derive(Clone, Copy)]
pub(crate) struct Swatches<'a> {
    pub(crate) name: &'a str,
    pub(crate) round: bool,
    pub(crate) none: (egui::Color32, &'a str),
}

fn clip_swatches(track: egui::Color32) -> Swatches<'static> {
    Swatches {
        name: "Color",
        round: false,
        none: (track, "Track color"),
    }
}

pub(crate) fn swatches(
    ui: &mut egui::Ui,
    current: Option<Color>,
    look: Swatches<'_>,
) -> Option<Option<Color>> {
    let mut chosen = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(theme::space::S, theme::space::S);
        ui.spacing_mut().interact_size.y = SWATCH;
        for row in theme::CLIP_COLORS.chunks(6).enumerate() {
            ui.horizontal(|ui| {
                for (j, &fill) in row.1.iter().enumerate() {
                    let color = color_from(fill);
                    let name = format!("{} {}", look.name, row.0 * 6 + j + 1);
                    if swatch(ui, fill, current == Some(color), look.round, &name).clicked() {
                        chosen = Some(Some(color));
                    }
                }
            });
        }
        let (dot, label) = look.none;
        if pill(ui, dot, label, current.is_none()).clicked() {
            chosen = Some(None);
        }
    });
    chosen
}

impl App {
    pub(crate) fn first_selected_clip(&self) -> Option<Id> {
        self.document
            .project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .find(|c| self.arrangement.clips.contains(&c.id))
            .map(|c| c.id)
    }

    pub(crate) fn toggle_clip_mute(&mut self) {
        let ids: Vec<Id> = self.arrangement.clips.iter().copied().collect();
        let project = self.document.project();
        let all = ids
            .iter()
            .all(|&id| find(project, id).is_some_and(|(c, _)| c.muted));
        self.document.set_clips_muted(&ids, !all);
    }

    pub(crate) fn toggle_clip_lock(&mut self) {
        let ids: Vec<Id> = self.arrangement.clips.iter().copied().collect();
        let project = self.document.project();
        let all = ids
            .iter()
            .all(|&id| find(project, id).is_some_and(|(c, _)| c.locked));
        self.document.set_clips_locked(&ids, !all);
    }

    pub(crate) fn start_clip_rename(&mut self, id: Id) {
        if let Some((clip, _)) = find(self.document.project(), id) {
            self.clip_props.renaming = Some((id, clip.name.clone()));
        }
    }

    pub(crate) fn clip_props_focus_name(&mut self) {
        self.clip_props.name = None;
        self.clip_props.focus_name = true;
    }

    pub(crate) fn apply_pending_clip_name(&mut self, clip: &Clip, cancelled: bool) {
        if let Some(name) = self.clip_props.name.take()
            && !cancelled
            && name != clip.name
        {
            self.document.rename_clip(clip.id, &name);
        }
    }

    pub(crate) fn open_clip_card(&mut self, id: Id) {
        let props = &mut self.clip_props;
        props.pinned = Some(id);
        props.name = None;
        props.focus_name = true;
        props.renaming = None;
    }

    pub(crate) fn clip_menu(&mut self, ui: &mut egui::Ui) {
        let Some(first) = self.first_selected_clip() else {
            return;
        };
        let ids: Vec<Id> = self.arrangement.clips.iter().copied().collect();
        let project = self.document.project();
        let Some((clip, track)) = find(project, first) else {
            return;
        };
        let current = clip.color;
        let several = ids.len() > 1;
        let all = |test: fn(&Clip) -> bool| {
            ids.iter()
                .all(|&id| find(project, id).is_some_and(|(c, _)| test(c)))
        };
        let (muted, locked, crossfade) =
            (all(|c| c.muted), all(|c| c.locked), all(|c| c.crossfade));
        let name = |verb: &str| {
            if several {
                format!("{verb} clips")
            } else {
                verb.to_owned()
            }
        };
        let (mute, lock) = (
            name(if muted { "Unmute" } else { "Mute" }),
            name(if locked { "Unlock" } else { "Lock" }),
        );
        self.context_command(ui, Command::Rename, "Rename");
        widgets::submenu(ui, "Color", |ui| {
            if let Some(color) = swatches(ui, current, clip_swatches(track)) {
                self.document.recolor_clips(&ids, color);
                ui.close();
            }
        });
        ui.separator();
        self.context_command(ui, Command::MuteClips, &mute);
        self.context_command(ui, Command::LockClips, &lock);
        let audio = self.selection_has_audio();
        if audio {
            ui.separator();
        }
        self.fade_menu(ui);
        if audio {
            let keys = self.keymap.keys(Command::ToggleCrossfade);
            if widgets::menu_toggle(ui, "Crossfade with previous", keys.as_ref(), crossfade)
                .on_hover_text("Blend this clip into the one before it where they overlap")
                .clicked()
            {
                self.run(ui.ctx(), Command::ToggleCrossfade);
            }
        }
        if let Some((track, clip)) = self.selected_audio_clip() {
            let label = if self.envelope_shown(track, clip) {
                "Hide envelope"
            } else {
                "Show envelope"
            };
            self.context_command(ui, Command::ClipEnvelope, label);
        }
        ui.separator();
        if self.can_convert_drum_map() {
            self.context_command(ui, Command::ConvertDrumMap, "Convert drum map\u{2026}");
            ui.separator();
        }
        let export = if several {
            "Export MIDI clips\u{2026}"
        } else {
            "Export MIDI clip\u{2026}"
        };
        self.context_command(ui, Command::ExportMidiClip, export);
        self.context_command(ui, Command::ClipProperties, "Properties\u{2026}");
    }

    fn clip_on_screen(&self, ctx: &egui::Context, canvas: Rect, id: Id) -> Option<Rect> {
        self.arrangement
            .clip_rects(
                ctx,
                canvas,
                self.document.project(),
                self.lanes.pending().len(),
                self.nav.view,
            )
            .into_iter()
            .find(|(_, clip, _)| *clip == id)
            .map(|(_, _, rect)| rect)
    }

    pub(crate) fn clip_rename(&mut self, ui: &mut egui::Ui, canvas: Rect) {
        let Some(id) = self.clip_props.renaming.as_ref().map(|(id, _)| *id) else {
            return;
        };
        let Some(rect) = self.clip_on_screen(ui.ctx(), canvas, id) else {
            self.clip_props.renaming = None;
            if find(self.document.project(), id).is_some() {
                self.open_clip_card(id);
            }
            return;
        };
        let shown = rect.intersect(canvas);
        let field = Rect::from_min_size(
            shown.min,
            vec2(shown.width().max(RENAME_MIN_WIDTH), TITLE + 2.0),
        );
        let Some((_, text)) = &mut self.clip_props.renaming else {
            return;
        };
        let cancel = ui.input(|i| i.key_pressed(Key::Escape));
        let response = ui.put(
            field,
            egui::TextEdit::singleline(text)
                .id(egui::Id::new(("clip rename", id)))
                .font(FontId::proportional(theme::size::CAPTION))
                .margin(vec2(4.0, 1.0)),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Clip name")
        });
        if !response.has_focus() && !response.lost_focus() {
            response.request_focus();
        }
        if cancel {
            self.clip_props.renaming = None;
        } else if response.lost_focus() {
            let name = text.clone();
            self.clip_props.renaming = None;
            self.document.rename_clip(id, &name);
        }
    }

    pub(crate) fn clip_card(&mut self, ctx: &egui::Context, canvas: Rect) {
        let Some(mut id) = self.clip_props.pinned else {
            return;
        };
        if self.arrangement.clips.is_empty() {
            self.clip_props = ClipProps::default();
            return;
        }
        if self.arrangement.clips.len() == 1
            && let Some(&only) = self.arrangement.clips.first()
            && only != id
        {
            id = only;
            self.clip_props.pinned = Some(only);
            self.clip_props.name = None;
        }
        let project = self.document.project();
        let (Some((clip, track)), Ok(tempo)) =
            (find(project, id), TempoMap::new(project.tempo.clone()))
        else {
            self.clip_props = ClipProps::default();
            return;
        };
        let clip = Arc::clone(clip);
        let size = self
            .clip_props
            .size
            .unwrap_or(vec2(CARD_WIDTH + 16.0, 260.0));
        let at = card_place(
            self.clip_on_screen(ctx, canvas, id),
            canvas,
            ctx.content_rect(),
            size,
        );
        let palette = *theme::p();
        let mut close = false;
        let shown = egui::Area::new(egui::Id::new("clip card"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .corner_radius(theme::radius::XL)
                    .stroke(Stroke::new(theme::stroke::HAIRLINE, palette.accent))
                    .show(ui, |ui| {
                        ui.set_width(CARD_WIDTH);
                        close = self.card_contents(ui, &clip, track, &tempo);
                    });
            });
        self.clip_props.size = Some(shown.response.rect.size());
        widgets::window_lip(ctx, shown.response.layer_id, shown.response.rect);
        let typing = self.typing || ctx.text_edit_focused();
        if close || (!typing && ctx.input(|i| i.key_pressed(Key::Escape))) {
            self.clip_props = ClipProps::default();
        }
    }

    pub(crate) fn name_and_color_rows(
        &mut self,
        ui: &mut egui::Ui,
        clip: &Clip,
        track: egui::Color32,
        place: &'static str,
    ) {
        let id = clip.id;
        row(ui, "Name", |ui| {
            let props = &mut self.clip_props;
            let text = props.name.get_or_insert_with(|| clip.name.clone());
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .id(egui::Id::new(("clip card name", id, place)))
                    .desired_width(ui.available_width()),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Clip name")
            });
            if std::mem::take(&mut props.focus_name) {
                response.request_focus();
            }
            if response.lost_focus() {
                if !cancel && let Some(name) = props.name.take() {
                    self.document.rename_clip(id, &name);
                }
                self.clip_props.name = None;
            } else if !response.has_focus() {
                props.name = None;
            }
        });

        row(ui, "Color", |ui| {
            if let Some(color) = swatches(ui, clip.color, clip_swatches(track)) {
                self.document.recolor_clips(&[id], color);
            }
        });
    }

    fn card_contents(
        &mut self,
        ui: &mut egui::Ui,
        clip: &Clip,
        track: egui::Color32,
        tempo: &TempoMap,
    ) -> bool {
        let palette = *theme::p();
        let id = clip.id;
        let close = widgets::title_row(ui, "Clip", true);
        self.name_and_color_rows(ui, clip, track, "card");

        if let ClipContent::Audio { media, .. } = &clip.content {
            ui.add_enabled_ui(!clip.locked, |ui| {
                row(ui, "Gain", |ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                    self.clip_gain_slider(ui, rect, clip, egui::Id::new(("clip card gain", id)));
                });
                for edge in [Edge::Start, Edge::End] {
                    let label = fade_label(edge);
                    row(ui, label, |ui| {
                        let ids = (egui::Id::new(("clip card fade", id, label)), FADE_SHAPE);
                        self.clip_fade_row(ui, (clip, tempo), edge, ids);
                    });
                }
            });
            if clip.locked {
                ui.label(
                    RichText::new("Locked: unlock it to change gain and fades")
                        .small()
                        .color(palette.text_hint),
                );
            }
            row(ui, "State", |ui| self.card_toggles(ui, clip));
            ui.separator();
            match self.lanes.library.get(media.0) {
                Some(item) => lanes::file_facts(ui, item),
                None => {
                    ui.label(RichText::new("File missing").color(palette.warning));
                }
            }
        } else {
            row(ui, "State", |ui| self.card_toggles(ui, clip));
            if let ClipContent::Midi { notes, .. } = &clip.content {
                ui.separator();
                ui.label(
                    RichText::new(format!("MIDI clip · {} notes", notes.len()))
                        .small()
                        .weak(),
                );
            }
        }
        close
    }

    fn card_toggles(&mut self, ui: &mut egui::Ui, clip: &Clip) {
        let named = |response: egui::Response, name: &'static str, on: bool| {
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, name)
            });
            response
        };
        let mut muted = clip.muted;
        let hint = match self.keymap.text(Command::MuteClips) {
            Some(keys) => format!("Mute or unmute this clip ({keys})"),
            None => "Mute or unmute this clip".to_owned(),
        };
        let mute = widgets::toggle_button(ui, &mut muted, "Mute");
        if named(mute, "Mute clip", muted)
            .on_hover_text(hint)
            .changed()
        {
            self.document.set_clips_muted(&[clip.id], muted);
        }
        let mut locked = clip.locked;
        let lock = widgets::toggle_button(ui, &mut locked, "Lock");
        if named(lock, "Lock clip", locked)
            .on_hover_text("A locked clip cannot be moved, trimmed, faded or deleted")
            .changed()
        {
            self.document.set_clips_locked(&[clip.id], locked);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_card_sits_beside_its_clip_inside_the_lanes() {
        let canvas = Rect::from_min_size(pos2(200.0, 100.0), vec2(800.0, 400.0));
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 560.0));
        let size = vec2(280.0, 250.0);
        let clip = Rect::from_min_size(pos2(300.0, 150.0), vec2(100.0, 50.0));
        assert_eq!(
            card_place(Some(clip), canvas, screen, size),
            pos2(408.0, 150.0)
        );
        let clip = Rect::from_min_size(pos2(800.0, 150.0), vec2(150.0, 50.0));
        assert_eq!(
            card_place(Some(clip), canvas, screen, size),
            pos2(512.0, 150.0)
        );
        let clip = Rect::from_min_size(pos2(250.0, 420.0), vec2(700.0, 50.0));
        assert_eq!(
            card_place(Some(clip), canvas, screen, size),
            pos2(712.0, 302.0)
        );
        assert_eq!(card_place(None, canvas, screen, size), pos2(460.0, 108.0));
        let gone = Rect::from_min_size(pos2(0.0, 0.0), vec2(50.0, 50.0));
        assert_eq!(
            card_place(Some(gone), canvas, screen, size),
            pos2(460.0, 108.0)
        );
    }
}
