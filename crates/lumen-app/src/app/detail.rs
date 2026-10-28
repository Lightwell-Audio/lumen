//! The detail panel's tabs and what each shows (M5 slice 3c extracted this
//! from the frame loop). The row names its subject at the left: the track
//! whose header or clip was clicked last (DP1). Devices is that track's
//! chain (M6 slice 2d); Clip appears for the opened clip, an audio clip's
//! properties beside its wave or a MIDI clip's notes; Mixer, at the right,
//! is the channel strips.

use std::collections::BTreeSet;

use eframe::egui::{self, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use lumen_core::meter::MeterMap;
use lumen_project::model::{ClipContent, Id};
use lumen_project::routing::all_tracks;

use super::panes::{self, Pane};
use crate::arrangement::{self, waves};
use crate::media::State;
use crate::{App, fonts, theme, widgets};

/// Which tab of the detail panel is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DetailTab {
    Devices,
    Clip,
    #[default]
    Mixer,
}

/// What the tab row follows between frames (not saved).
#[derive(Debug, Default)]
pub(crate) struct DetailState {
    /// The track of the header or clip clicked last: the clip selection is
    /// a set, so which one came last is kept here.
    anchor: Option<Id>,
    /// The selections last frame, to tell what was just clicked.
    seen_tracks: Vec<Id>,
    seen_clips: BTreeSet<Id>,
    /// The audio clip opened in the Clip tab; an opened MIDI clip is the
    /// note editor's.
    opened_audio: Option<Id>,
    /// The opened clip went while the panel had the keyboard: the Devices
    /// tab takes it next frame.
    focus_devices: bool,
}

/// The properties column beside an opened audio clip's wave: the clip
/// card's rows, with its padding.
const PROPS_WIDTH: f32 = super::clip_card::CARD_WIDTH + 2.0 * theme::space::M;
/// The bar numbers over the wave.
const RULER: f32 = 18.0;
/// The least room between two bar numbers; past it, every second, fourth…
const BAR_ROOM: f32 = 32.0;
/// The widest the subject's name is drawn before it truncates.
const SUBJECT_WIDTH: f32 = 160.0;
/// The subject's colour bar.
const SUBJECT_BAR: f32 = 3.0;

impl App {
    /// The track holding clip `clip`.
    pub(crate) fn track_of_clip(&self, clip: Id) -> Option<Id> {
        all_tracks(self.document.project())
            .find(|t| t.clips.iter().any(|c| c.id == clip))
            .map(|t| t.id)
    }

    /// The clip open in the Clip tab, audio or MIDI.
    pub(crate) fn opened_clip(&self) -> Option<Id> {
        self.note_editor.clip.or(self.detail.opened_audio)
    }

    /// The opened clip is an audio clip.
    pub(crate) fn opened_audio(&self) -> Option<Id> {
        self.detail.opened_audio
    }

    /// The track the panel is about (DP1): the one whose header or clip
    /// was clicked last while anything is selected, else the opened clip's.
    pub(crate) fn subject(&self) -> Option<Id> {
        let project = self.document.project();
        let exists = |id: &Id| all_tracks(project).any(|t| t.id == *id);
        let arr = &self.arrangement;
        if !arr.selected.is_empty() || !arr.clips.is_empty() {
            let found = self
                .detail
                .anchor
                .filter(exists)
                .or_else(|| arr.selected.iter().rev().copied().find(exists))
                .or_else(|| {
                    self.first_selected_clip()
                        .and_then(|c| self.track_of_clip(c))
                });
            if found.is_some() {
                return found;
            }
        }
        self.opened_clip().and_then(|c| self.track_of_clip(c))
    }

    /// Opens clip `clip` in the Clip tab (double-click or Enter): a MIDI
    /// clip in the note editor, an audio clip's properties beside its wave.
    /// Its track becomes the subject.
    pub(crate) fn open_clip(&mut self, clip: Id) {
        if self.editor_clip(clip).is_some() {
            self.open_note_editor(clip);
            return;
        }
        let Some(track) = self.track_of_clip(clip) else {
            return;
        };
        self.close_note_editor();
        self.detail.opened_audio = Some(clip);
        self.detail.anchor = Some(track);
        self.detail_tab = DetailTab::Clip;
        self.show_detail = true;
        self.nav
            .steps
            .push(super::navigate::ViewStep::ShowClip(clip));
    }

    /// A MIDI clip was opened in the note editor: it replaces an opened
    /// audio clip, and its track becomes the subject.
    pub(crate) fn opened_in_note_editor(&mut self, clip: Id) {
        self.detail.opened_audio = None;
        if let Some(track) = self.track_of_clip(clip) {
            self.detail.anchor = Some(track);
        }
    }

    /// Shows `tab` (the tab row, and Show devices, clip and mixer). Clip
    /// only while a clip is open.
    pub(crate) fn show_detail_tab(&mut self, tab: DetailTab) {
        if tab == DetailTab::Clip && self.opened_clip().is_none() {
            return;
        }
        self.detail_tab = tab;
        self.show_detail = true;
        if tab == DetailTab::Devices {
            // Tall enough for a whole card; the user can still drag it.
            self.chain.grow = true;
        }
    }

    /// Once a frame, after the note editor's refresh: what was just
    /// clicked becomes the subject, a header clicked while on Clip shows
    /// Devices, and a deleted opened clip takes its tab with it.
    pub(crate) fn refresh_detail(&mut self, ctx: &egui::Context) {
        if self.arrangement.selected != self.detail.seen_tracks {
            if let Some(&last) = self.arrangement.selected.last() {
                self.detail.anchor = Some(last);
                if self.detail_tab == DetailTab::Clip {
                    self.detail_tab = DetailTab::Devices;
                    self.chain.grow = true;
                }
            }
            self.detail.seen_tracks = self.arrangement.selected.clone();
        }
        if self.arrangement.clips != self.detail.seen_clips {
            let fresh: Vec<Id> = self
                .arrangement
                .clips
                .difference(&self.detail.seen_clips)
                .filter_map(|c| self.track_of_clip(*c))
                .collect();
            // A clip added on the subject's track keeps it; else the
            // first new clip's track.
            if !self.detail.anchor.is_some_and(|a| fresh.contains(&a))
                && let Some(&track) = fresh.first()
            {
                self.detail.anchor = Some(track);
            }
            self.detail.seen_clips = self.arrangement.clips.clone();
        }
        if let Some(clip) = self.detail.opened_audio
            && self.track_of_clip(clip).is_none()
        {
            self.detail.opened_audio = None;
        }
        if self.detail_tab == DetailTab::Clip && self.opened_clip().is_none() {
            self.detail_tab = DetailTab::Devices;
            self.chain.grow = true;
            // The keyboard follows, if it was in the panel.
            self.detail.focus_devices = panes::focused(ctx) == Some(Pane::Detail);
        }
    }

    /// The tab row: the subject, a hairline, Devices and the opened clip's
    /// Clip tab, then Mixer at the right after another hairline. The tabs
    /// are words over a sliding underline (the interface audit's T4).
    pub(crate) fn detail_tabs(&mut self, ui: &mut Ui) {
        let project = self.document.project();
        let subject = self
            .subject()
            .and_then(|id| all_tracks(project).find(|t| t.id == id))
            .map(|t| (t.name.clone(), arrangement::color_of(t.color)));
        let opened = self.opened_clip().and_then(|id| {
            all_tracks(project)
                .flat_map(|t| &t.clips)
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
        });
        let quiet = self.detail_tab == DetailTab::Mixer;
        let devices_tip = match &subject {
            Some((name, _)) => format!("The devices on {name}"),
            None => "The devices on the selected track".to_owned(),
        };
        let mut tabs = vec![widgets::Tab {
            label: "Devices",
            note: None,
            disabled_because: None,
            tip: &devices_tip,
        }];
        if let Some(name) = &opened {
            tabs.push(widgets::Tab {
                label: "Clip",
                note: Some(name),
                disabled_because: None,
                tip: "The opened clip: its properties and wave, or its notes",
            });
        }
        tabs.push(widgets::Tab {
            label: "Mixer",
            note: None,
            disabled_because: None,
            tip: "Every track's channel strip",
        });
        let mixer = tabs.len() - 1;
        let chosen = match self.detail_tab {
            DetailTab::Devices => 0,
            DetailTab::Clip => 1,
            DetailTab::Mixer => mixer,
        };
        let layout = widgets::TabsLayout {
            flush: false,
            right_from: Some(mixer),
            focus: std::mem::take(&mut self.detail.focus_devices).then_some(0),
        };
        let mut picked = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            subject_chip(ui, subject.as_ref(), quiet);
            picked = widgets::tabs_with(ui, egui::Id::new("detail tabs"), chosen, &tabs, layout);
        });
        if let Some(picked) = picked {
            let tab = if picked == mixer {
                DetailTab::Mixer
            } else if picked == 0 {
                DetailTab::Devices
            } else {
                DetailTab::Clip
            };
            self.show_detail_tab(tab);
        }
    }

    /// The chosen tab's content in `content`, drawn after the arrangement
    /// so Tab reaches it in reading order.
    pub(crate) fn detail_content(&mut self, ui: &mut Ui, content: Rect) {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
        child.set_clip_rect(content.intersect(ui.clip_rect()));
        match self.detail_tab {
            DetailTab::Clip => {
                if let Some(clip) = self.detail.opened_audio {
                    self.audio_clip_view(&mut child, content, clip);
                } else if self.note_editor.popped && self.note_editor.clip.is_some() {
                    let state = widgets::EmptyState {
                        icon: None,
                        icon_size: theme::space::L,
                        title: "The clip editor is in its own window",
                        hint: "Close that window, or dock it here",
                        action: Some(widgets::EmptyAction {
                            label: "Dock editor",
                            keys: None,
                            tip: "Put the editor back in the detail panel",
                        }),
                    };
                    if widgets::empty_state(&mut child, content, &state) {
                        self.note_editor.popped = false;
                    }
                } else {
                    // The arrows are the notes' keys while the pane has the
                    // keyboard: egui must not take them to move the focus.
                    super::panes::keep_arrows(ui.ctx(), super::panes::Pane::Detail);
                    self.note_editor_surface(&mut child, content, crate::canvas::Surface::Docked);
                }
            }
            DetailTab::Devices => self.devices_panel(&mut child, content),
            DetailTab::Mixer => {
                self.lend_cue_controls();
                self.arrangement
                    .mixer_panel(&mut child, &mut self.document, &self.keymap);
                self.take_cue_controls();
            }
        }
    }

    /// An opened audio clip: the clip card's rows in a column that scrolls
    /// when the panel is short, beside the clip's wave under its bar
    /// numbers (DP1).
    fn audio_clip_view(&mut self, ui: &mut Ui, area: Rect, clip: Id) {
        let palette = *theme::p();
        let column =
            Rect::from_min_size(area.min, vec2(PROPS_WIDTH.min(area.width()), area.height()));
        ui.painter().rect_filled(column, 0.0, palette.app);
        ui.painter().vline(
            column.right() - theme::stroke::HAIRLINE / 2.0,
            column.y_range(),
            Stroke::new(theme::stroke::HAIRLINE, palette.border),
        );
        let mut props = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(column.shrink(theme::space::M))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        egui::ScrollArea::vertical()
            .id_salt("docked clip properties")
            .auto_shrink([false, false])
            .show(&mut props, |ui| {
                ui.set_width(super::clip_card::CARD_WIDTH.min(ui.available_width()));
                self.docked_clip_props(ui, clip);
            });
        let wave = Rect::from_min_max(pos2(column.right(), area.top()), area.max);
        if wave.width() > 2.0 * theme::space::L {
            self.paint_clip_wave(ui, wave, clip);
        }
    }

    /// The wave of clip `clip` filling `area` (inset 12 pt by 16 pt), at
    /// its gain and through its fades, under a ruler of bar numbers.
    fn paint_clip_wave(&self, ui: &Ui, area: Rect, clip: Id) {
        let palette = *theme::p();
        let project = self.document.project();
        let Some((track, model)) =
            all_tracks(project).find_map(|t| t.clips.iter().find(|c| c.id == clip).map(|c| (t, c)))
        else {
            return;
        };
        let inner = area.shrink2(vec2(theme::space::L, theme::space::M));
        let painter = ui.painter_at(area);
        let (start, length) = (model.start, model.length);
        let quarters = length.as_quarters().max(f64::EPSILON);
        let x_of =
            |q: f64| inner.left() + ((q - start.as_quarters()) / quarters) as f32 * inner.width();

        // The bar numbers, thinned to keep their room.
        if let Ok(meter) = MeterMap::new(project.meter.clone()) {
            let first = meter.bbt_at(start).bar;
            let last = meter.bbt_at(start + length).bar;
            let bars = (last - first + 1).max(1);
            let per_bar = inner.width() / bars as f32;
            let mut every = 1;
            while per_bar * (every as f32) < BAR_ROOM && every < bars {
                every *= 2;
            }
            let font = FontId::monospace(theme::size::MONO_SMALL);
            let mut bar = first;
            while bar <= last {
                let at = meter.bar_start(bar);
                if at >= start && (bar - first) % every == 0 {
                    let x = x_of(at.as_quarters());
                    painter.text(
                        pos2(x + theme::space::XXS, inner.top() + RULER / 2.0),
                        egui::Align2::LEFT_CENTER,
                        (bar + 1).to_string(),
                        font.clone(),
                        palette.text_hint,
                    );
                }
                bar += 1;
            }
        }

        let colour = arrangement::clip_color(track, model);
        let body = Rect::from_min_max(pos2(inner.left(), inner.top() + RULER), inner.max);
        if body.height() < arrangement::TITLE * 2.0 {
            return;
        }
        let radius = theme::radius::SMALL;
        painter.rect_filled(body, radius, palette.clip_body);
        let title = Rect::from_min_size(body.min, vec2(body.width(), arrangement::TITLE));
        painter.rect_filled(
            title,
            egui::CornerRadius {
                nw: radius,
                ne: radius,
                sw: 0,
                se: 0,
            },
            colour,
        );
        let name = widgets::fit(
            &painter,
            &model.name,
            FontId::proportional(theme::size::CAPTION),
            title.width() - 2.0 * theme::space::XS,
        );
        painter.text(
            pos2(title.left() + theme::space::XS + 1.0, title.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            FontId::proportional(theme::size::CAPTION),
            theme::CLIP_TEXT,
        );
        let lane = Rect::from_min_max(pos2(body.left(), title.bottom()), body.max);
        self.paint_wave_columns(&painter, lane, model, theme::wave(colour));
        painter.rect_stroke(
            body,
            radius,
            Stroke::new(theme::stroke::HAIRLINE, colour.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
    }

    /// The wave of `clip` in `lane`, one column a point wide: the extent of
    /// the frames under each column from the peaks, both channels together.
    fn paint_wave_columns(
        &self,
        painter: &egui::Painter,
        lane: Rect,
        clip: &lumen_project::model::Clip,
        colour: Color32,
    ) {
        if !matches!(clip.content, ClipContent::Audio { .. }) {
            return;
        }
        let rate = self.lanes.rate();
        let library = &self.lanes.library;
        let ready = |key: u64| {
            library.with_twins().find(|i| i.id == key).and_then(|item| {
                let (State::Ready(audio), Some(peaks)) = (&item.state, &item.peaks) else {
                    return None;
                };
                (audio.sample_rate == rate).then(|| {
                    (
                        waves::Source {
                            slot: 0,
                            channels: peaks.channels as u32,
                            native_rate: item.info.sample_rate.unwrap_or(audio.sample_rate),
                        },
                        std::sync::Arc::clone(peaks),
                    )
                })
            })
        };
        let spans = waves::one_clip(self.document.project(), clip.id, rate, |key| {
            ready(key).map(|(source, _)| source)
        });
        let Some(peaks) = spans.first().and_then(|s| ready(s.media)).map(|(_, p)| p) else {
            return;
        };
        let start = clip.start.as_quarters();
        let quarters = clip.length.as_quarters().max(f64::EPSILON);
        let fade_in = clip.fade_in.length.as_quarters();
        let fade_out = clip.fade_out.length.as_quarters();
        let middle = lane.center().y;
        let half = lane.height() / 2.0 - theme::space::XXS;
        // One column per physical pixel, so the wave stays crisp when
        // scaled.
        let pixel = 1.0 / painter.pixels_per_point();
        let columns = (lane.width() / pixel).floor().max(0.0) as usize;
        let frame_at = |q: f64| {
            spans
                .iter()
                .find(|s| q >= s.start && q < s.end)
                .or(spans.last())
                .map(|s| (s.first_frame + (q - s.start) * s.frames_per_beat, s.gain))
        };
        let mut mesh = egui::Mesh::default();
        for column in 0..columns {
            let q0 = start + quarters * column as f64 / columns as f64;
            let q1 = start + quarters * (column + 1) as f64 / columns as f64;
            let (Some((f0, gain)), Some((f1, _))) = (frame_at(q0), frame_at(q1)) else {
                continue;
            };
            let [lmin, lmax, rmin, rmax] = peaks.extent(f0.min(f1), f0.max(f1));
            let (lo, hi) = (
                f32::from(lmin.min(rmin)) / 127.0,
                f32::from(lmax.max(rmax)) / 127.0,
            );
            let into = q0 - start;
            let mut level = gain;
            if fade_in > 0.0 && into < fade_in {
                level *= clip.fade_in.level((into / fade_in) as f32);
            }
            let left = quarters - into;
            if fade_out > 0.0 && left < fade_out {
                level *= clip.fade_out.level((left / fade_out) as f32);
            }
            let (a, b) = (lo * level, hi * level);
            let top = middle - a.max(b) * half;
            let bottom = middle - a.min(b) * half;
            let x = lane.left() + column as f32 * pixel;
            mesh.add_colored_rect(
                Rect::from_min_max(pos2(x, top), pos2(x + pixel, bottom.max(top + pixel))),
                colour,
            );
        }
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// The subject at the left of the tab row: a 3 × 14 pt bar in the track's
/// colour and its name in medium type, then a hairline; in hint grey while
/// the Mixer is chosen. "No track selected" when there is none.
fn subject_chip(ui: &mut Ui, subject: Option<&(String, Color32)>, quiet: bool) {
    let palette = theme::p();
    let height = theme::size::CONTROL;
    let ink = if quiet || subject.is_none() {
        palette.text_hint
    } else {
        palette.text
    };
    let text = subject.map_or("No track selected", |(name, _)| name.as_str());
    let font = fonts::medium(theme::size::BODY);
    let fitted = widgets::fit(ui.painter(), text, font.clone(), SUBJECT_WIDTH);
    let galley = ui.painter().layout_no_wrap(fitted, font, ink);
    let bar_room = if subject.is_some() {
        SUBJECT_BAR + theme::space::S
    } else {
        0.0
    };
    let width = theme::space::M + bar_room + galley.size().x + theme::space::S;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let name = subject.map_or_else(|| text.to_owned(), |(name, _)| format!("Showing {name}"));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
    response.on_hover_text(
        "The track the panel is about: the one whose header or clip was clicked last",
    );
    let mut x = rect.left() + theme::space::M;
    if let Some((_, colour)) = subject {
        let alpha = if quiet { 0.5 } else { 1.0 };
        let bar = Rect::from_center_size(
            pos2(x + SUBJECT_BAR / 2.0, rect.center().y),
            vec2(SUBJECT_BAR, theme::size::TAB_DIVIDER),
        );
        ui.painter()
            .rect_filled(bar, 1.0, colour.gamma_multiply(alpha));
        x += bar_room;
    }
    ui.painter().galley(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    widgets::tab_divider(ui, rect.right(), rect.center().y);
}
