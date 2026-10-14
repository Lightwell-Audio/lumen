//! The detail panel's tabs and what each shows (M5 slice 3c extracted this
//! from the frame loop): Devices arrive in Milestone 6, Clip is the MIDI
//! note editor, Mixer is the channel strips.

use eframe::egui::{self, Rect, Ui};

use crate::{App, theme, widgets};

/// Which tab of the detail panel is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DetailTab {
    Devices,
    Clip,
    #[default]
    Mixer,
}

impl DetailTab {
    const ALL: [DetailTab; 3] = [DetailTab::Devices, DetailTab::Clip, DetailTab::Mixer];

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(2)
    }
}

impl App {
    /// The tab row, as words over a sliding underline (the interface
    /// audit's T4). Devices arrives with Milestone 6.
    pub(crate) fn detail_tabs(&mut self, ui: &mut Ui) {
        let devices = crate::arrives("Milestone 6");
        let tabs = [
            widgets::Tab {
                label: "Devices",
                disabled_because: Some(&devices),
                tip: "",
            },
            widgets::Tab {
                label: "Clip",
                disabled_because: None,
                tip: "Edit the notes of a MIDI clip",
            },
            widgets::Tab {
                label: "Mixer",
                disabled_because: None,
                tip: "Every track's channel strip",
            },
        ];
        let chosen = self.detail_tab.index();
        if let Some(picked) = widgets::tabs(ui, egui::Id::new("detail tabs"), chosen, &tabs) {
            self.detail_tab = DetailTab::ALL[picked];
        }
    }

    /// The chosen tab's content in `content`, drawn after the arrangement
    /// so Tab reaches it in reading order.
    pub(crate) fn detail_content(&mut self, ui: &mut Ui, content: Rect) {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
        child.set_clip_rect(content.intersect(ui.clip_rect()));
        match self.detail_tab {
            DetailTab::Clip => {
                if self.note_editor.popped && self.note_editor.clip.is_some() {
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
            DetailTab::Devices | DetailTab::Mixer => {
                self.lend_cue_controls();
                self.arrangement
                    .mixer_panel(&mut child, &mut self.document, &self.keymap);
                self.take_cue_controls();
            }
        }
    }
}
