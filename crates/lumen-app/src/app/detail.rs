//! The detail panel's tabs and what each shows (M5 slice 3c extracted this
//! from the frame loop). The row names its subject at the left: the track
//! whose header or clip was clicked last (DP1). Devices is that track's
//! chain (M6 slice 2d); Clip appears for the opened clip, an audio clip's
//! properties beside its wave or a MIDI clip's notes; Mixer, at the right,
//! is the channel strips.

use std::collections::BTreeSet;

use eframe::egui::{self, Color32, Rect, Sense, Ui, pos2, vec2};
use lumen_project::model::Id;
use lumen_project::routing::all_tracks;

use super::panes::{self, Pane};
use crate::arrangement;
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
        // Opening a clip frames the editor on it, even the same one again.
        self.audio_editor.framed_for = None;
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
        // A selection made some other way (the arrow keys, a range) moves
        // the subject; only a header click leaves the Clip tab.
        if self.arrangement.selected != self.detail.seen_tracks {
            if let Some(&last) = self.arrangement.selected.last() {
                self.detail.anchor = Some(last);
            }
            self.detail.seen_tracks = self.arrangement.selected.clone();
        }
        if let Some(track) = self.arrangement.header_clicked.take() {
            self.detail.anchor = Some(track);
            if self.detail_tab == DetailTab::Clip {
                self.detail_tab = DetailTab::Devices;
                self.chain.grow = true;
            }
        }
        if self.arrangement.clips != self.detail.seen_clips {
            let fresh: BTreeSet<Id> = self
                .arrangement
                .clips
                .difference(&self.detail.seen_clips)
                .copied()
                .collect();
            // One pass over the tracks: a clip added on the subject's track
            // keeps it; else the first track with a new clip.
            let tracks: Vec<Id> = all_tracks(self.document.project())
                .filter(|t| t.clips.iter().any(|c| fresh.contains(&c.id)))
                .map(|t| t.id)
                .collect();
            if !self.detail.anchor.is_some_and(|a| tracks.contains(&a))
                && let Some(&track) = tracks.first()
            {
                self.detail.anchor = Some(track);
            }
            self.detail.seen_clips = self.arrangement.clips.clone();
        }
        self.settle_opened_clip(ctx);
    }

    /// A deleted opened clip takes its tab with it. Also run before the
    /// tab row is drawn, since a key can delete it after `refresh_detail`.
    fn settle_opened_clip(&mut self, ctx: &egui::Context) {
        if let Some(clip) = self.detail.opened_audio
            && self.track_of_clip(clip).is_none()
        {
            self.detail.opened_audio = None;
        }
        let gone = self
            .opened_clip()
            .is_none_or(|clip| self.track_of_clip(clip).is_none());
        if self.detail_tab == DetailTab::Clip && gone {
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
        self.settle_opened_clip(&ui.ctx().clone());
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
            DetailTab::Clip if opened.is_some() => 1,
            DetailTab::Clip => 0,
            DetailTab::Mixer => mixer,
        };
        let layout = widgets::TabsLayout {
            flush: false,
            right_from: Some(mixer),
            focus: std::mem::take(&mut self.detail.focus_devices).then_some(0),
        };
        // A tab coming or going moves the others: the underline jumps to
        // its place rather than gliding across from where the index was.
        let tabs_id = egui::Id::new(("detail tabs", tabs.len()));
        let mut picked = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            subject_chip(ui, subject.as_ref(), quiet);
            picked = widgets::tabs_with(ui, tabs_id, chosen, &tabs, layout);
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

    /// The audio clip opened in the Clip tab, if any.
    pub(crate) fn detail_audio_clip(&self) -> Option<Id> {
        self.detail.opened_audio
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
    let fitted = widgets::fit(ui.painter(), text, font.clone(), theme::size::SUBJECT_NAME);
    let galley = ui.painter().layout_no_wrap(fitted, font, ink);
    let bar_room = if subject.is_some() {
        theme::size::SUBJECT_BAR + theme::space::S
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
        let alpha = if quiet { theme::DIMMED } else { 1.0 };
        let bar = Rect::from_center_size(
            pos2(x + theme::size::SUBJECT_BAR / 2.0, rect.center().y),
            vec2(theme::size::SUBJECT_BAR, theme::size::TAB_DIVIDER),
        );
        ui.painter()
            .rect_filled(bar, theme::radius::MARK, colour.gamma_multiply(alpha));
        x += bar_room;
    }
    ui.painter().galley(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    widgets::tab_divider(ui, rect.right(), rect.center().y);
}
