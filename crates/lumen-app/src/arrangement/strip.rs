//! One channel strip in the mixer (M4 slice 5, the owner's S1 console,
//! ADR-0040), from the top: the rails of the groups it is in, its name,
//! where it plays to, polarity and input trim, its sends (the owner's O2:
//! two knobs, then "+N ▾" for them all), pan, the fader beside its meter,
//! the volume typed in and the peak held, then M, S and arm.
//!
//! A short panel folds sections away (the owner's B7): sends first, then
//! trim and polarity, then the output; name, pan, fader, meter and the
//! buttons always stay. Strip controls are named "… in the mixer", so
//! each has a name of its own beside its header's twin.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Id as EguiId, Rangef, Rect, Sense, Stroke, Ui, pos2,
    vec2,
};
use eframe::emath::GuiRounding as _;
use lumen_project::model::{Id, Output, Project, Track, TrackKind};
use lumen_project::routing::{self as rules, Destination};
use lumen_project::solo::Heard;

use super::sends::{Knob, returns};
use super::{Arrangement, color_of, headers, nesting, pinned};
use crate::commands::Keymap;
use crate::document::Document;
use crate::meters::FLOOR_DB;
use crate::widgets::{self, SliderKind, fader, meter};
use crate::{fonts, motion, theme};

/// A strip's width (the owner's S1).
pub(super) const WIDTH: f32 = 76.0;
/// Side inset inside a strip.
const INSET: f32 = 6.0;
/// The height of a control row, and the gap between rows.
const ROW: f32 = 18.0;
const GAP: f32 = 4.0;
/// The group rails along the top: 3 pt for each group the track is in
/// (and for a group itself), at most four.
const RAIL: f32 = 3.0;
const TOP: f32 = 4.0 * RAIL + 2.0;
const NAME: f32 = 20.0;
/// Two send knobs, then the "+N ▾" row (the owner's O2).
const KNOB: f32 = 22.0;
const SENDS: f32 = KNOB + GAP + 16.0;
/// The fader's travel is at least this tall; the cap and the clip light
/// take room above and below it.
const TRAVEL_MIN: f32 = 64.0;
const TRAVEL_PAD: f32 = 12.0;
const BUTTONS: f32 = 20.0;
const BOTTOM: f32 = 6.0;
/// dB marks on the scale between fader and meter.
const MARKS: [f32; 6] = [6.0, 0.0, -6.0, -12.0, -24.0, -48.0];

/// Which optional sections a strip shows, for the panel's height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Sections {
    pub output: bool,
    pub trim: bool,
    pub sends: bool,
}

impl Sections {
    /// The sections that fit `height`, folding sends away first, then trim
    /// and polarity, then the output (the owner's B7).
    pub(super) fn fitting(height: f32) -> Self {
        let mut s = Sections {
            output: true,
            trim: true,
            sends: true,
        };
        for fold in [
            |s: &mut Sections| s.sends = false,
            |s: &mut Sections| s.trim = false,
            |s: &mut Sections| s.output = false,
        ] {
            if s.travel(height) >= TRAVEL_MIN {
                break;
            }
            fold(&mut s);
        }
        s
    }

    /// The height everything but the fader takes.
    fn fixed(self) -> f32 {
        let optional = [(self.output, ROW), (self.trim, ROW), (self.sends, SENDS)];
        TOP + NAME
            + optional
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, h)| GAP + h)
                .sum::<f32>()
            + GAP
            + ROW // pan
            + GAP
            + 2.0 * TRAVEL_PAD
            + GAP
            + ROW // volume and peak
            + GAP
            + BUTTONS
            + BOTTOM
    }

    /// The fader's travel in a strip `height` tall.
    pub(super) fn travel(self, height: f32) -> f32 {
        height - self.fixed()
    }

    /// The shortest strip: everything folded and the shortest travel.
    pub(super) fn min_height() -> f32 {
        Sections {
            output: false,
            trim: false,
            sends: false,
        }
        .fixed()
            + TRAVEL_MIN
    }
}

/// What a strip needs from the panel besides its track.
pub(super) struct StripLook<'a> {
    pub sections: Sections,
    pub heard: &'a Heard,
    pub keymap: &'a Keymap,
}

/// Where `track` plays to, in words: a group's or return's name, "Master",
/// or a pair of outputs ("1/2").
fn destination_text(project: &Project, track: &Track) -> String {
    match rules::destination(project, track) {
        Destination::Track(id) if id == project.master.id => "Master".into(),
        Destination::Track(id) => project
            .tracks
            .iter()
            .find(|t| t.id == id)
            .map_or_else(|| "Master".into(), |t| t.name.clone()),
        Destination::Device(pair) => format!("{}/{}", 2 * pair + 1, 2 * pair + 2),
    }
}

/// What a screen reader says for a strip: "Mixer strip: Kick, Audio track,
/// muted" (the header says "Track Kick, …").
fn strip_description(track: &Track) -> String {
    let header = headers::track_description(track);
    let rest = header.strip_prefix("Track ").unwrap_or(&header);
    format!("Mixer strip: {rest}")
}

/// Whether sending track `id` to `output` would feed the audio back into
/// itself: such choices are never offered (risk R4).
fn would_loop(project: &Project, id: Id, output: Output) -> bool {
    let mut trial = project.clone();
    if let Some(track) = trial.tracks.iter_mut().find(|t| t.id == id) {
        std::sync::Arc::make_mut(track).output = output;
    }
    rules::has_loop(&trial)
}

impl Arrangement {
    /// Draws track `index`'s strip (the master when `index` is `None`) in
    /// `rect` and applies what the user does with it.
    pub(super) fn strip(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        index: Option<usize>,
        rect: Rect,
        look: &StripLook<'_>,
    ) {
        let project = doc.project();
        let track = match index {
            Some(i) => match project.tracks.get(i) {
                Some(t) => t.clone(),
                None => return,
            },
            None => project.master.clone(),
        };
        let palette = *theme::p();
        let id = EguiId::new(("strip", track.id));
        let selected = self.selected.contains(&track.id);
        let controls = theme::controls(selected);
        let hears = look.heard.of(track.id);
        let heard = hears.output || track.kind == TrackKind::Master;
        let painter = ui.painter_at(rect);

        // Background: click to select, drag to move (the owner's B8),
        // right-click for the track's menu.
        let background = ui.interact(rect, id, Sense::click_and_drag());
        background.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                selected,
                strip_description(&track),
            )
        });
        if background.clicked() && track.kind != TrackKind::Master {
            let modifiers = ui.input(|i| i.modifiers);
            self.click_select(doc.project(), track.id, modifiers);
        }
        if background.drag_started() && track.kind != TrackKind::Master {
            self.start_drag(doc.project(), track.id, true);
        }
        if track.kind != TrackKind::Master {
            background.context_menu(|ui| {
                if !self.selected.contains(&track.id) {
                    self.selected = vec![track.id];
                }
                let renaming = self.renaming.is_some();
                self.track_menu(ui, doc, track.id, look.keymap);
                if !renaming && self.renaming.is_some() {
                    self.renaming_in_strip = true;
                }
            });
        }
        let fill = if selected {
            palette.selected_row
        } else if matches!(track.kind, TrackKind::Return | TrackKind::Master) {
            palette.raised
        } else {
            palette.header
        };
        painter.rect_filled(rect, 0.0, fill);

        // The rails of the groups it is in (outermost on top), and its own
        // when it is a group: the arrangement's H1 rails, laid across.
        let project = doc.project();
        let mut rails: Vec<Color32> = lumen_project::groups::ancestors(&project.tracks, track.id)
            .filter_map(|g| project.tracks.iter().find(|t| t.id == g))
            .map(|g| color_of(g.color))
            .collect();
        rails.reverse();
        if track.kind == TrackKind::Group {
            rails.push(color_of(track.color));
        }
        for (n, color) in rails.iter().take(4).enumerate() {
            let y = rect.top() + n as f32 * RAIL;
            painter.rect_filled(
                Rect::from_x_y_ranges(rect.x_range(), y..=y + RAIL),
                0.0,
                *color,
            );
        }

        let x = rect.x_range().shrink(INSET);
        let mut y = rect.top() + TOP;
        self.strip_name(ui, doc, &track, (x, y), (id, heard));
        y += NAME + GAP;

        if look.sections.output {
            self.output_button(ui, doc, &track, Rect::from_x_y_ranges(x, y..=y + ROW), id);
            y += ROW + GAP;
        }
        if look.sections.trim {
            let row = Rect::from_x_y_ranges(x, y..=y + ROW);
            if track.kind == TrackKind::Master {
                // The master has no trim here: its Solo/Cue switch instead.
                let mid = row.center().y;
                let at = Rect::from_x_y_ranges(
                    rect.left()..=rect.center().x + 23.0 + 10.0,
                    row.y_range(),
                );
                self.cue_switch(ui, at, mid, " in the mixer");
            } else {
                self.trim_row(ui, doc, &track, row, (id, controls));
            }
            y += ROW + GAP;
        }
        if look.sections.sends {
            if super::sends::has_sends(track.kind) {
                self.strip_sends(ui, doc, &track, look.heard, (x, y), selected);
            }
            y += SENDS + GAP;
        }

        // Pan.
        let pan_rect = Rect::from_x_y_ranges(x, y..=y + ROW);
        let pan = widgets::value_slider(
            ui,
            pan_rect,
            id.with("pan"),
            track.pan,
            SliderKind::Pan,
            &format!("{} pan in the mixer", track.name),
            controls,
        );
        if pan != track.pan {
            doc.set_pan(track.id, pan);
        }
        y += ROW + GAP;

        // The fader, its scale and the meter (the owner's S1).
        let travel = Rangef::new(
            y + TRAVEL_PAD,
            y + TRAVEL_PAD + look.sections.travel(rect.height()),
        );
        let fader_x = rect.left() + INSET + fader::CAP.x / 2.0 - 2.0;
        if let Some(db) = fader::fader(
            ui,
            (fader_x, travel),
            id.with("fader"),
            track.gain_db,
            &format!("{} fader in the mixer", track.name),
            controls,
        ) {
            doc.set_volume(track.id, db);
        }
        let meter_rect =
            Rect::from_x_y_ranges(rect.right() - INSET - 12.0..=rect.right() - INSET, travel);
        meter::scale(&painter, meter_rect.left() - 4.0, travel, &MARKS);
        let reading = self.meters.reading(track.id.0);
        let base = if self.track_colour_meters {
            theme::wave(color_of(track.color))
        } else {
            palette.accent
        };
        meter::vertical(&painter, meter_rect, 2.0, &reading, base, heard);
        // The clip light, over the meter: lit until clicked.
        let clip = Rect::from_x_y_ranges(meter_rect.x_range(), travel.min - 9.0..=travel.min - 4.0);
        painter.rect_filled(
            clip,
            1.5,
            if reading.clipped {
                palette.record
            } else {
                controls.fill
            },
        );
        self.clear_clip_on_click(ui, clip.expand(3.0), &track, id.with("clip"));
        y = travel.max + TRAVEL_PAD + GAP;

        // The volume typed in, and the peak held.
        let half = (x.span() - GAP) / 2.0;
        let volume = Rect::from_x_y_ranges(x.min..=x.min + half, y..=y + ROW);
        let db = widgets::value_field(
            ui,
            volume,
            id.with("volume"),
            track.gain_db,
            SliderKind::Volume,
            &format!("{} volume in the mixer", track.name),
        );
        if db != track.gain_db {
            doc.set_volume(track.id, db);
        }
        let peak = Rect::from_x_y_ranges(x.max - half..=x.max, y..=y + ROW);
        let hold = reading.hold_db();
        painter.rect_filled(
            peak,
            theme::radius::SMALL,
            if reading.clipped {
                palette.record
            } else {
                palette.raised
            },
        );
        painter.text(
            peak.center(),
            Align2::CENTER_CENTER,
            if hold > FLOOR_DB {
                widgets::db_text(hold).replace(" dB", "")
            } else {
                "\u{2014}".into()
            },
            FontId::monospace(theme::size::CAPTION),
            if reading.clipped {
                theme::text_on(palette.record)
            } else {
                palette.text_secondary
            },
        );
        self.clear_clip_on_click(ui, peak, &track, id.with("peak"));
        y += ROW + GAP;

        // M, S and arm, centred.
        let ppp = ui.ctx().pixels_per_point();
        let size = vec2(BUTTONS, BUTTONS);
        let arms = matches!(
            track.kind,
            TrackKind::Audio | TrackKind::Midi | TrackKind::Group
        );
        let count = if track.kind == TrackKind::Master {
            1.0
        } else if arms {
            3.0
        } else {
            2.0
        };
        let gap = 5.0;
        let mut bx = rect.center().x - (count * BUTTONS + (count - 1.0) * gap) / 2.0;
        let mut next = || {
            let r = Rect::from_min_size(pos2(bx, y), size).round_to_pixels(ppp);
            bx += BUTTONS + gap;
            r
        };
        let mute = next();
        let solo = if track.kind == TrackKind::Master {
            Rect::NOTHING
        } else {
            next()
        };
        if track.kind == TrackKind::Master {
            self.master_mute(ui, doc, &track, mute, (id, controls));
        } else {
            self.mute_solo(
                ui,
                doc,
                &track,
                hears,
                (mute, solo),
                (id, controls, " in the mixer"),
            );
        }
        if arms && let Some(i) = index {
            let arm = nesting::arm_state(doc.project(), i);
            let name = format!("{} in the mixer", track.name);
            if headers::arm_button(ui, next(), id, &name, arm, controls) {
                doc.set_armed(&self.targets(track.id), !arm.on());
            }
        }

        painter.vline(
            rect.right() - 0.5,
            rect.y_range(),
            Stroke::new(1.0, palette.border),
        );
        // Strips being dragged are dimmed, as their headers are.
        if self.is_dragged(track.id) {
            painter.rect_filled(rect, 0.0, palette.header.gamma_multiply(0.6));
        }
    }

    /// The colour chip, a group's fold arrow, a return's letter, and the
    /// name (double-click renames it here).
    fn strip_name(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        (x, y): (Rangef, f32),
        (id, heard): (EguiId, bool),
    ) {
        let palette = *theme::p();
        let mid = y + NAME / 2.0;
        let painter = ui.painter();
        painter.rect_filled(
            Rect::from_x_y_ranges(x.min..=x.min + 3.0, mid - 7.0..=mid + 7.0),
            0.0,
            color_of(track.color),
        );
        let mut left = x.min + 7.0;
        if track.kind == TrackKind::Group {
            let collapsed = self.collapsed.contains(&track.id);
            let arrow = Rect::from_center_size(pos2(left + 5.0, mid), vec2(12.0, 18.0));
            let fold = ui
                .interact(arrow, id.with("fold"), Sense::click())
                .on_hover_text("Hide or show the group's tracks, here and in the arrangement");
            fold.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    true,
                    collapsed,
                    format!("Fold group {} in the mixer", track.name),
                )
            });
            if fold.clicked() {
                self.toggle_collapsed(doc.project(), track.id);
            }
            let open = motion::glide(
                ui.ctx(),
                id.with("fold glide"),
                if collapsed { 0.0 } else { 1.0 },
                true,
            );
            widgets::draw_arrow(
                ui.painter(),
                arrow.center(),
                open,
                if fold.hovered() {
                    palette.text
                } else {
                    palette.text_secondary
                },
            );
            left += 12.0;
        }
        if track.kind == TrackKind::Return
            && let Some(letter) = pinned::letter_of(doc.project(), track.id)
        {
            let badge = Rect::from_min_size(
                pos2(left, mid - 7.0),
                vec2(14.0_f32.max(8.0 * letter.len() as f32 + 6.0), 14.0),
            )
            .round_to_pixels(ui.ctx().pixels_per_point());
            let fill = color_of(track.color);
            ui.painter().rect_filled(badge, theme::radius::SMALL, fill);
            ui.painter().text(
                badge.center(),
                Align2::CENTER_CENTER,
                &letter,
                fonts::medium(theme::size::CAPTION),
                theme::text_on(fill),
            );
            left = badge.right() + 4.0;
        }
        let name_rect = Rect::from_x_y_ranges(left..=x.max, mid - 10.0..=mid + 10.0);
        if track.kind == TrackKind::Master {
            ui.painter().text(
                name_rect.left_center(),
                Align2::LEFT_CENTER,
                &track.name,
                fonts::medium(theme::size::BODY),
                palette.text,
            );
            return;
        }
        if let Some(name) = self.name(ui, doc, track, name_rect, (heard, None), (id, true)) {
            name.on_hover_text(&track.name);
        }
    }

    /// Where the track plays to; a click opens the choices.
    fn output_button(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        row: Rect,
        id: EguiId,
    ) {
        let palette = *theme::p();
        let text = destination_text(doc.project(), track);
        let response = ui
            .interact(row, id.with("output"), Sense::click())
            .on_hover_text("Where this track plays to");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::ComboBox,
                true,
                format!("{} output in the mixer: {text}", track.name),
            )
        });
        let hover = motion::hover(ui.ctx(), id.with("output hover"), response.hovered());
        let painter = ui.painter();
        painter.rect_filled(
            row,
            theme::radius::SMALL,
            palette.control.lerp_to_gamma(palette.control_hover, hover),
        );
        let font = FontId::proportional(theme::size::CAPTION);
        let shown = if painter
            .layout_no_wrap(text.clone(), font.clone(), Color32::WHITE)
            .size()
            .x
            > row.width() - 20.0
        {
            super::sends::shorten(painter, &text, font.clone(), row.width() - 20.0)
        } else {
            text
        };
        painter.text(
            pos2(row.left() + 5.0, row.center().y),
            Align2::LEFT_CENTER,
            shown,
            font,
            palette.text_secondary,
        );
        widgets::draw_arrow(
            painter,
            pos2(row.right() - 8.0, row.center().y),
            1.0,
            palette.text_hint,
        );
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(160.0);
                self.output_menu(ui, doc, track);
            });
    }

    /// The choices of where `track` plays to: inside Lumen, the master and
    /// the groups that would not feed back; then the device's output pairs
    /// (the owner's B6), the main pair marked, the cue pair greyed.
    fn output_menu(&mut self, ui: &mut Ui, doc: &mut Document, track: &Track) {
        let palette = *theme::p();
        let project = doc.project().clone();
        let current = rules::destination(&project, track);
        let master = track.kind == TrackKind::Master;
        let heading = |ui: &mut Ui, text: &str| {
            ui.label(egui::RichText::new(text).small().color(palette.text_hint));
        };
        let mut picked = None;
        let mut choose = |ui: &mut Ui, label: &str, on: bool, output: Output, tip: &str| {
            let item = ui.selectable_label(on, label);
            let item = if tip.is_empty() {
                item
            } else {
                item.on_hover_text(tip)
            };
            if item.clicked() {
                picked = Some(output);
                ui.close();
            }
        };
        if !master {
            heading(ui, "Inside Lumen");
            choose(
                ui,
                "Master",
                current == Destination::Track(project.master.id),
                Output::Master,
                "",
            );
            for target in project.tracks.iter().filter(|t| {
                t.kind == TrackKind::Group
                    && t.id != track.id
                    && !would_loop(&project, track.id, Output::Track(t.id))
            }) {
                choose(
                    ui,
                    &target.name,
                    current == Destination::Track(target.id),
                    Output::Track(target.id),
                    "",
                );
            }
            ui.separator();
        }
        heading(ui, "Audio outputs");
        // The device's outputs are looked up while a menu wants them.
        self.outputs.wanted = true;
        if let Some(status) = &self.outputs.status {
            ui.label(egui::RichText::new(status).small().color(palette.text_hint));
        }
        let skips = if master { "" } else { "Skips the master" };
        for (pair, names) in self.outputs.pairs.clone() {
            let single = !names.contains('\u{00B7}');
            let label = if single {
                format!("{}", 2 * pair + 1)
            } else {
                format!("{}/{}", 2 * pair + 1, 2 * pair + 2)
            };
            let main = self.outputs.main == Some(pair);
            if self.outputs.cue == Some(pair) {
                ui.add_enabled(
                    false,
                    egui::Button::selectable(false, format!("{label} Cue")),
                )
                .on_disabled_hover_text(
                    "Used for cue: choose another Cue output in Audio settings to use it here",
                );
                continue;
            }
            let on = match track.output {
                Output::Device(p) => p == pair,
                Output::Auto => master && main,
                _ => false,
            };
            let text = if main { format!("{label} Main") } else { label };
            // The master on the main pair follows it, whichever pair the
            // audio settings make main.
            let output = if master && main {
                Output::Auto
            } else {
                Output::Device(pair)
            };
            let tip = if skips.is_empty() {
                names
            } else {
                format!("{names} \u{00B7} {skips}")
            };
            choose(ui, &text, on, output, &tip);
        }
        if let Some(output) = picked
            && let Err(refused) = doc.set_output(track.id, output)
        {
            self.note = Some(refused.reason());
        }
    }

    /// Polarity (Ø) and input trim.
    fn trim_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        row: Rect,
        (id, controls): (EguiId, theme::Controls),
    ) {
        let palette = *theme::p();
        let flip = Rect::from_min_size(row.min, vec2(ROW, ROW));
        let response = ui
            .interact(flip, id.with("invert"), Sense::click())
            .on_hover_text("Invert polarity (Ø)");
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                true,
                track.invert,
                format!("Invert polarity of {} in the mixer", track.name),
            )
        });
        if response.clicked() {
            doc.set_invert(&self.targets(track.id), !track.invert);
        }
        let hover = motion::hover(ui.ctx(), id.with("invert hover"), response.hovered());
        let fill = if track.invert {
            palette.accent
        } else {
            controls.fill.lerp_to_gamma(controls.hover, hover)
        };
        ui.painter().rect_filled(flip, theme::radius::SMALL, fill);
        ui.painter().text(
            flip.center(),
            Align2::CENTER_CENTER,
            "\u{00D8}",
            fonts::medium(theme::size::CAPTION),
            if track.invert {
                theme::text_on(fill)
            } else {
                controls.text
            },
        );
        let trim = Rect::from_min_max(pos2(flip.right() + GAP, row.top()), row.max);
        let db = widgets::value_slider(
            ui,
            trim,
            id.with("trim"),
            track.trim_db,
            SliderKind::Trim,
            &format!("{} trim in the mixer", track.name),
            controls,
        );
        if db != track.trim_db {
            doc.set_trim(track.id, db);
        }
    }

    /// The owner's O2: the first two returns' knobs, then "+N ▾" listing
    /// every send.
    fn strip_sends(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        heard: &Heard,
        (x, y): (Rangef, f32),
        selected: bool,
    ) {
        let returns = returns(doc.project());
        if returns.is_empty() {
            return;
        }
        let ppp = ui.ctx().pixels_per_point();
        let shown = returns.len().min(2);
        let pitch = x.span() / 2.0;
        for (n, to) in returns.iter().take(shown).enumerate() {
            let cx = x.min + pitch * (n as f32 + 0.5);
            let rect = Rect::from_center_size(pos2(cx, y + KNOB / 2.0), vec2(KNOB, KNOB))
                .round_to_pixels(ppp);
            let name = format!(
                "Send {} to {} {} in the mixer",
                track.name, to.letter, to.name
            );
            self.send_control(
                ui,
                doc,
                heard,
                Knob {
                    rect,
                    track,
                    to,
                    selected,
                    name,
                    place: "strip",
                },
            );
        }
        if returns.len() > shown {
            let at = pos2(x.min, y + KNOB + GAP + 8.0);
            self.more_sends(ui, doc, track, &returns, (shown, at), (selected, true));
        }
    }

    /// The master's mute.
    fn master_mute(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        master: &Track,
        rect: Rect,
        (id, controls): (EguiId, theme::Controls),
    ) {
        let palette = *theme::p();
        let response = ui
            .interact(rect, id.with("mute"), Sense::click())
            .on_hover_text("Mute the master: silences the main output");
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                true,
                master.mute,
                "Mute Master in the mixer",
            )
        });
        if response.clicked() {
            doc.set_mute(&[master.id], !master.mute);
        }
        let hover = motion::hover(ui.ctx(), id.with("mute hover"), response.hovered());
        let fill = if master.mute {
            palette.start_marker
        } else {
            controls.fill.lerp_to_gamma(controls.hover, hover)
        };
        ui.painter().rect_filled(rect, theme::radius::SMALL, fill);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "M",
            fonts::medium(theme::size::CAPTION),
            if master.mute {
                theme::text_on(fill)
            } else {
                controls.text
            },
        );
    }

    /// A click on a clip light or a held peak clears it; Alt+click clears
    /// every one.
    fn clear_clip_on_click(&mut self, ui: &mut Ui, area: Rect, track: &Track, id: EguiId) {
        let response = ui
            .interact(area, id, Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        if response.hovered() {
            use crate::app::info::{Held, Info, Mouse};
            self.info = Some(
                Info::new(format!("{} peak", track.name))
                    .on(Held::NONE, Mouse::Click, "clear its peak and clip light")
                    .on(Held::ALT, Mouse::Click, "clear every one"),
            );
        }
        let name = format!("Clear the peak of {} in the mixer", track.name);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        if response.clicked() {
            if ui.input(|i| i.modifiers.alt) {
                self.meters.clear_all_clips();
            } else {
                self.meters.clear_clip(track.id.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_panel_folds_sends_then_trim_then_the_output() {
        let all = Sections::fitting(1_000.0);
        assert!(all.output && all.trim && all.sends);
        assert!(all.travel(1_000.0) > TRAVEL_MIN);
        // Just short of room for the sends: they go first.
        let full = Sections {
            output: true,
            trim: true,
            sends: true,
        };
        let h = full.fixed() + TRAVEL_MIN - 1.0;
        let s = Sections::fitting(h);
        assert!(!s.sends && s.trim && s.output, "{s:?}");
        // Shorter still: trim, then the output.
        let s = Sections::fitting(h - SENDS - GAP - ROW);
        assert!(!s.sends && !s.trim && s.output, "{s:?}");
        let s = Sections::fitting(Sections::min_height());
        assert!(!s.sends && !s.trim && !s.output);
        assert_eq!(s.travel(Sections::min_height()), TRAVEL_MIN);
    }
}
