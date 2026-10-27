//! The pointer, keys and typing on the tempo and meter lanes (M7 slice A6).
//! What the lanes are and look like is `song_lanes/`; every edit here is a
//! `Document` method, one undo step, and a drag is one step in all.
//!
//! On the tempo lane a click adds a change at the pointer (snapped in time,
//! whole BPM in value; Alt for off the grid and tenths), a drag moves a
//! change, a double-click types its tempo, Delete takes a change away (the
//! first stays). On the meter lane a click on empty lane asks for a time
//! signature on the nearest bar line, a drag moves a chip to another bar
//! line, a double-click types its signature.

use eframe::egui::{self, CursorIcon, Id, Pos2, Rect, Sense, pos2, vec2};
use lumen_core::time::{TempoPoint, Ticks};
use lumen_project::edit::maps::tempo_in;

use super::info::{Held, Info, Mouse};
use super::rulers::Layout;
use crate::App;
use crate::commands::Command;
use crate::song_lanes::scale::{
    self, Plane, Scale, bpm_text, meter_hit, parse_bpm, snap_bpm, tempo_hit,
};
use crate::song_lanes::{Band, Gesture, Look, MenuSpot, Picked, SongLanes, Typing, Which};
use crate::{motion, theme, widgets};

/// The merge keys of a drag on each lane.
const TEMPO_DRAG: &str = "tempo lane";
const METER_DRAG: &str = "meter lane";

impl App {
    /// How tall the band is this frame: it glides open and shut.
    pub(crate) fn song_band_height(&self, ctx: &egui::Context) -> f32 {
        let target = if self.song.shown {
            SongLanes::full_height()
        } else {
            0.0
        };
        motion::glide(ctx, Id::new("song band"), target, true)
    }

    /// Shows or hides the lanes.
    pub(crate) fn song_lanes_toggle(&mut self) {
        self.song.shown = !self.song.shown;
        if !self.song.shown {
            self.song.picked = None;
            self.song.focus = false;
            self.song.typing = None;
            self.song.hover = None;
        }
    }

    /// The piece of the ruler corner's well that shows and hides the lanes
    /// (the interface audit's H1): an icon, lit while they are shown, whose
    /// tooltip names them and their key.
    pub(crate) fn song_lanes_button(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let shown = self.song.shown;
        let id = ui.id().with("song lanes button");
        let lit = motion::hover(ui.ctx(), id.with("lit"), shown);
        let (response, ink) = widgets::well::button(ui, rect, id, (lit, theme::Tone::On), true);
        let keys = self.keymap.keys(Command::SongLanesShow);
        let response = widgets::tip(
            response,
            if shown {
                "Hide the tempo and meter lanes"
            } else {
                "Show the tempo and meter lanes"
            },
            keys.as_ref(),
            Some("Change the tempo and time signature along the song"),
        );
        if response.clicked() {
            self.song_lanes_toggle();
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                shown,
                "Tempo and meter lanes",
            )
        });
        let side = theme::size::CAPTION + 3.0;
        let icon = Rect::from_center_size(rect.center(), vec2(side, side));
        ui.painter().extend(crate::icons::shapes(
            crate::icons::Icon::TempoLanes,
            icon,
            ink,
        ));
    }

    /// The lanes, below the bar ruler: their pointer, then their look.
    /// `height` is how much of the band shows (it glides).
    pub(crate) fn song_band(&mut self, ui: &mut egui::Ui, at: &Layout, height: f32) {
        if height < 0.5 {
            return;
        }
        let top = at.ruler.bottom();
        let width = at.ruler.width();
        let header = self.arrangement.header_width();
        let tempo = Rect::from_min_size(
            pos2(at.ruler.left(), top),
            vec2(width, theme::size::SONG_TEMPO),
        );
        let meter = Rect::from_min_size(
            pos2(at.ruler.left(), tempo.bottom()),
            vec2(width, theme::size::SONG_METER),
        );
        let corner = Rect::from_min_size(
            pos2(at.ruler.left() - header, top),
            vec2(header, SongLanes::full_height()),
        );
        let planes = |rect| Plane {
            rect,
            left: at.canvas.left(),
            view: at.view,
        };
        let band = Band {
            corner,
            tempo: planes(tempo),
            meter: planes(meter),
        };
        let open = height >= SongLanes::full_height() - 0.5;
        let project = self.document.project();
        let (tempos, meters) = (project.tempo.clone(), project.meter.clone());
        let scale = match self.song.gesture {
            Some(Gesture::Tempo { scale, .. }) => scale,
            _ => Scale::fit(&tempos),
        };
        self.song.drawn = Some((band, scale));
        self.song.hover = None;
        if open {
            self.tempo_input(ui, &band.tempo, scale, &tempos);
            self.meter_input(ui, &band.meter, &meters);
        }
        let painter = ui.painter().with_clip_rect(Rect::from_min_size(
            pos2(corner.left(), top),
            vec2(header + width, height),
        ));
        let look = Look {
            project: self.document.project(),
            meter: &self.transport.timeline.meter,
            playhead: self.transport.playhead(),
            picked: self.song.picked_on(tempos.len(), meters.len()),
            hover: self.song.hover,
            scale,
        };
        crate::song_lanes::draw(&painter, &band, &look);
        if let Some(hint) = self.song_hint(&band, scale) {
            widgets::draw_hint(&painter, hint.0, &hint.1, at.canvas);
        }
        self.song_open_asked(&band);
        self.song_typing(ui.ctx());
    }

    /// The chip beside the pointer while a change is dragged: where, and
    /// what it will be.
    fn song_hint(&self, band: &Band, scale: Scale) -> Option<(Pos2, String)> {
        let project = self.document.project();
        let map = &self.transport.timeline.meter;
        match self.song.gesture? {
            Gesture::Tempo { index, .. } => {
                let point = project.tempo.get(index)?;
                let at = band.tempo.pos(scale, point);
                let bbt = map.bbt_at(point.at);
                Some((
                    pos2(at.x + 12.0, at.y),
                    format!(
                        "{}.{}  {} BPM",
                        bbt.bar + 1,
                        bbt.beat + 1,
                        bpm_text(point.bpm)
                    ),
                ))
            }
            Gesture::Meter { index, .. } => {
                let point = project.meter.get(index)?;
                let chip = scale::chip(&band.meter, point);
                Some((
                    pos2(chip.right() + 6.0, chip.center().y),
                    format!("Bar {}  {}", map.bbt_at(point.at).bar + 1, point.meter),
                ))
            }
        }
    }

    fn tempo_input(
        &mut self,
        ui: &mut egui::Ui,
        plane: &Plane,
        scale: Scale,
        points: &[TempoPoint],
    ) {
        let response = ui.interact(
            plane.rect,
            ui.id().with("tempo lane"),
            Sense::click_and_drag(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Tempo lane: click to add a tempo change, drag a point to move it, double-click a point to type its tempo",
            )
        });
        let alt = ui.input(|i| i.modifiers.alt);
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        // A drag that began on a change goes on wherever the pointer is.
        if let Some(Gesture::Tempo { index, scale }) = self.song.gesture {
            let down = ui.input(|i| i.pointer.any_down());
            if down
                && response.dragged_by(egui::PointerButton::Primary)
                && let Some(p) = response.interact_pointer_pos()
            {
                let at = self.snapped(plane.ticks_at(p.x), alt);
                let bpm = snap_bpm(plane.bpm_at(scale, p.y), alt);
                self.document
                    .move_tempo_change(index, at, bpm, Some(TEMPO_DRAG));
            }
            if response.drag_stopped() || !down {
                self.song.gesture = None;
            }
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            return;
        }
        let near = pointer.and_then(|p| tempo_hit(plane, scale, points, p));
        let menu_at = response
            .secondary_clicked()
            .then(|| response.interact_pointer_pos())
            .flatten();
        if let Some(p) = menu_at {
            self.song.menu = Some(MenuSpot {
                which: Which::Tempo,
                index: tempo_hit(plane, scale, points, p),
                at: self.snapped(plane.ticks_at(p.x), alt),
            });
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos())
        {
            let index = tempo_hit(plane, scale, points, origin)
                .or_else(|| self.add_tempo_at(plane, scale, origin, alt, Some(TEMPO_DRAG)));
            if let Some(index) = index {
                self.song.picked = Some(Picked {
                    which: Which::Tempo,
                    index,
                });
                self.song.focus = true;
                self.song.gesture = Some(Gesture::Tempo { index, scale });
            }
            return;
        }
        if response.double_clicked()
            && let Some(p) = response.interact_pointer_pos()
            && let Some(index) = tempo_hit(plane, scale, points, p)
        {
            self.type_tempo(plane, index);
            return;
        }
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.song.focus = true;
            let index = tempo_hit(plane, scale, points, p)
                .or_else(|| self.add_tempo_at(plane, scale, p, alt, None));
            self.song.picked = index.map(|index| Picked {
                which: Which::Tempo,
                index,
            });
            return;
        }
        if let Some(index) = near {
            self.song.hover = Some(Picked {
                which: Which::Tempo,
                index,
            });
        }
        let here = pointer.filter(|p| plane.rect.contains(*p));
        if response.hovered() && here.is_some() && !egui::Popup::is_any_open(ui.ctx()) {
            let (cursor, info) = match near.and_then(|i| points.get(i)) {
                Some(point) => (
                    CursorIcon::Grab,
                    Info::new(format!(
                        "Tempo change \u{00B7} {} BPM{}",
                        bpm_text(point.bpm),
                        if point.ramp {
                            " \u{00B7} ramps to the next change"
                        } else {
                            ""
                        }
                    ))
                    .on(Held::NONE, Mouse::Drag, "move")
                    .on(Held::NONE, Mouse::Click, "pick")
                    .on(Held::NONE, Mouse::DoubleClick, "type a tempo")
                    .on(Held::NONE, Mouse::RightClick, "ramp to the next change")
                    .key(Command::Delete, "delete"),
                ),
                None => (
                    CursorIcon::Crosshair,
                    Info::new("Tempo lane")
                        .on(Held::NONE, Mouse::Click, "add a tempo change")
                        .on(Held::NONE, Mouse::Drag, "add one and move it")
                        .on(Held::ALT, Mouse::Click, "off the grid, to a tenth")
                        .key(Command::SongLanesShow, "hide the lanes"),
                ),
            };
            ui.ctx().set_cursor_icon(cursor);
            self.set_info(info);
        }
        widgets::Menu::context(&response).show(|ui| self.song_menu(ui, plane));
    }

    /// Adds a tempo change at `p`; returns its index.
    fn add_tempo_at(
        &mut self,
        plane: &Plane,
        scale: Scale,
        p: Pos2,
        alt: bool,
        merge: Option<&str>,
    ) -> Option<usize> {
        let at = self.snapped(plane.ticks_at(p.x), alt);
        let bpm = snap_bpm(plane.bpm_at(scale, p.y), alt);
        if !self.document.put_tempo_change(at, bpm, merge) {
            self.lanes
                .say("A song holds 4,096 tempo changes at most".to_string());
            return None;
        }
        self.document
            .project()
            .tempo
            .iter()
            .position(|t| t.at == at)
    }

    fn meter_input(
        &mut self,
        ui: &mut egui::Ui,
        plane: &Plane,
        points: &[lumen_core::meter::MeterPoint],
    ) {
        let response = ui.interact(
            plane.rect,
            ui.id().with("meter lane"),
            Sense::click_and_drag(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                "Meter lane: click to add a time signature on a bar line, drag a chip to move it, double-click a chip to type its signature",
            )
        });
        if let Some(Gesture::Meter { index, grab }) = self.song.gesture {
            let down = ui.input(|i| i.pointer.any_down());
            if down
                && response.dragged_by(egui::PointerButton::Primary)
                && let Some(p) = response.interact_pointer_pos()
            {
                let at = plane.ticks_at(p.x - grab);
                self.document.move_meter_change(index, at, Some(METER_DRAG));
            }
            if response.drag_stopped() || !down {
                self.song.gesture = None;
            }
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            return;
        }
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let near = pointer.and_then(|p| meter_hit(plane, points, p));
        if response.secondary_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.song.menu = Some(MenuSpot {
                which: Which::Meter,
                index: meter_hit(plane, points, p),
                at: plane.ticks_at(p.x),
            });
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos())
            && let Some(index) = meter_hit(plane, points, origin)
        {
            self.song.picked = Some(Picked {
                which: Which::Meter,
                index,
            });
            self.song.focus = true;
            // The first change stays at zero: a drag on it only picks it.
            if index > 0 {
                let grab = origin.x - scale::chip(plane, &points[index]).left();
                self.song.gesture = Some(Gesture::Meter { index, grab });
            }
            return;
        }
        if response.double_clicked()
            && let Some(p) = response.interact_pointer_pos()
            && let Some(index) = meter_hit(plane, points, p)
        {
            self.type_meter(plane, Some(index), points[index].at);
            return;
        }
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.song.focus = true;
            match meter_hit(plane, points, p) {
                Some(index) => {
                    self.song.picked = Some(Picked {
                        which: Which::Meter,
                        index,
                    });
                }
                None => {
                    self.song.picked = None;
                    let at = plane.ticks_at(p.x);
                    let at = self.transport.timeline.meter.nearest_bar_start(at);
                    self.type_meter(plane, None, at);
                }
            }
            return;
        }
        if let Some(index) = near {
            self.song.hover = Some(Picked {
                which: Which::Meter,
                index,
            });
        }
        let here = pointer.filter(|p| plane.rect.contains(*p));
        if response.hovered() && here.is_some() && !egui::Popup::is_any_open(ui.ctx()) {
            let (cursor, info) = match near.and_then(|i| points.get(i).map(|p| (i, p))) {
                Some((index, point)) => {
                    let info = Info::new(format!("Time signature change \u{00B7} {}", point.meter))
                        .on(Held::NONE, Mouse::Click, "pick")
                        .on(Held::NONE, Mouse::DoubleClick, "type a signature");
                    if index > 0 {
                        (
                            CursorIcon::Grab,
                            info.on(Held::NONE, Mouse::Drag, "move to another bar line")
                                .key(Command::Delete, "delete"),
                        )
                    } else {
                        (CursorIcon::Default, info)
                    }
                }
                None => (
                    CursorIcon::Crosshair,
                    Info::new("Meter lane")
                        .on(
                            Held::NONE,
                            Mouse::Click,
                            "add a time signature on the nearest bar line",
                        )
                        .key(Command::SongLanesShow, "hide the lanes"),
                ),
            };
            ui.ctx().set_cursor_icon(cursor);
            self.set_info(info);
        }
        widgets::Menu::context(&response).show(|ui| self.song_menu(ui, plane));
    }

    /// Opens the box that types change `index` of the tempo lane's tempo.
    fn type_tempo(&mut self, plane: &Plane, index: usize) {
        let Some(point) = self.document.project().tempo.get(index).copied() else {
            return;
        };
        let scale = Scale::fit(&self.document.project().tempo);
        let at = plane.pos(scale, &point);
        self.song.typing = Some(Typing {
            which: Which::Tempo,
            index: Some(index),
            at: point.at,
            text: bpm_text(point.bpm),
            anchor: pos2(at.x + 10.0, plane.rect.top() + 2.0),
        });
    }

    /// Opens the box that types a time signature: change `index`'s, or one
    /// to add at the bar line `at`.
    fn type_meter(&mut self, plane: &Plane, index: Option<usize>, at: Ticks) {
        let project = self.document.project();
        let current = project.meter_map().meter_at(at);
        let text = match index {
            Some(i) => project.meter.get(i).map_or(current, |p| p.meter),
            None => current,
        }
        .to_string();
        self.song.typing = Some(Typing {
            which: Which::Meter,
            index,
            at,
            text,
            anchor: pos2(plane.x(at) + 4.0, plane.rect.top() + 1.0),
        });
    }

    /// The box a tempo or a time signature is typed in: Enter applies, Esc
    /// cancels, leaving it applies too.
    fn song_typing(&mut self, ctx: &egui::Context) {
        let Some(mut typing) = self.song.typing.take() else {
            return;
        };
        let name = match typing.which {
            Which::Tempo => "Tempo of the change",
            Which::Meter => "Time signature of the change",
        };
        let palette = theme::p();
        let mut done = None;
        egui::Area::new(Id::new("song typing"))
            .order(egui::Order::Foreground)
            .fixed_pos(typing.anchor)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(palette.overlay)
                    .stroke(egui::Stroke::new(
                        theme::stroke::HAIRLINE,
                        palette.border_strong,
                    ))
                    .corner_radius(theme::radius::MEDIUM)
                    .inner_margin(theme::space::XS)
                    .show(ui, |ui| {
                        let edit = ui.add_sized(
                            vec2(theme::size::SONG_TYPE, theme::size::CONTROL),
                            egui::TextEdit::singleline(&mut typing.text)
                                .font(egui::FontId::monospace(theme::size::BODY))
                                .horizontal_align(egui::Align::Center),
                        );
                        edit.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name)
                        });
                        if !edit.has_focus() && !edit.lost_focus() {
                            edit.request_focus();
                            edit.scroll_to_me(None);
                        }
                        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        if cancel {
                            done = Some(false);
                        } else if edit.lost_focus() {
                            done = Some(true);
                        }
                    });
            });
        match done {
            None => self.song.typing = Some(typing),
            Some(false) => {}
            Some(true) => self.song_commit(&typing),
        }
    }

    /// Applies what was typed, when it is a tempo or signature Lumen has.
    fn song_commit(&mut self, typing: &Typing) {
        match typing.which {
            Which::Tempo => {
                let (Some(index), Some(bpm)) = (typing.index, parse_bpm(&typing.text)) else {
                    return;
                };
                if let Some(point) = self.document.project().tempo.get(index).copied() {
                    self.document.move_tempo_change(index, point.at, bpm, None);
                }
            }
            Which::Meter => {
                let unit = self
                    .document
                    .project()
                    .meter_map()
                    .meter_at(typing.at)
                    .denominator;
                let Some((beats, unit)) = widgets::parse_signature(
                    &typing.text,
                    unit,
                    crate::document::MAX_BEATS_PER_BAR,
                ) else {
                    return;
                };
                match typing.index {
                    Some(index) => self.document.set_meter_change(index, beats, unit, None),
                    None => {
                        if !self.document.put_meter_change(typing.at, beats, unit) {
                            self.lanes.say(
                                "A song holds 4,096 time signature changes at most".to_string(),
                            );
                        }
                    }
                }
            }
        }
    }

    /// The menu of a right-click on a lane.
    fn song_menu(&mut self, ui: &mut egui::Ui, plane: &Plane) {
        let Some(spot) = self.song.menu else {
            return;
        };
        match (spot.which, spot.index) {
            (Which::Tempo, Some(index)) => {
                if widgets::menu_item(ui, "Type a tempo\u{2026}", None, true).clicked() {
                    self.type_tempo(plane, index);
                }
                let tempos = &self.document.project().tempo;
                let ramps = tempos.get(index).is_some_and(|p| p.ramp);
                let label = if ramps {
                    "Hold until the next change"
                } else {
                    "Ramp to the next change"
                };
                if widgets::menu_item(ui, label, None, index + 1 < tempos.len()).clicked() {
                    self.document.set_tempo_ramp(index, !ramps);
                }
                let keys = self.keymap.keys(Command::Delete);
                if widgets::menu_item(ui, "Delete tempo change", keys.as_ref(), index > 0).clicked()
                {
                    self.document.remove_tempo_change(index);
                    self.song.picked = None;
                }
            }
            (Which::Tempo, None) => {
                if widgets::menu_item(ui, "Add a tempo change here", None, true).clicked() {
                    let bpm = tempo_in(&self.document.project().tempo, spot.at);
                    self.document.put_tempo_change(spot.at, bpm, None);
                    if let Some(index) = self
                        .document
                        .project()
                        .tempo
                        .iter()
                        .position(|t| t.at == spot.at)
                    {
                        self.type_tempo(plane, index);
                    }
                }
            }
            (Which::Meter, Some(index)) => {
                if widgets::menu_item(ui, "Type a time signature\u{2026}", None, true).clicked() {
                    self.type_meter(plane, Some(index), spot.at);
                }
                let keys = self.keymap.keys(Command::Delete);
                if widgets::menu_item(ui, "Delete time signature change", keys.as_ref(), index > 0)
                    .clicked()
                {
                    self.document.remove_meter_change(index);
                    self.song.picked = None;
                }
            }
            (Which::Meter, None) => {
                if widgets::menu_item(ui, "Add a time signature here\u{2026}", None, true).clicked()
                {
                    let at = self.transport.timeline.meter.nearest_bar_start(spot.at);
                    self.type_meter(plane, None, at);
                }
            }
        }
    }

    /// Whether Delete means a picked change: the lanes have the keyboard and
    /// a change other than the first of its lane is picked.
    pub(crate) fn song_keys_picked(&self) -> bool {
        self.song.shown
            && self.song.focus
            && !self.note_keys()
            && !self.chain.live
            && self.song.picked.is_some_and(|p| p.index > 0)
            && self
                .song
                .picked_on(
                    self.document.project().tempo.len(),
                    self.document.project().meter.len(),
                )
                .is_some()
    }

    /// The picked tempo change, when it has a next change to ramp to.
    pub(crate) fn song_ramp_target(&self) -> Option<usize> {
        let tempos = &self.document.project().tempo;
        self.song
            .picked
            .filter(|p| p.which == Which::Tempo && p.index + 1 < tempos.len())
            .map(|p| p.index)
    }

    /// Makes the picked tempo change ramp to the next one, or hold again.
    pub(crate) fn song_toggle_ramp(&mut self) {
        if let Some(index) = self.song_ramp_target() {
            let ramp = !self.document.project().tempo[index].ramp;
            self.document.set_tempo_ramp(index, ramp);
        }
    }

    /// Deletes the picked change.
    pub(crate) fn song_delete(&mut self) {
        let Some(picked) = self.song.picked.take() else {
            return;
        };
        match picked.which {
            Which::Tempo => self.document.remove_tempo_change(picked.index),
            Which::Meter => self.document.remove_meter_change(picked.index),
        }
    }

    /// Adds a tempo change at the playhead, at the tempo playing there, and
    /// asks for the new tempo (a command, so it works from the keyboard).
    pub(crate) fn song_add_tempo_at_playhead(&mut self) {
        if !self.song.shown {
            self.song.shown = true;
        }
        let at = self.transport.playhead();
        let bpm = tempo_in(&self.document.project().tempo, at);
        if !self.document.put_tempo_change(at, bpm, None) {
            self.lanes
                .say("A song holds 4,096 tempo changes at most".to_string());
            return;
        }
        if let Some(index) = self
            .document
            .project()
            .tempo
            .iter()
            .position(|t| t.at == at)
        {
            self.song.picked = Some(Picked {
                which: Which::Tempo,
                index,
            });
            self.song.focus = true;
            self.song.ask = Some((Which::Tempo, Some(index), at));
        }
    }

    /// Asks for a time signature on the bar line nearest the playhead.
    pub(crate) fn song_add_meter_at_playhead(&mut self) {
        self.song.shown = true;
        let at = self
            .transport
            .timeline
            .meter
            .nearest_bar_start(self.transport.playhead());
        self.song.ask = Some((Which::Meter, None, at));
    }

    /// Opens a box a command asked for, once the lanes have a place on
    /// screen to put it.
    pub(crate) fn song_open_asked(&mut self, band: &Band) {
        let Some((which, index, at)) = self.song.ask.take() else {
            return;
        };
        match which {
            Which::Tempo => {
                if let Some(index) = index {
                    self.type_tempo(&band.tempo, index);
                }
            }
            Which::Meter => self.type_meter(&band.meter, index, at),
        }
    }
}
