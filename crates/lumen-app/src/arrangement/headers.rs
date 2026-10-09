//! The track header column (M3 slice 4a): rail, fold arrow, type icon,
//! name, M S and the record-arm button; the unfolded volume and pan row;
//! resizing; the track and add-track menus. Design: DESIGN §7a, ADR-0025.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Id as EguiId, Key, Rect, Sense, Stroke, Ui, pos2,
    vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{Id, TrackKind};

use super::{
    Arrangement, BUTTON, FOLD_BELOW, FOLDED, MAX_HEIGHT, RowKind, STEP, color_from, color_of,
    snap_height,
};
use crate::commands::{Command, Keymap};
use crate::document::{Document, audible};
use crate::icons::draw_type_icon;
use crate::lanes::Lanes;
use crate::widgets::{self, SliderKind, draw_arrow, draw_edge_glow, gradient, value_slider};
use crate::{fonts, motion, theme};

impl Arrangement {
    /// The header column in `column`, whose top is level with the lanes'
    /// top (below the ruler).
    pub fn headers(
        &mut self,
        ui: &mut Ui,
        column: Rect,
        doc: &mut Document,
        lanes: &Lanes,
        keymap: &Keymap,
    ) {
        self.prune(doc.project());
        let ctx = ui.ctx().clone();
        let palette = *theme::p();
        let painter = ui.painter_at(column);
        painter.rect_filled(column, 0.0, palette.header);
        let rows = self.rows(&ctx, doc.project(), lanes.pending().len());
        let content = Self::content_height(&rows);

        // Below the last track: double-click or right-click to add one.
        let below = Rect::from_min_max(
            pos2(
                column.left(),
                (column.top() + content - self.top).max(column.top()),
            ),
            column.max,
        );
        if below.height() > 0.0 {
            let empty = ui.interact(below, ui.id().with("headers below"), Sense::click());
            empty.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Other,
                    true,
                    "Empty track area: double-click to add an audio track",
                )
            });
            if empty.hovered() {
                use crate::app::info::{Held, Info, Mouse};
                self.info = Some(
                    Info::new("Empty track area")
                        .on(Held::NONE, Mouse::DoubleClick, "add an audio track")
                        .on(Held::NONE, Mouse::RightClick, "add another kind")
                        .key(Command::AddAudioTrack, "audio track")
                        .key(Command::AddMidiTrack, "MIDI track"),
                );
            }
            if empty.double_clicked() {
                self.selected = vec![doc.add_track(TrackKind::Audio, None)];
            } else if empty.clicked() {
                self.selected.clear();
            }
            empty.context_menu(|ui| self.add_menu(ui, doc, None, keymap));

            // "+ Add track ▾": a menu of every track type with its shortcut,
            // and a hint that double-clicking here adds an audio track
            // (owner's choice, option B).
            let button = Rect::from_min_size(
                below.min + vec2(8.0, 8.0),
                vec2(column.width() - 16.0, 26.0),
            );
            if button.bottom() < column.bottom() {
                let response = ui
                    .interact(button, ui.id().with("add track"), Sense::click())
                    .on_hover_text("Add an audio, MIDI, group or return track");
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add track")
                });
                let hover =
                    motion::hover(&ctx, ui.id().with("add track hover"), response.hovered());
                let open =
                    egui::Popup::is_id_open(&ctx, egui::Popup::default_response_id(&response));
                painter.rect_filled(
                    button,
                    theme::radius::MEDIUM,
                    palette.control.lerp_to_gamma(palette.control_hover, hover),
                );
                if open || hover > 0.0 {
                    painter.rect_stroke(
                        button,
                        theme::radius::MEDIUM,
                        Stroke::new(
                            1.0,
                            palette
                                .accent
                                .gamma_multiply(if open { 1.0 } else { hover }),
                        ),
                        egui::StrokeKind::Inside,
                    );
                }
                painter.text(
                    pos2(button.left() + 10.0, button.center().y),
                    Align2::LEFT_CENTER,
                    "+ Add track",
                    FontId::proportional(theme::size::BODY),
                    palette.text,
                );
                draw_arrow(
                    &painter,
                    pos2(button.right() - 12.0, button.center().y),
                    1.0,
                    palette.text_secondary,
                );
                egui::Popup::menu(&response)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.set_min_width(button.width());
                        self.add_menu(ui, doc, self.selected.last().copied(), keymap);
                    });
            }
        }

        let modifiers = ui.input(|i| i.modifiers);
        let audible = audible(doc.project());
        let mut hot_edge = None;
        for &row in &rows {
            let rect = Rect::from_min_size(
                pos2(column.left(), column.top() + row.y - self.top),
                vec2(column.width(), row.height),
            );
            if rect.bottom() < column.top() || rect.top() > column.bottom() {
                continue;
            }
            let RowKind::Track(index) = row.kind else {
                // A file still loading: a quiet placeholder header.
                let name = lanes
                    .pending()
                    .get(match row.kind {
                        RowKind::Pending(i) => i,
                        RowKind::Track(_) => unreachable!(),
                    })
                    .and_then(|(id, _)| lanes.library.get(*id))
                    .map_or_else(String::new, |i| i.name.clone());
                painter.text(
                    pos2(rect.left() + 12.0, rect.top() + FOLDED / 2.0),
                    Align2::LEFT_CENTER,
                    format!("{name} \u{00B7} loading"),
                    FontId::proportional(theme::size::BODY),
                    palette.text_hint,
                );
                painter.hline(
                    column.x_range(),
                    rect.bottom() - 0.5,
                    Stroke::new(1.0, palette.border),
                );
                continue;
            };
            let track = doc.project().tracks[index].clone();
            let id = EguiId::new(("track header", track.id));
            let selected = self.selected.contains(&track.id);
            let heard = audible(&track);

            // Background: click to select, right-click for the menu.
            // Click to select; drag to move (the selection, or this track).
            let background = ui.interact(rect, id, Sense::click_and_drag());
            background.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    true,
                    selected,
                    track_description(&track),
                )
            });
            if background.clicked() {
                self.click_select(doc.project(), track.id, modifiers);
            }
            if background.drag_started() {
                self.start_drag(doc.project(), track.id);
            }
            background.context_menu(|ui| {
                if !self.selected.contains(&track.id) {
                    self.selected = vec![track.id];
                }
                self.track_menu(ui, doc, track.id, keymap);
            });
            let fill = if selected {
                palette.selected_row
            } else {
                palette.header
            };
            let look = theme::controls(selected);
            painter.rect_filled(rect, 0.0, fill);
            // The rail: the track colour, or red with a glow into the
            // header while armed.
            let rail = Rect::from_min_size(rect.min, vec2(4.0, rect.height()));
            if track.armed {
                gradient(
                    &painter,
                    Rect::from_min_size(rect.min, vec2(28.0, rect.height())),
                    palette.record.gamma_multiply(0.30),
                    Color32::TRANSPARENT,
                );
                painter.rect_filled(rail, 0.0, palette.record);
            } else {
                painter.rect_filled(rail, 0.0, color_of(track.color));
            }

            // Top row: fold arrow, name, M S ●.
            let mid = rect.top() + FOLDED / 2.0;
            let unfolded = motion::glide(
                &ctx,
                id.with("fold"),
                if self.expanded.contains(&track.id) {
                    1.0
                } else {
                    0.0
                },
                true,
            );
            let arrow = Rect::from_center_size(pos2(rect.left() + 16.0, mid), vec2(16.0, 18.0));
            let fold = ui
                .interact(arrow, id.with("fold button"), Sense::click())
                .on_hover_text(if self.expanded.contains(&track.id) {
                    "Fold the track"
                } else {
                    "Unfold the track (volume and pan; more detail in the lane)"
                });
            fold.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("Fold {}", track.name),
                )
            });
            if fold.clicked() {
                self.toggle_fold(track.id);
            }
            draw_arrow(
                &painter,
                arrow.center(),
                unfolded,
                if fold.hovered() {
                    palette.text
                } else {
                    palette.text_secondary
                },
            );

            // M, S and arm sit evenly spaced, snapped to whole pixels
            // together: egui snaps rectangles but not circles, so an
            // unsnapped arm button sat up to half a pixel off M and S, and
            // its ring off its dot (owner, after slice 4a). The armed glow
            // reaches `ARM_GLOW` beyond the button, inside the gap.
            let ppp = ui.ctx().pixels_per_point();
            let button = |x: f32| {
                Rect::from_min_size(pos2(x, mid - BUTTON / 2.0), vec2(BUTTON, BUTTON))
                    .round_to_pixels(ppp)
            };
            let arm_rect = button(rect.right() - 10.0 - BUTTON);
            let solo_rect = button(arm_rect.left() - BUTTON_GAP - BUTTON);
            let mute_rect = button(solo_rect.left() - BUTTON_GAP - BUTTON);

            // The track type, then the name, with room after the arrow.
            draw_type_icon(
                &painter,
                pos2(rect.left() + 34.0, mid),
                track.kind,
                if heard {
                    palette.text_secondary
                } else {
                    palette.text_hint
                },
            );
            let kind_hover =
                Rect::from_center_size(pos2(rect.left() + 34.0, mid), vec2(16.0, 16.0));
            let kind = ui
                .interact(kind_hover, id.with("kind"), Sense::hover())
                .on_hover_text(kind_name(track.kind));
            kind.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, kind_name(track.kind))
            });
            let name_rect = Rect::from_min_max(
                pos2(rect.left() + 48.0, mid - 10.0),
                pos2(mute_rect.left() - 6.0, mid + 10.0),
            );
            // The name is on top of the header's background: it gets the
            // same menu, so a right-click on it opens the track's menu too.
            if let Some(name) = self.name(ui, doc, &track, name_rect, heard, id) {
                let keys = keymap.keys(Command::Rename);
                let name = widgets::tip(name, "Double-click to rename", keys.as_ref(), None);
                name.context_menu(|ui| {
                    if !self.selected.contains(&track.id) {
                        self.selected = vec![track.id];
                    }
                    self.track_menu(ui, doc, track.id, keymap);
                });
            }

            let toggle =
                |ui: &mut Ui, r: Rect, what: &str, label: &str, on: bool, on_fill: Color32| {
                    let response =
                        ui.interact(r, id.with(what), Sense::click())
                            .on_hover_text(match what {
                                "mute" => "Mute (0)",
                                _ => "Solo: click for this track only, Ctrl+click to add it",
                            });
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Checkbox,
                            true,
                            on,
                            format!("{} {}", label, track.name),
                        )
                    });
                    let hover =
                        motion::hover(ui.ctx(), id.with(what).with("hover"), response.hovered());
                    let fill = if on {
                        on_fill
                    } else {
                        look.fill.lerp_to_gamma(look.hover, hover)
                    };
                    let p = ui.painter();
                    p.rect_filled(r, theme::radius::SMALL, fill);
                    p.text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        &label[..1],
                        fonts::medium(theme::size::CAPTION),
                        if on {
                            theme::text_on(on_fill)
                        } else {
                            look.text
                        },
                    );
                    response.clicked()
                };
            if toggle(
                ui,
                mute_rect,
                "mute",
                "Mute",
                track.mute,
                palette.start_marker,
            ) {
                doc.set_mute(&self.targets(track.id), !track.mute);
            }
            if toggle(ui, solo_rect, "solo", "Solo", track.solo, palette.accent) {
                doc.set_solo(&self.targets(track.id), !track.solo, !modifiers.command);
            }
            if arm_button(ui, arm_rect, id, &track.name, track.armed, look) {
                doc.set_armed(&self.targets(track.id), !track.armed);
            }

            // Unfolded: volume and pan on a second row.
            if row.height >= FOLDED + 22.0 {
                self.mixer_row(ui, doc, &track, rect, id, look);
            }

            // Bottom edge: drag to resize.
            let edge = Rect::from_min_max(
                pos2(rect.left(), rect.bottom() - 3.0),
                pos2(rect.right(), rect.bottom() + 3.0),
            );
            let resize = ui
                .interact(edge, id.with("resize"), Sense::drag())
                .on_hover_cursor(CursorIcon::ResizeVertical);
            resize.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Other,
                    true,
                    format!("Resize {}", track.name),
                )
            });
            if resize.drag_started() {
                self.resizing = Some((track.id, row.height));
            }
            if resize.dragged()
                && let Some((rid, start)) = self.resizing
                && rid == track.id
            {
                let total = resize.total_drag_delta().map_or(0.0, |d| d.y);
                let wanted = snap_height((start + total).clamp(FOLDED, MAX_HEIGHT));
                if wanted < FOLD_BELOW {
                    self.expanded.remove(&track.id);
                } else {
                    self.expanded.insert(track.id);
                    self.heights.insert(track.id, wanted);
                }
                // Follow the pointer exactly while dragging.
                ctx.data_mut(|d| d.remove::<f32>(EguiId::new(("track height", track.id))));
            }
            if resize.drag_stopped() {
                self.resizing = None;
            }
            painter.hline(
                column.x_range(),
                rect.bottom() - 0.5,
                Stroke::new(1.0, palette.border),
            );
            // The handle glows while hovered or dragged (owner request).
            let hot = resize.hovered() || self.resizing.is_some_and(|(r, _)| r == track.id);
            let glow = motion::hover(&ctx, id.with("edge glow"), hot);
            if glow > 0.0 {
                hot_edge = Some((track.id, glow));
                draw_edge_glow(&painter, column.x_range(), rect.bottom(), glow);
            }
        }
        self.hot_edge = hot_edge;
        painter.vline(
            column.right() - 0.5,
            column.y_range(),
            Stroke::new(1.0, palette.border),
        );
        // Dragged tracks are dimmed; the line shows where they will land.
        for row in &rows {
            if let RowKind::Track(index) = row.kind
                && self.is_dragged(doc.project().tracks[index].id)
            {
                let rect = Rect::from_min_size(
                    pos2(column.left(), column.top() + row.y - self.top),
                    vec2(column.width(), row.height),
                );
                painter.rect_filled(rect, 0.0, palette.header.gamma_multiply(0.6));
            }
        }
        self.follow_drag(ui, column, &rows, content, doc);
        self.draw_drop_line(&painter, column);
    }

    /// The name, or the rename field. Double-click to rename; Enter or
    /// clicking away keeps the new name, Esc cancels. The name's response,
    /// for its tooltip and menu.
    fn name(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &lumen_project::model::Track,
        rect: Rect,
        heard: bool,
        id: EguiId,
    ) -> Option<egui::Response> {
        let palette = theme::p();
        if let Some((renaming, text)) = &mut self.renaming
            && *renaming == track.id
        {
            let edit_id = id.with("rename");
            let response = ui.put(
                rect,
                egui::TextEdit::singleline(text)
                    .id(edit_id)
                    .font(FontId::proportional(theme::size::BODY))
                    .margin(vec2(4.0, 2.0)),
            );
            if !response.has_focus() && !response.lost_focus() {
                response.request_focus();
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                self.renaming = None;
            } else if response.lost_focus() {
                let name = text.clone();
                self.renaming = None;
                doc.rename_track(track.id, &name);
            }
            return None;
        }
        let response = ui.interact(rect, id.with("name"), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("{}: double-click to rename", track.name),
            )
        });
        if response.double_clicked() {
            self.renaming = Some((track.id, track.name.clone()));
        } else if response.clicked() {
            let modifiers = ui.input(|i| i.modifiers);
            self.click_select(doc.project(), track.id, modifiers);
        }
        let painter = ui.painter().clone();
        let galley = painter.layout(
            track.name.clone(),
            FontId::proportional(theme::size::BODY),
            if heard {
                palette.text
            } else {
                palette.text_hint
            },
            rect.width(),
        );
        // One line, shortened with "…" if too long.
        let text = if galley.rows.len() > 1 {
            let mut name = track.name.clone();
            while !name.is_empty() {
                name.pop();
                let g = painter.layout_no_wrap(
                    format!("{name}\u{2026}"),
                    FontId::proportional(theme::size::BODY),
                    palette.text,
                );
                if g.size().x <= rect.width() {
                    break;
                }
            }
            format!("{name}\u{2026}")
        } else {
            track.name.clone()
        };
        painter.text(
            pos2(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(theme::size::BODY),
            if heard {
                palette.text
            } else {
                palette.text_hint
            },
        );
        Some(response)
    }

    /// Volume and pan, for an unfolded track. Double-click resets either.
    fn mixer_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &lumen_project::model::Track,
        rect: Rect,
        id: EguiId,
        look: theme::Controls,
    ) {
        // One row of sliders that show their value inside (owner request):
        // volume fills from the left; pan fills from the centre and grows
        // more saturated the harder it pans.
        let center_y = rect.top() + FOLDED + STEP / 2.0;
        let left = rect.left() + 30.0;
        let right = rect.right() - 8.0;
        let pan_rect = Rect::from_min_max(
            pos2(right - 54.0, center_y - 10.0),
            pos2(right, center_y + 10.0),
        );
        let volume_rect = Rect::from_min_max(
            pos2(left, center_y - 10.0),
            pos2(pan_rect.left() - 6.0, center_y + 10.0),
        );
        let db = value_slider(
            ui,
            volume_rect,
            id.with("volume"),
            track.gain_db,
            SliderKind::Volume,
            &format!("Volume {}", track.name),
            look,
        );
        if db != track.gain_db {
            doc.set_volume(track.id, db);
        }
        let pan = value_slider(
            ui,
            pan_rect,
            id.with("pan"),
            track.pan,
            SliderKind::Pan,
            &format!("Pan {}", track.name),
            look,
        );
        if pan != track.pan {
            doc.set_pan(track.id, pan);
        }
    }

    /// Right-click menu on a track.
    fn track_menu(&mut self, ui: &mut Ui, doc: &mut Document, id: Id, keymap: &Keymap) {
        let targets = self.targets(id);
        let many = targets.len() > 1;
        let item = |ui: &mut Ui, label: &str, command: Command| {
            widgets::menu_item(ui, label, keymap.keys(command).as_ref(), true).clicked()
        };
        if item(ui, "Rename", Command::Rename)
            && let Some(track) = doc.project().tracks.iter().find(|t| t.id == id)
        {
            self.renaming = Some((id, track.name.clone()));
        }
        let duplicate = if many {
            "Duplicate tracks"
        } else {
            "Duplicate"
        };
        if item(ui, duplicate, Command::Duplicate) {
            self.selected = doc.duplicate_tracks(&targets);
        }
        let delete = if many { "Delete tracks" } else { "Delete" };
        if item(ui, delete, Command::Delete) {
            doc.delete_tracks(&targets);
            self.selected.clear();
        }
        if item(ui, "Import audio\u{2026}", Command::ImportAudio) {
            self.selected = vec![id];
            self.import_requested = true;
        }
        if item(ui, "Move up", Command::MoveTracksUp) {
            self.move_selected(doc, -1);
        }
        if item(ui, "Move down", Command::MoveTracksDown) {
            self.move_selected(doc, 1);
        }
        if ui
            .button(if self.expanded.contains(&id) {
                "Fold"
            } else {
                "Unfold"
            })
            .clicked()
        {
            self.toggle_fold(id);
            ui.close();
        }
        ui.menu_button("Colour", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(6.0 * 26.0);
                for c in theme::CLIP_COLORS {
                    let (r, response) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                    ui.painter().rect_filled(r, theme::radius::SMALL, c);
                    if response.hovered() {
                        ui.painter().rect_stroke(
                            r,
                            theme::radius::SMALL,
                            Stroke::new(1.5, theme::p().text),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if response.clicked() {
                        doc.recolor_tracks(&targets, color_from(c));
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        self.add_menu(ui, doc, Some(id), keymap);
    }

    /// "Add … track" entries, adding after `after`.
    fn add_menu(&mut self, ui: &mut Ui, doc: &mut Document, after: Option<Id>, keymap: &Keymap) {
        for (kind, text, command) in [
            (
                TrackKind::Audio,
                "Add audio track",
                Some(Command::AddAudioTrack),
            ),
            (
                TrackKind::Midi,
                "Add MIDI track",
                Some(Command::AddMidiTrack),
            ),
            (TrackKind::Group, "Add group track", None),
            (TrackKind::Return, "Add return track", None),
        ] {
            let keys = command.and_then(|c| keymap.keys(c));
            if widgets::menu_item(ui, text, keys.as_ref(), true).clicked() {
                self.selected = vec![doc.add_track(kind, after)];
            }
        }
    }
}

/// The record-arm button: round, with a red dot that lights when armed.
fn arm_button(
    ui: &mut Ui,
    rect: Rect,
    id: EguiId,
    name: &str,
    armed: bool,
    look: theme::Controls,
) -> bool {
    let palette = theme::p();
    let response = ui
        .interact(rect, id.with("arm"), Sense::click())
        .on_hover_text("Arm for recording (recording arrives in Milestone 9)");
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            true,
            armed,
            format!("Arm {name}"),
        )
    });
    let hover = motion::hover(ui.ctx(), id.with("arm hover"), response.hovered());
    let p = ui.painter();
    // Every circle shares this exact centre (owner: the ring must sit
    // centred on the dot).
    let center = rect.center();
    let radius = rect.width() / 2.0;
    if armed {
        // The one deliberate loop in Lumen (owner's choice): while armed, a
        // ring breathes slowly around the button. Reduce motion holds it
        // still; frames are requested only while a track is armed.
        let breath = if motion::reduced() {
            0.5
        } else {
            let t = ui.input(|i| i.time) as f32;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
            0.5 - 0.5 * (t * std::f32::consts::TAU / ARM_BREATH).cos()
        };
        for (spread, alpha) in [(ARM_GLOW, 0.10), (ARM_GLOW * 0.7, 0.18)] {
            p.circle_filled(
                center,
                radius + spread * (0.6 + 0.4 * breath),
                palette.record.gamma_multiply(alpha * (0.6 + 0.4 * breath)),
            );
        }
        p.circle_stroke(
            center,
            radius + 1.0 + breath,
            Stroke::new(1.5, palette.record.gamma_multiply(0.45 + 0.3 * breath)),
        );
        p.circle_filled(center, radius, palette.record.gamma_multiply(0.25));
        p.circle_filled(center, 4.5, palette.record);
    } else {
        p.circle_filled(center, radius, look.fill.lerp_to_gamma(look.hover, hover));
        p.circle_filled(
            center,
            4.5,
            palette.record.gamma_multiply(0.45 + 0.25 * hover),
        );
    }
    response.clicked()
}

/// Seconds per breath of the armed ring.
const ARM_BREATH: f32 = 2.4;

/// How far the armed glow reaches beyond the button, at its widest.
const ARM_GLOW: f32 = 4.0;

/// The space between M, S and the arm button; wider than the glow, so it
/// never touches S.
const BUTTON_GAP: f32 = 8.0;

/// What a screen reader says for a track header: "Track Kick, Audio
/// track, muted, armed for recording".
pub(super) fn track_description(track: &lumen_project::model::Track) -> String {
    let mut parts = vec![
        format!("Track {}", track.name),
        kind_name(track.kind).to_owned(),
    ];
    if track.mute {
        parts.push("muted".into());
    }
    if track.solo {
        parts.push("soloed".into());
    }
    if track.armed {
        parts.push("armed for recording".into());
    }
    parts.join(", ")
}

/// The track type, for the header icon's tooltip.
fn kind_name(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Audio => "Audio track",
        TrackKind::Midi => "MIDI track",
        TrackKind::Group => "Group track",
        TrackKind::Return => "Return track",
        TrackKind::Master => "Master track",
    }
}
