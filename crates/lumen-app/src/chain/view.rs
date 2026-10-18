//! The Devices tab: the shown track's chain in a sunk tray (the owner's C4).
//! Cables join the chips at either end and every card; they carry a Beam,
//! the playhead's light, while the song plays, and a dotted line carries a
//! MIDI track's notes into its instrument.

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rangef, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};
use lumen_project::model::{Device, DeviceKind, Id, Output, PluginRef, Project, Track, TrackKind};
use lumen_project::routing::all_tracks;

use super::card::{self, Action, Face, Look};
use super::geometry::{CHIP_H, Layout, PORT};
use super::place::Place;
use super::strip;
use super::{CardDrag, Dragged, actions};
use crate::app::panes::{self, Pane};
use crate::arrangement::color_of;
use crate::commands::Command;
use crate::document::Slot;
use crate::hosting::State;
use crate::widgets::raised;
use crate::{App, light, motion, theme, widgets};

/// A device's row in the chain, with what the card needs (no state blobs).
struct Row {
    id: Id,
    name: String,
    vendor: String,
    identity: String,
    /// A VST3 plugin: its class id is the identity.
    vst3: bool,
    instrument: bool,
    bypass: bool,
    face: OwnedFace,
    latency_ms: f32,
    /// The strip's knobs and how many parameters the plugin offers.
    slots: Vec<Option<strip::Slot>>,
    parameters: usize,
    /// The plugin's own window and Learn.
    editor: strip::Editor,
    learning: bool,
}

enum OwnedFace {
    Ready,
    Loading,
    Failed(String),
    Missing(usize),
    Safe(usize),
}

impl OwnedFace {
    fn face(&self) -> Face<'_> {
        match self {
            OwnedFace::Ready => Face::Ready,
            OwnedFace::Loading => Face::Loading,
            OwnedFace::Failed(words) => Face::Failed(words),
            OwnedFace::Missing(saved) => Face::Missing { saved: *saved },
            OwnedFace::Safe(saved) => Face::Safe { saved: *saved },
        }
    }
}

/// What the tab saw this frame, applied after the painting.
#[derive(Default)]
struct Events {
    select: Option<Id>,
    fold: Option<Id>,
    power: Option<(Id, bool)>,
    reload: Option<Id>,
    rescan: bool,
    load_one: Option<Id>,
    strip: Vec<(Id, strip::Output)>,
}

impl App {
    /// Once a frame, before the keys: the chosen card is still on the shown
    /// track, and Delete means "remove it" while the tab has the keyboard.
    pub(crate) fn refresh_chain(&mut self, ctx: &egui::Context) {
        // A card let go on a track's header last frame: follow it there, or
        // say why it was refused.
        if let Some(drop) = self.arrangement.device_drop.take() {
            match drop.result {
                Ok(device) => {
                    self.arrangement.selected = vec![drop.to];
                    self.chain.selected = Some(device);
                }
                Err(refusal) => self.say(refusal.to_string()),
            }
        }
        let shown = self.chain_track();
        let project = self.document.project();
        let still_there = |chosen: Id| {
            shown.is_some_and(|track| {
                all_tracks(project)
                    .any(|t| t.id == track && t.devices.iter().any(|d| d.id == chosen))
            })
        };
        if self.chain.selected.is_some_and(|c| !still_there(c)) {
            self.chain.selected = None;
        }
        let docked = self.show_detail
            && self.detail_tab == crate::app::detail::DetailTab::Devices
            && (self.chain.hovered || panes::focused(ctx) == Some(Pane::Detail));
        self.chain.live = docked && self.chain.selected.is_some();
        self.chain.hovered = false;
    }

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
        let project = self.document.project();
        let Some(track) = all_tracks(project).find(|t| t.id == track_id) else {
            return;
        };
        let rows = self.rows_of(track, rate);
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
        let layout = Layout::new(&folded, tray.height());
        let pictures: Vec<Option<egui::TextureHandle>> =
            rows.iter().map(|r| self.picture_of(ui.ctx(), r)).collect();
        let playing = self.transport.state().playing;
        let beam = motion::hover(ui.ctx(), egui::Id::new("chain beam"), playing);
        let mut events = Events::default();
        let selected = self.chain.selected;
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
                if rows.is_empty() {
                    let gap = Rect::from_min_max(
                        pos2(at(layout.input()).right(), whole.top()),
                        pos2(at(layout.output()).left(), whole.bottom()),
                    );
                    painter.text(
                        pos2(gap.center().x, layout.cable_y + origin.y - CHIP_H),
                        Align2::CENTER_BOTTOM,
                        "Double-click a device in the browser to add it here",
                        FontId::proportional(theme::size::CAPTION),
                        palette.text_hint,
                    );
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
                    let look = Look {
                        name: &row.name,
                        vendor: &row.vendor,
                        identity: &row.identity,
                        instrument: row.instrument,
                        bypass: row.bypass,
                        selected: selected == Some(row.id),
                        face: row.face.face(),
                        latency_ms: row.latency_ms,
                        reports_latency: row.vst3,
                        track_color: track_colour,
                        band: super::geometry::band(layout.card_h),
                        folded: folded[i],
                        picture: pictures[i].as_ref(),
                        slots: &row.slots,
                        parameters: row.parameters,
                        editor: &row.editor,
                        learning: row.learning,
                    };
                    let out = card::paint(ui, rect, egui::Id::new(("device", row.id.0)), &look);
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
                        Some(Action::Rescan) => events.rescan = true,
                        Some(Action::LoadOne) => events.load_one = Some(row.id),
                        None => {}
                    }
                    let (id, bypass, is_folded) = (row.id, row.bypass, folded[i]);
                    widgets::Menu::context(&out.body).show(|ui| {
                        self.chain.selected = Some(id);
                        let bypass_label = if bypass { "Turn on" } else { "Bypass" };
                        self.menu_command(ui, Command::DeviceBypass, bypass_label);
                        let fold_label = if is_folded { "Open" } else { "Fold" };
                        self.menu_command(ui, Command::DeviceFold, fold_label);
                        self.menu_command(ui, Command::DeviceMoveEarlier, "Move earlier");
                        self.menu_command(ui, Command::DeviceMoveLater, "Move later");
                        self.menu_command(ui, Command::DeviceDuplicate, "Duplicate");
                        self.menu_command(ui, Command::DeviceCopy, "Copy");
                        self.menu_command(ui, Command::DevicePaste, "Paste");
                        self.send_menus(ui, track_id);
                        self.menu_command(ui, Command::DeviceRemove, "Remove");
                    });
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

        if let Some((carried, gap)) = dropped {
            egui::DragAndDrop::clear_payload(ui.ctx());
            let slot = Slot::At(gap);
            match carried {
                Carried::New(plugin) => self.add_device_at(
                    plugin,
                    Place::Track {
                        track: track_id,
                        slot,
                    },
                ),
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
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(60));
        }
        if let Some(id) = events.reload {
            self.hosting.reload(id);
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
            .get_or_insert_with(|| super::Pictures::new(move || wake.request_repaint()));
        pictures.poll(ctx);
        pictures.get(&row.identity, module.as_deref())
    }

    /// The shown track's devices as rows, with how each is running.
    fn rows_of(&self, track: &Track, rate: u32) -> Vec<Row> {
        track.devices.iter().map(|d| self.row_of(d, rate)).collect()
    }

    fn row_of(&self, d: &Device, rate: u32) -> Row {
        let (name, vendor, identity) = match &d.plugin {
            PluginRef::Vst3 {
                class_id,
                name,
                vendor,
                ..
            } => (name.clone(), vendor.clone(), class_id.clone()),
            PluginRef::Builtin { name } => (
                crate::document::device_name(d),
                "Lumen".to_owned(),
                name.clone(),
            ),
        };
        let (face, frames) = if d.is_vst3() {
            let face = match self.hosting.state(d.id) {
                Some(State::Ready) => OwnedFace::Ready,
                Some(State::Failed(words)) => OwnedFace::Failed(words.clone()),
                Some(State::Missing) => {
                    OwnedFace::Missing(d.state.len() + d.controller_state.len())
                }
                Some(State::Safe) => OwnedFace::Safe(d.state.len() + d.controller_state.len()),
                Some(State::Loading) | None => OwnedFace::Loading,
            };
            let frames = self
                .hosting
                .latency(d.id)
                .or_else(|| d.reported.map(|r| r.latency_at(rate)));
            (face, frames.unwrap_or(0))
        } else {
            (OwnedFace::Ready, 0)
        };
        let latency_ms = if rate == 0 {
            0.0
        } else {
            frames as f32 / rate as f32 * 1000.0
        };
        let (slots, parameters) = if d.is_vst3() && matches!(face, OwnedFace::Ready) {
            self.strip_of(d)
        } else {
            (Vec::new(), 0)
        };
        let editor = self.editor_look(d);
        Row {
            id: d.id,
            slots,
            parameters,
            editor,
            learning: self.chain.learning == Some(d.id),
            name,
            vendor,
            identity,
            vst3: d.is_vst3(),
            instrument: d.is_instrument(),
            bypass: d.bypass,
            face,
            latency_ms,
        }
    }
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
    /// "Move to track" and "Copy to track" in a card's menu: the tracks that
    /// can take the chosen device.
    fn send_menus(&mut self, ui: &mut Ui, here: Id) {
        let targets = self.send_targets();
        for (copy, label) in [(false, "Move to track"), (true, "Copy to track")] {
            let mut chosen = None;
            widgets::submenu(ui, label, |ui| {
                if targets.is_empty() {
                    ui.add_enabled(false, egui::Label::new("No track can take it"));
                }
                for (id, name) in &targets {
                    // Moving to its own track is reordering, not this.
                    let enabled = copy || *id != here;
                    if widgets::menu_item(ui, name, None, enabled).clicked() {
                        chosen = Some(*id);
                        ui.close();
                    }
                }
            });
            if let Some(to) = chosen {
                self.send_chain_device(to, copy);
            }
        }
    }
}

/// A chip at either end of the chain.
struct Chip {
    name: String,
    /// "MIDI", "in" or "out": said in mono beside the name.
    kind: &'static str,
    colour: Color32,
}

fn paint_chip(painter: &egui::Painter, rect: Rect, chip: &Chip) {
    let palette = theme::p();
    let face = raised::Face::new(theme::controls(false));
    raised::paint(painter, rect, theme::radius::FULL, &face);
    painter.rect_filled(
        Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(8.0, 8.0)),
        2.0,
        chip.colour,
    );
    let mono = FontId::monospace(theme::size::MONO_SMALL);
    let kind = painter.layout_no_wrap(chip.kind.to_owned(), mono.clone(), palette.text_hint);
    let name_room = rect.width() - 24.0 - kind.size().x - 16.0;
    let font = FontId::proportional(theme::size::CAPTION);
    let name = widgets::fit(painter, &chip.name, font.clone(), name_room);
    painter.text(
        pos2(rect.left() + 22.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        font,
        palette.text,
    );
    painter.galley(
        pos2(
            rect.right() - 10.0 - kind.size().x,
            rect.center().y - kind.size().y / 2.0,
        ),
        kind,
        palette.text_hint,
    );
}

/// Where a dragged device would land: an accent line across the tray and a
/// plus on the cable. Neighbours stay put.
fn paint_drop_mark(painter: &egui::Painter, at: Pos2, y: Rangef) {
    let palette = theme::p();
    light::vline(
        painter,
        light::LIT,
        at.x,
        y.shrink(theme::space::M),
        palette.accent,
        1.0,
    );
    painter.circle_filled(at, 9.0, palette.accent);
    let stroke = Stroke::new(1.5, palette.on_accent);
    painter.hline(Rangef::new(at.x - 4.0, at.x + 4.0), at.y, stroke);
    painter.vline(at.x, Rangef::new(at.y - 4.0, at.y + 4.0), stroke);
}

/// The device being dragged, as a small pill that follows the pointer.
pub(crate) fn paint_drag_ghost(ctx: &egui::Context, name: &str) {
    let Some(pointer) = ctx.pointer_latest_pos() else {
        return;
    };
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let palette = theme::p();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("device drag ghost"),
    ));
    let font = FontId::proportional(theme::size::BODY);
    let galley = painter.layout_no_wrap(name.to_owned(), font, palette.text);
    let rect = Rect::from_min_size(
        pointer + vec2(12.0, 8.0),
        galley.size() + vec2(2.0 * theme::space::M, theme::space::S),
    );
    painter.rect_filled(rect, theme::radius::MEDIUM, palette.overlay);
    painter.rect_stroke(
        rect,
        theme::radius::MEDIUM,
        Stroke::new(theme::stroke::HAIRLINE, palette.accent),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        rect.min + vec2(theme::space::M, theme::space::S / 2.0),
        galley,
        palette.text,
    );
}

/// A port: a ring on the card's edge, lit with the cable.
fn paint_port(painter: &egui::Painter, at: Pos2, beam: f32) {
    let palette = theme::p();
    painter.circle(
        at,
        PORT,
        palette.well,
        Stroke::new(theme::stroke::LIT, palette.border_strong),
    );
    light::dot(painter, light::BEAM, at, PORT - 1.0, palette.accent, beam);
}

/// The cables between the chips and the cards, solid for audio and dotted
/// for the notes into an instrument.
fn paint_cables(
    painter: &egui::Painter,
    layout: &Layout,
    origin: Pos2,
    dotted_first: bool,
    beam: f32,
) {
    let palette = theme::p();
    let y = layout.cable_y + origin.y;
    for (i, (from, to)) in layout.cables().into_iter().enumerate() {
        let (from, to) = (from + origin.x, to + origin.x);
        if i == 0 && dotted_first {
            let colour = palette.border_strong.lerp_to_gamma(palette.accent, beam);
            painter.extend(egui::Shape::dotted_line(
                &[pos2(from, y), pos2(to, y)],
                colour,
                5.0,
                1.5,
            ));
        } else {
            painter.hline(
                Rangef::new(from, to),
                y,
                Stroke::new(2.0, palette.border_strong),
            );
            light::hline(
                painter,
                light::BEAM,
                Rangef::new(from, to),
                y,
                palette.accent,
                beam,
            );
        }
    }
}

/// Where a track's sound goes, in words for the output chip.
fn output_name(project: &Project, track: &Track) -> String {
    let named = |id: Id| {
        all_tracks(project)
            .find(|t| t.id == id)
            .map(|t| t.name.clone())
    };
    if track.kind == TrackKind::Master {
        // The master plays to a pair of the device (the main one by default).
        return match track.output {
            Output::Device(pair) => format!("Out {}/{}", 2 * pair + 1, 2 * pair + 2),
            _ => "Out 1/2".to_owned(),
        };
    }
    match track.output {
        Output::Master => "Master".to_owned(),
        Output::Track(id) => named(id).unwrap_or_else(|| "Master".to_owned()),
        Output::Device(pair) => format!("Out {}/{}", 2 * pair + 1, 2 * pair + 2),
        Output::Auto => track
            .parent
            .and_then(named)
            .unwrap_or_else(|| "Master".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_chip_names_where_the_sound_goes() {
        let mut project = crate::empty_project();
        let color = project.master.color;
        let group = Track::new(Id(1), "Drums".to_owned(), TrackKind::Group, color);
        let mut kick = Track::new(Id(2), "Kick".to_owned(), TrackKind::Audio, color);
        kick.parent = Some(Id(1));
        project.tracks = vec![group.into(), kick.clone().into()];
        assert_eq!(output_name(&project, &kick), "Drums");
        kick.output = Output::Master;
        assert_eq!(output_name(&project, &kick), "Master");
        kick.output = Output::Device(1);
        assert_eq!(output_name(&project, &kick), "Out 3/4");
        kick.output = Output::Track(Id(99));
        assert_eq!(output_name(&project, &kick), "Master");
    }
}
