//! Clip properties (M3 slice 6e): mute (0), lock, rename (Ctrl+R or F2, in
//! the clip's title bar), colour, and the properties card. The card is the
//! owner's P1: the hover card, pinned beside its clip and edited in place
//! (Alt+Enter, or Properties in the right-click menu). Tab walks its fields,
//! so fades and gain no longer need the mouse. Every change is one undo
//! step; a lock keeps a clip in place, so locked clips can still be muted,
//! renamed and recoloured.

use eframe::egui::{self, FontId, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use eframe::egui::{pos2, vec2};
use lumen_core::fade::FadeShape;
use lumen_core::time::{TempoMap, Ticks};
use lumen_project::edit::Edge;
use lumen_project::model::{Clip, ClipContent, Color, Id, Project};

use crate::arrangement::{TITLE, color_from, color_of};
use crate::widgets::{self, SliderKind};
use crate::{App, fonts, icons, lanes, theme};

/// The card's width: its content wraps or truncates inside it.
const CARD_WIDTH: f32 = 272.0;
/// The labels' column.
const LABEL_WIDTH: f32 = 62.0;
/// A slider's height.
const ROW: f32 = 18.0;
/// A colour swatch.
const SWATCH: f32 = 14.0;
/// The narrowest title-bar rename field.
const RENAME_MIN_WIDTH: f32 = 120.0;

/// What the clip properties need between frames (not saved).
#[derive(Debug, Default)]
pub(crate) struct ClipProps {
    /// The clip whose card is pinned open.
    pinned: Option<Id>,
    /// The name being typed in the card.
    name: Option<String>,
    /// Focus the card's name field when it next shows.
    focus_name: bool,
    /// The card's size last frame, to place it.
    size: Option<Vec2>,
    /// The clip being renamed in its title bar, with the text so far.
    renaming: Option<(Id, String)>,
}

impl ClipProps {
    /// The clip whose card is open, if any.
    pub(crate) fn pinned(&self) -> Option<Id> {
        self.pinned
    }
}

/// The clip `id`, and its track's colour.
fn find(project: &Project, id: Id) -> Option<(&Clip, egui::Color32)> {
    project.tracks.iter().find_map(|t| {
        t.clips
            .iter()
            .find(|c| c.id == id)
            .map(|c| (&**c, color_of(t.color)))
    })
}

/// How long the fade at `edge` of `clip` lasts, in milliseconds.
fn fade_ms(tempo: &TempoMap, clip: &Clip, edge: Edge) -> f32 {
    let end = clip.start + clip.length;
    let (a, b) = match edge {
        Edge::Start => (clip.start, clip.start + clip.fade_in.length),
        Edge::End => (end - clip.fade_out.length, end),
    };
    ((tempo.seconds_at(b) - tempo.seconds_at(a)) * 1000.0) as f32
}

/// How long `clip` lasts, in milliseconds.
fn clip_ms(tempo: &TempoMap, clip: &Clip) -> f32 {
    let end = clip.start + clip.length;
    ((tempo.seconds_at(end) - tempo.seconds_at(clip.start)) * 1000.0) as f32
}

/// The length of a fade at `edge` of `clip` that lasts `ms`.
fn fade_ticks(tempo: &TempoMap, clip: &Clip, edge: Edge, ms: f32) -> Ticks {
    let seconds = f64::from(ms.clamp(0.0, clip_ms(tempo, clip))) / 1000.0;
    let end = clip.start + clip.length;
    match edge {
        Edge::Start => tempo.ticks_at_seconds(tempo.seconds_at(clip.start) + seconds) - clip.start,
        Edge::End => end - tempo.ticks_at_seconds(tempo.seconds_at(end) - seconds),
    }
}

/// Where the card goes: right of the clip's part in view, or left of it
/// when there is no room, kept across the lanes and inside the window
/// (`screen`), so a short window does not cut it off; at the top centre
/// when the clip is out of view.
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

/// One labelled row of the card.
fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
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

/// A colour swatch named `name`, ringed in the accent when `on`. The
/// track's colour is round, the clip colours square.
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
            1.5,
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

/// The clip colours as swatches, then the track's colour; returns the one
/// clicked (`Some(None)`: the track's colour again).
fn swatches(
    ui: &mut egui::Ui,
    current: Option<Color>,
    track: egui::Color32,
) -> Option<Option<Color>> {
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(theme::space::XS + 1.0, theme::space::XS + 1.0);
        for (i, &fill) in theme::CLIP_COLORS.iter().enumerate() {
            let color = color_from(fill);
            let name = format!("Colour {}", i + 1);
            if swatch(ui, fill, current == Some(color), false, &name).clicked() {
                chosen = Some(Some(color));
            }
        }
        if swatch(ui, track, current.is_none(), true, "Track colour").clicked() {
            chosen = Some(None);
        }
    });
    chosen
}

impl App {
    /// The selected clip that comes first: top track first, then by time.
    fn first_selected_clip(&self) -> Option<Id> {
        self.document
            .project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .find(|c| self.arrangement.clips.contains(&c.id))
            .map(|c| c.id)
    }

    /// The clip property keys on the selected clips: 0 mutes or unmutes,
    /// Ctrl+R or F2 renames, Alt+Enter opens the properties card. Runs
    /// before the track keys, so a clip selection takes F2.
    pub(crate) fn clip_property_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || self.typing {
            return;
        }
        let Some(first) = self.first_selected_clip() else {
            return;
        };
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_key(modifiers, key));
        if pressed(Modifiers::ALT, Key::Enter) {
            self.open_clip_card(first);
        } else if pressed(Modifiers::NONE, Key::Num0) {
            self.toggle_clip_mute();
        } else if pressed(Modifiers::COMMAND, Key::R) || pressed(Modifiers::NONE, Key::F2) {
            self.start_clip_rename(first);
        } else {
            return;
        }
        ctx.request_repaint();
    }

    /// Mutes the selected clips, or unmutes them when all are muted.
    fn toggle_clip_mute(&mut self) {
        let ids: Vec<Id> = self.arrangement.clips.iter().copied().collect();
        let project = self.document.project();
        let all = ids
            .iter()
            .all(|&id| find(project, id).is_some_and(|(c, _)| c.muted));
        self.document.set_clips_muted(&ids, !all);
    }

    /// Locks the selected clips, or unlocks them when all are locked.
    fn toggle_clip_lock(&mut self) {
        let ids: Vec<Id> = self.arrangement.clips.iter().copied().collect();
        let project = self.document.project();
        let all = ids
            .iter()
            .all(|&id| find(project, id).is_some_and(|(c, _)| c.locked));
        self.document.set_clips_locked(&ids, !all);
    }

    /// Starts renaming `id` in its title bar.
    pub(crate) fn start_clip_rename(&mut self, id: Id) {
        if let Some((clip, _)) = find(self.document.project(), id) {
            self.clip_props.renaming = Some((id, clip.name.clone()));
        }
    }

    /// Pins the properties card open on `id`, its name field focused.
    pub(crate) fn open_clip_card(&mut self, id: Id) {
        let props = &mut self.clip_props;
        props.pinned = Some(id);
        props.name = None;
        props.focus_name = true;
        props.renaming = None;
    }

    /// The clip entries of the lanes' menu, for the selected clips:
    /// rename, colour, mute, lock and the properties card.
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
        let (muted, locked) = (all(|c| c.muted), all(|c| c.locked));
        let name = |verb: &str| {
            if several {
                format!("{verb} clips")
            } else {
                format!("{verb} clip")
            }
        };
        let (mute, lock) = (
            name(if muted { "Unmute" } else { "Mute" }),
            name(if locked { "Unlock" } else { "Lock" }),
        );
        if ui
            .add(egui::Button::new("Rename").shortcut_text("Ctrl+R"))
            .clicked()
        {
            self.start_clip_rename(first);
            ui.close();
        }
        ui.menu_button("Colour", |ui| {
            if let Some(color) = swatches(ui, current, track) {
                self.document.recolor_clips(&ids, color);
                ui.close();
            }
        });
        if ui.add(egui::Button::new(mute).shortcut_text("0")).clicked() {
            self.toggle_clip_mute();
            ui.close();
        }
        if ui.button(lock).clicked() {
            self.toggle_clip_lock();
            ui.close();
        }
        if ui
            .add(egui::Button::new("Properties").shortcut_text("Alt+Enter"))
            .clicked()
        {
            self.open_clip_card(first);
            ui.close();
        }
        ui.separator();
    }

    /// Where clip `id` is drawn, if it is in view.
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

    /// The rename field in a clip's title bar, while it is renamed: Enter
    /// or clicking away applies, Esc cancels. A clip out of view is renamed
    /// in its card instead.
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

    /// The pinned properties card (P1), beside its clip. It follows the
    /// selection: selecting another clip moves it there, selecting none
    /// closes it, as do Esc and its close button.
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
        let clip = clip.clone();
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
                    .stroke(Stroke::new(1.0, palette.accent))
                    .show(ui, |ui| {
                        ui.set_width(CARD_WIDTH);
                        close = self.card_contents(ui, &clip, track, &tempo);
                    });
            });
        self.clip_props.size = Some(shown.response.rect.size());
        // Esc while typing cancels the typing; the next one closes.
        let typing = self.typing || ctx.egui_wants_keyboard_input();
        if close || (!typing && ctx.input(|i| i.key_pressed(Key::Escape))) {
            self.clip_props = ClipProps::default();
        }
    }

    /// What the card shows; true when its close button was clicked.
    fn card_contents(
        &mut self,
        ui: &mut egui::Ui,
        clip: &Clip,
        track: egui::Color32,
        tempo: &TempoMap,
    ) -> bool {
        let palette = *theme::p();
        let id = clip.id;
        let mut close = false;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                vec2(CARD_WIDTH - icons::CLOSE_SIZE - theme::space::S, 0.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(RichText::new("Clip").font(fonts::display(theme::size::TITLE)));
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = icons::close_button(ui).clicked();
            });
        });
        ui.add_space(theme::space::XS);

        row(ui, "Name", |ui| {
            let props = &mut self.clip_props;
            let text = props.name.get_or_insert_with(|| clip.name.clone());
            let cancel = ui.input(|i| i.key_pressed(Key::Escape));
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .id(egui::Id::new(("clip card name", id)))
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
                // Not being typed: follow the clip (an undo, a rename in
                // its title bar).
                props.name = None;
            }
        });

        row(ui, "Colour", |ui| {
            if let Some(color) = swatches(ui, clip.color, track) {
                self.document.recolor_clips(&[id], color);
            }
        });

        if let ClipContent::Audio { gain_db, media, .. } = &clip.content {
            ui.add_enabled_ui(!clip.locked, |ui| {
                row(ui, "Gain", |ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                    let db = widgets::value_slider(
                        ui,
                        rect,
                        egui::Id::new(("clip card gain", id)),
                        *gain_db,
                        SliderKind::ClipGain,
                        "Clip gain",
                        theme::controls(false),
                    );
                    if (db - gain_db).abs() > 1e-4 {
                        self.document.set_clip_gain(&[id], db, Some("card gain"));
                    }
                });
                for (edge, label, fade) in [
                    (Edge::Start, "Fade in", &clip.fade_in),
                    (Edge::End, "Fade out", &clip.fade_out),
                ] {
                    row(ui, label, |ui| {
                        let combo = 76.0;
                        let width = ui.available_width() - combo - ui.spacing().item_spacing.x;
                        let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
                        let ms = fade_ms(tempo, clip, edge);
                        let max_ms = clip_ms(tempo, clip);
                        let set = widgets::value_slider(
                            ui,
                            rect,
                            egui::Id::new(("clip card fade", id, label)),
                            ms,
                            SliderKind::Millis { max_ms },
                            label,
                            theme::controls(false),
                        );
                        if (set - ms).abs() > 0.05 {
                            let length = fade_ticks(tempo, clip, edge, set);
                            self.document
                                .set_fade_length(id, edge, length, Some("card fade"));
                        }
                        let now = FadeShape::of(&fade.curve);
                        egui::ComboBox::from_id_salt(("clip card shape", id, label))
                            .width(combo)
                            .selected_text(now.map_or("Custom", FadeShape::name))
                            .show_ui(ui, |ui| {
                                for shape in FadeShape::ALL {
                                    if ui
                                        .selectable_label(now == Some(shape), shape.name())
                                        .clicked()
                                    {
                                        self.document.set_fade_curve(
                                            &[id],
                                            edge,
                                            shape.curve(),
                                            None,
                                        );
                                    }
                                }
                            });
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
            if let ClipContent::Midi { notes } = &clip.content {
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

    /// The card's Mute and Lock toggles.
    fn card_toggles(&mut self, ui: &mut egui::Ui, clip: &Clip) {
        // Framed buttons that light when on, so off still reads as a
        // control.
        if ui
            .add(egui::Button::new("Mute clip").selected(clip.muted))
            .on_hover_text("Mute or unmute this clip (0)")
            .clicked()
        {
            self.document.set_clips_muted(&[clip.id], !clip.muted);
        }
        if ui
            .add(egui::Button::new("Lock clip").selected(clip.locked))
            .on_hover_text("A locked clip cannot be moved, trimmed, faded or deleted")
            .clicked()
        {
            self.document.set_clips_locked(&[clip.id], !clip.locked);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::fade::Fade;

    fn clip(start: i64, quarters: i64) -> Clip {
        Clip {
            id: Id(1),
            name: "Kick".into(),
            start: Ticks::quarters(start),
            length: Ticks::quarters(quarters),
            muted: false,
            color: None,
            locked: false,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
            loop_length: None,
            content: ClipContent::Midi { notes: vec![] },
        }
    }

    #[test]
    fn fade_lengths_read_and_set_in_milliseconds() {
        // 120 BPM: a quarter is 500 ms.
        let tempo = TempoMap::constant(120.0).expect("a tempo");
        let mut c = clip(4, 8);
        assert_eq!(clip_ms(&tempo, &c), 4000.0);
        c.fade_in.length = fade_ticks(&tempo, &c, Edge::Start, 250.0);
        c.fade_out.length = fade_ticks(&tempo, &c, Edge::End, 1000.0);
        assert_eq!(c.fade_in.length, Ticks(Ticks::quarters(1).0 / 2));
        assert_eq!(c.fade_out.length, Ticks::quarters(2));
        assert!((fade_ms(&tempo, &c, Edge::Start) - 250.0).abs() < 0.01);
        assert!((fade_ms(&tempo, &c, Edge::End) - 1000.0).abs() < 0.01);
        // Never longer than the clip.
        assert_eq!(fade_ticks(&tempo, &c, Edge::End, 1e9), c.length);
    }

    #[test]
    fn the_card_sits_beside_its_clip_inside_the_lanes() {
        let canvas = Rect::from_min_size(pos2(200.0, 100.0), vec2(800.0, 400.0));
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 560.0));
        let size = vec2(280.0, 250.0);
        // Room on the right.
        let clip = Rect::from_min_size(pos2(300.0, 150.0), vec2(100.0, 50.0));
        assert_eq!(
            card_place(Some(clip), canvas, screen, size),
            pos2(408.0, 150.0)
        );
        // Not on the right: on the left.
        let clip = Rect::from_min_size(pos2(800.0, 150.0), vec2(150.0, 50.0));
        assert_eq!(
            card_place(Some(clip), canvas, screen, size),
            pos2(512.0, 150.0)
        );
        // Neither: inside the right edge; low clips push it up, as far as
        // the window's bottom (the card may cover the time ruler).
        let clip = Rect::from_min_size(pos2(250.0, 420.0), vec2(700.0, 50.0));
        assert_eq!(
            card_place(Some(clip), canvas, screen, size),
            pos2(712.0, 302.0)
        );
        // Out of view: top centre.
        assert_eq!(card_place(None, canvas, screen, size), pos2(460.0, 108.0));
        let gone = Rect::from_min_size(pos2(0.0, 0.0), vec2(50.0, 50.0));
        assert_eq!(
            card_place(Some(gone), canvas, screen, size),
            pos2(460.0, 108.0)
        );
    }
}
