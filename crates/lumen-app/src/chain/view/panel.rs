//! The tab itself: the tray, its cables and chips, the cards as they are
//! painted, and what the frame asks for after them.

use super::menu::CardMenu;
use super::paint::{Chip, output_name, paint_cables, paint_chip, paint_drop_mark, paint_port};
use super::rows::{OwnedFace, Row};
use super::*;
use crate::chain::{Pictures, geometry, sidechain};

/// What the tab saw this frame, applied after the painting.
#[derive(Default)]
struct Events {
    select: Option<Id>,
    fold: Option<Id>,
    power: Option<(Id, bool)>,
    reload: Option<Id>,
    reload_clean: Option<Id>,
    step: Option<(Id, bool)>,
    rescan: bool,
    load_one: Option<Id>,
    strip: Vec<(Id, strip::Output)>,
}

/// What a drag over the tray carries.
#[derive(Clone)]
enum Carried {
    /// A device from the browser, to be added.
    New(PluginRef),
    /// A card of this chain, to be moved.
    Card(CardDrag),
}

impl App {
    /// The Devices tab in `area`.
    pub(crate) fn devices_panel(&mut self, ui: &mut Ui, area: Rect) {
        let palette = *theme::p();
        ui.painter().rect_filled(area, 0.0, palette.canvas);
        let Some(track_id) = self.chain_track() else {
            let state = widgets::EmptyState {
                icon: None,
                icon_size: theme::space::L,
                title: "Select a track to see its devices",
                hint: "Then double-click a device in the browser to add it",
                action: None,
            };
            widgets::empty_state(ui, area, &state);
            return;
        };
        let rate = self.transport.state().sample_rate;
        // What arrives for the track's sidechain, for the cards' lights.
        let keyed = all_tracks(self.document.project())
            .find(|t| t.id == track_id)
            .is_some_and(|t| t.devices.iter().any(|d| d.is_vst3() && !d.is_instrument()));
        if keyed {
            let playing = self.transport.state().playing;
            self.read_sidechain(ui.ctx(), track_id, playing);
        }
        let project = self.document.project();
        let Some(track) = all_tracks(project).find(|t| t.id == track_id) else {
            return;
        };
        let rows = self.rows_of(track, rate);
        let ctx = ui.ctx().clone();
        let ids: Vec<Id> = rows
            .iter()
            .filter(|r| r.field.is_some())
            .map(|r| r.id)
            .collect();
        let stopped = rows
            .iter()
            .any(|r| matches!(r.face, OwnedFace::Failed { .. }));
        for id in ids {
            self.ensure_listed(&ctx, id);
            let shown = self.strip_ids(id);
            self.hosting.ask_choices(id, &shown);
        }
        if stopped {
            // The age of the sound Reload would use keeps counting.
            ctx.request_repaint_after(crate::motion::AGE_FRAME);
        }
        let project = self.document.project();
        let Some(track) = all_tracks(project).find(|t| t.id == track_id) else {
            return;
        };
        let midi = track.kind == TrackKind::Midi;
        let input = Chip {
            name: track.name.clone(),
            kind: if midi { "MIDI" } else { "in" },
            colour: color_of(track.color),
        };
        let output = Chip {
            name: output_name(project, track),
            kind: "out",
            colour: palette.text_secondary,
        };
        let dotted_in = midi && rows.first().is_none_or(|r| r.instrument);
        let track_colour = color_of(track.color);
        let track_name = track.name.clone();

        let tray = area.shrink(theme::space::S);
        let painter = ui.painter().clone();
        raised::paint_sunken(&painter, tray, theme::radius::LARGE);
        if ui.rect_contains_pointer(tray) {
            self.chain.hovered = true;
        }
        let folded: Vec<bool> = rows
            .iter()
            .map(|r| self.chain.folded.contains(&r.id))
            .collect();
        let extra = if rows
            .iter()
            .any(|r| r.sidechain.is_some() || r.outputs.is_some())
        {
            geometry::KEY_ROOM
        } else {
            0.0
        };
        let pictures: Vec<Option<egui::TextureHandle>> =
            rows.iter().map(|r| self.picture_of(ui.ctx(), r)).collect();
        let selected = self.chain.selected;
        // A built-in with a drawing steps its card down with the tray's height
        // (whole, short, compact), with a little give either way (M8), and its
        // screen reads what the track hears.
        let steps: Vec<Option<geometry::Step>> = rows
            .iter()
            .map(|row| {
                row.drawing.as_ref().map(|d| {
                    let was = self.chain.steps.get(&row.id).copied();
                    geometry::Step::of(d.drawing.card, tray.height(), was)
                })
            })
            .collect();
        for (row, step) in rows.iter().zip(&steps) {
            if let Some(step) = step {
                self.chain.steps.insert(row.id, *step);
            }
        }
        let level = if motion::reduced() {
            0.0
        } else {
            card::level_of(self.arrangement.meters.reading(track_id.0).peak)
        };
        // A short tray turns the whole chain compact (D2), with a little
        // give either side of the switch so a drag across it does not flicker.
        let compact = geometry::compact(self.chain.compact, tray.height(), extra);
        self.chain.compact = compact;
        let mut looks: Vec<Look<'_>> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| Look {
                name: &row.name,
                vendor: &row.vendor,
                identity: &row.identity,
                instrument: row.instrument,
                bypass: row.bypass,
                selected: selected == Some(row.id),
                face: row.face.face(),
                latency_ms: row.latency_ms,
                reports_latency: row.vst3 || row.builtin,
                running: row.running,
                track_color: track_colour,
                band: 0.0,
                folded: folded[i],
                picture: pictures[i].as_ref(),
                slots: &row.slots,
                parameters: row.parameters,
                editor: &row.editor,
                learning: row.learning,
                field: row.field.as_ref().map(|f| Field {
                    name: f.name.as_deref(),
                    changed: f.changed,
                }),
                sidechain: row.sidechain.as_ref(),
                outputs: row.outputs.as_ref(),
                personal: steps[i].and_then(|step| personal_of(row, step, level)),
                compact,
                message: Default::default(),
            })
            .collect();
        // The tallest compact card sets how short the panel can get, so a
        // card's whole message always fits.
        let heights: Vec<f32> = looks.iter().map(|l| card::compact::height(ui, l)).collect();
        self.chain.least = heights.iter().copied().fold(0.0, f32::max);
        let plain_h = geometry::card_height_with(tray.height() - 2.0 * geometry::PAD, extra);
        let layout = if steps.iter().any(Option::is_some) {
            // Cards of their own sizes: a faced card's is its step's.
            let sizes: Vec<egui::Vec2> = rows
                .iter()
                .enumerate()
                .map(|(i, row)| match (&row.drawing, steps[i]) {
                    (Some(d), Some(step)) if !folded[i] => step.size(d.drawing.card),
                    _ => vec2(geometry::CARD_W, if compact { heights[i] } else { plain_h }),
                })
                .collect();
            Layout::mixed(&folded, tray.height(), &sizes)
        } else if compact {
            Layout::sized(&folded, tray.height(), &heights)
        } else {
            Layout::keyed(&folded, tray.height(), extra)
        };
        if !compact {
            for (i, (look, row)) in looks.iter_mut().zip(&rows).enumerate() {
                let room = if row.sidechain.is_some() || row.outputs.is_some() {
                    sidechain::ROOM
                } else {
                    0.0
                };
                look.band = geometry::band(layout.card(i).height() - room);
            }
        }
        let playing = self.transport.state().playing;
        let beam = motion::hover(ui.ctx(), egui::Id::new("chain beam"), playing);
        let mut events = Events::default();
        // A device dragged from the browser, or a card of this chain picked
        // up: where it would land.
        let from_browser = egui::DragAndDrop::payload::<Dragged>(ui.ctx()).and_then(|d| {
            self.plugins
                .catalog()
                .find(&d.0)
                .and_then(actions::plugin_ref)
        });
        let picked = egui::DragAndDrop::payload::<CardDrag>(ui.ctx())
            .filter(|d| d.track == track_id)
            .and_then(|d| rows.iter().find(|r| r.id == d.device).map(|r| (*d, r)));
        let dragged: Option<(Carried, DeviceKind)> = match (&from_browser, &picked) {
            (Some(plugin), _) => Some((Carried::New(plugin.clone()), plugin.kind())),
            (None, Some((card, row))) => Some((
                Carried::Card(*card),
                if row.instrument {
                    DeviceKind::Instrument
                } else {
                    DeviceKind::Effect
                },
            )),
            _ => None,
        };
        if let Some((_, row)) = &picked {
            paint_drag_ghost(ui.ctx(), &row.name);
        }
        let released = dragged.is_some() && ui.ctx().input(|i| i.pointer.any_released());
        let mut dropped: Option<(Carried, usize)> = None;

        let mut browse = false;
        let mut scroll_ui = ui.new_child(UiBuilder::new().max_rect(tray.shrink(1.0)));
        egui::ScrollArea::horizontal()
            .id_salt(("chain scroll", track_id.0))
            .auto_shrink([false, false])
            .show(&mut scroll_ui, |ui| {
                let size = vec2(layout.width().max(tray.width() - 2.0), tray.height() - 2.0);
                let (whole, _) = ui.allocate_exact_size(size, Sense::hover());
                let origin = whole.min;
                let at = |r: Rect| r.translate(origin.to_vec2());
                let painter = ui.painter().with_clip_rect(ui.clip_rect());

                paint_cables(&painter, &layout, origin, dotted_in, beam);
                paint_chip(&painter, at(layout.input()), &input);
                paint_chip(&painter, at(layout.output()), &output);
                paint_port(&painter, at(layout.input()).right_center(), beam);
                paint_port(&painter, at(layout.output()).left_center(), beam);
                // The tray's own menu, in its empty state and past the last
                // card: a device can be added from here, without the browser
                // (M8 slice B0, review M4). It is drawn before the cards and
                // the empty state so their own controls take the pointer.
                let tail = Rect::from_min_max(
                    pos2(at(layout.output()).right() + theme::space::S, whole.top()),
                    whole.max,
                );
                let tail = ui.interact(
                    tail,
                    egui::Id::new(("chain tail", track_id.0)),
                    Sense::click(),
                );
                tail.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "Add a device to the chain",
                    )
                });
                widgets::Menu::context(&tail).show(|ui| {
                    // The row lands on the track whose chain this is.
                    self.arrangement.selected = vec![track_id];
                    self.menu_command(ui, Command::AddUtility, "Add Prism (utility)");
                    self.menu_command(ui, Command::AddSampler, "Add Shutter (sampler)");
                });
                if rows.is_empty() {
                    // The shared empty state (the interface audit's D1, the
                    // owner's A), above the in and out chips and their cable.
                    let above = Rect::from_min_max(
                        whole.min,
                        pos2(
                            whole.right(),
                            layout.cable_y + origin.y - CHIP_H / 2.0 - theme::space::XS,
                        ),
                    );
                    let title = format!("No devices on {track_name}");
                    let state = widgets::EmptyState {
                        icon: None,
                        icon_size: theme::space::L,
                        title: &title,
                        hint: "Double-click a device in the browser, or drag one here",
                        action: Some(widgets::EmptyAction {
                            label: "Browse devices",
                            keys: None,
                            tip: "Open the browser on its Devices tab",
                        }),
                    };
                    browse = widgets::empty_state(ui, above, &state);
                }
                for (i, row) in rows.iter().enumerate() {
                    let rect = at(layout.card(i));
                    if !ui.is_rect_visible(rect.expand(32.0)) {
                        continue;
                    }
                    paint_port(&painter, pos2(rect.left(), layout.cable_y + origin.y), beam);
                    paint_port(
                        &painter,
                        pos2(rect.right(), layout.cable_y + origin.y),
                        beam,
                    );
                    let look = &looks[i];
                    let out = card::paint(ui, rect, egui::Id::new(("device", row.id.0)), look);
                    if let Some(field) = &out.field {
                        self.preset_menu(field, row.id);
                    }
                    if let Some(chooser) = &out.sidechain {
                        self.sidechain_menu(chooser, row.id);
                    }
                    if let Some(chooser) = &out.outputs {
                        self.outputs_menu(chooser, row.id);
                    }
                    if let Some(forward) = out.step {
                        events.step = Some((row.id, forward));
                    }
                    // A second device of the same name says which it is.
                    let nth = rows[..i].iter().filter(|r| r.name == row.name).count();
                    let spoken = if nth == 0 {
                        format!("{} on {}", row.name, track_name)
                    } else {
                        format!("{} ({}) on {}", row.name, nth + 1, track_name)
                    };
                    out.body.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            true,
                            look.selected,
                            spoken.clone(),
                        )
                    });
                    if out.body.clicked() || out.body.secondary_clicked() {
                        events.select = Some(row.id);
                    }
                    if out.body.double_clicked() {
                        events.fold = Some(row.id);
                    }
                    if out.body.drag_started() {
                        events.select = Some(row.id);
                        egui::DragAndDrop::set_payload(
                            ui.ctx(),
                            CardDrag {
                                track: track_id,
                                device: row.id,
                            },
                        );
                    }
                    if out.power.clicked() {
                        events.power = Some((row.id, !row.bypass));
                    }
                    for output in out.strip {
                        events.strip.push((row.id, output));
                    }
                    match out.action {
                        Some(Action::Reload) => events.reload = Some(row.id),
                        Some(Action::ReloadClean) => events.reload_clean = Some(row.id),
                        Some(Action::Rescan) => events.rescan = true,
                        Some(Action::LoadOne) => events.load_one = Some(row.id),
                        None => {}
                    }
                    let ready = matches!(row.face, OwnedFace::Ready);
                    let running = ready && (row.vst3 || row.builtin);
                    let menu = CardMenu {
                        id: row.id,
                        bypass: row.bypass,
                        folded: folded[i],
                        compact: compact && !folded[i],
                        preset: row.field.as_ref().map(|f| f.name.clone()),
                        parameters: if running { row.parameters } else { 0 },
                        learning: row.learning,
                        window: (row.vst3
                            && ready
                            && !matches!(row.editor, strip::Editor::Unavailable(_)))
                        .then_some(matches!(
                            row.editor,
                            strip::Editor::Open | strip::Editor::Opening
                        )),
                    };
                    widgets::Menu::context(&out.body).show(|ui| {
                        self.card_menu(ui, track_id, &menu);
                    });
                    // The compact card's name opens the same menu under it.
                    if let Some(name) = &out.name {
                        widgets::Menu::below(name).show(|ui| {
                            if self.card_menu(ui, track_id, &menu) {
                                ui.close();
                            }
                        });
                    }
                }
                if let Some((carried, kind)) = &dragged
                    && let Some(pointer) = ui.ctx().pointer_latest_pos()
                    && tray.contains(pointer)
                {
                    let instrument = *kind == DeviceKind::Instrument;
                    if instrument && !midi {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
                    } else {
                        // An effect lands in a gap, never before the
                        // instrument; an instrument takes the first place.
                        let floor = rows.iter().position(|r| r.instrument).map_or(0, |i| i + 1);
                        let gap = if instrument {
                            0
                        } else {
                            layout.gap_at(pointer.x - origin.x).max(floor)
                        };
                        paint_drop_mark(
                            &painter,
                            pos2(layout.gap_x(gap) + origin.x, layout.cable_y + origin.y),
                            whole.y_range(),
                        );
                        if released {
                            dropped = Some((carried.clone(), gap));
                        }
                    }
                }
            });
        if browse {
            self.show_device_browser();
        }

        if let Some((carried, gap)) = dropped {
            egui::DragAndDrop::clear_payload(ui.ctx());
            let slot = Slot::At(gap);
            match carried {
                Carried::New(plugin) => {
                    let place = Place::Track {
                        track: track_id,
                        slot,
                    };
                    self.add_device_showing_window(plugin, place);
                }
                Carried::Card(card) => {
                    self.send_device(track_id, card.device, track_id, slot, false);
                }
            }
        }
        if let Some(id) = events.select {
            self.chain.selected = Some(id);
        }
        if let Some(id) = events.fold {
            self.chain.selected = Some(id);
            self.toggle_fold_of(id);
        }
        if let Some((id, bypass)) = events.power {
            self.chain.selected = Some(id);
            self.bypass_device(track_id, id, bypass);
        }
        for (device, output) in events.strip {
            self.chain.selected = Some(device);
            self.apply_strip(device, output);
        }
        if self.chain.text_pending.replace(false) {
            ui.ctx().request_repaint_after(crate::motion::TEXT_FRAME);
        }
        if let Some(id) = events.reload {
            self.hosting.reload(id);
        }
        if let Some(id) = events.reload_clean {
            self.hosting.reload_clean(id);
        }
        if let Some((id, forward)) = events.step {
            self.chain.selected = Some(id);
            self.step_preset(ui.ctx(), id, forward);
        }
        if let Some(id) = events.load_one {
            self.load_one_plugin(id);
        }
        if events.rescan {
            self.rescan_plugins(ui.ctx(), lumen_plugin_catalog::scan::Mode::Changed);
        }
    }

    /// The plugin's own picture for `row`, once its bundle has been read.
    fn picture_of(&mut self, ctx: &egui::Context, row: &Row) -> Option<egui::TextureHandle> {
        if !row.vst3 {
            return None;
        }
        let module = self
            .plugins
            .catalog()
            .find(&format!("vst3:{}", row.identity))
            .and_then(|listed| listed.module.clone());
        let wake = ctx.clone();
        let pictures = self
            .chain
            .pictures
            .get_or_insert_with(|| Pictures::new(move || wake.request_repaint()));
        pictures.poll(ctx);
        pictures.get(&row.identity, module.as_deref())
    }
}
