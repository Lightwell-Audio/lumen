//! The track header menus (M3 slice 4a): a track's right-click menu and
//! the "Add … track" entries. Grouping joins them in M4 slice 3.

use eframe::egui::{self, Sense, Stroke, Ui, vec2};
use lumen_devices::Preset;
use lumen_project::model::{Id, MidiTo, TrackKind};

use super::{Arrangement, color_from};
use crate::app::hardware::MenuPorts;
use crate::commands::{Command, Keymap};
use crate::document::Document;
use crate::{theme, widgets};

impl Arrangement {
    /// Right-click menu on a track.
    pub(super) fn track_menu(&mut self, ui: &mut Ui, doc: &mut Document, id: Id, keymap: &Keymap) {
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
        // Grouping (M4 slice 3): offered where it can run.
        let project = doc.project();
        let track = project.tracks.iter().find(|t| t.id == id);
        let group = track.is_some_and(|t| t.kind == TrackKind::Group);
        let in_group = track.is_some_and(|t| t.parent.is_some());
        let groupable = targets.iter().any(|t| {
            project
                .tracks
                .iter()
                .any(|x| x.id == *t && lumen_project::groups::can_be_grouped(x.kind))
        });
        if groupable && item(ui, "Group tracks", Command::GroupTracks) {
            self.group_selected(doc);
        }
        let ungroup = match (group, in_group) {
            (true, _) => Some("Ungroup"),
            (false, true) => Some("Take out of group"),
            (false, false) => None,
        };
        if let Some(label) = ungroup
            && item(ui, label, Command::Ungroup)
        {
            self.ungroup_selected(doc);
        }
        if group {
            let shown = !self.collapsed.contains(&id);
            let label = if shown {
                "Hide its tracks"
            } else {
                "Show its tracks"
            };
            if ui.button(label).clicked() {
                self.toggle_collapsed(doc.project(), id);
                ui.close();
            }
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
        self.midi_menus(ui, doc, id, &targets);
        widgets::submenu(ui, "Colour", |ui| {
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

    /// What plays a MIDI track (M5 slice 1): its instrument, or a MIDI
    /// output port and channel. Only on MIDI tracks; applies to every
    /// selected MIDI track.
    fn midi_menus(&mut self, ui: &mut Ui, doc: &mut Document, id: Id, targets: &[Id]) {
        let project = doc.project();
        let Some(track) = project
            .tracks
            .iter()
            .find(|t| t.id == id && t.kind == TrackKind::Midi)
        else {
            return;
        };
        let midi: Vec<Id> = project
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Midi && targets.contains(&t.id))
            .map(|t| t.id)
            .collect();
        let preset = track
            .instrument()
            .map(|d| d.preset().and_then(Preset::from_id).unwrap_or_default());
        let to = track.midi_to.clone();
        let ports = MenuPorts::of(ui.ctx());
        widgets::submenu(ui, "Instrument", |ui| {
            for p in Preset::ALL {
                if widgets::menu_toggle(ui, p.name(), None, to.is_none() && preset == Some(p))
                    .clicked()
                {
                    doc.set_instrument(&midi, Some(p));
                    ui.close();
                }
            }
            ui.separator();
            if widgets::menu_toggle(ui, "None", None, preset.is_none()).clicked() {
                doc.set_instrument(&midi, None);
                ui.close();
            }
        });
        widgets::submenu(ui, "MIDI To", |ui| {
            if widgets::menu_toggle(ui, "Instrument", None, to.is_none()).clicked() {
                doc.set_midi_to(&midi, None);
                ui.close();
            }
            ui.separator();
            if ports.0.is_empty() {
                widgets::menu_item(ui, "No MIDI outputs", None, false);
            }
            for port in &ports.0 {
                let label = if port.present {
                    port.label.clone()
                } else {
                    format!("{} (not connected)", port.label)
                };
                widgets::submenu(ui, label, |ui| {
                    for channel in 0..16_u8 {
                        let chosen = to
                            .as_ref()
                            .is_some_and(|t| t.port == port.id && t.channel == channel);
                        let name = format!("Channel {}", channel + 1);
                        if widgets::menu_toggle(ui, &name, None, chosen).clicked() {
                            let out = MidiTo {
                                port: port.id.clone(),
                                channel,
                            };
                            doc.set_midi_to(&midi, Some(out));
                            ui.close();
                        }
                    }
                });
            }
        });
    }

    /// "Add … track" entries, adding after `after`.
    pub(super) fn add_menu(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        after: Option<Id>,
        keymap: &Keymap,
    ) {
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
            let full = kind == TrackKind::Return && doc.returns_full();
            let item = widgets::menu_item(ui, text, keys.as_ref(), !full);
            let item = if full {
                item.on_disabled_hover_text("A project holds 12 return tracks at most")
            } else {
                item
            };
            if item.clicked() {
                if kind == TrackKind::Return {
                    // After the last return, shown even if folded away.
                    self.add_track(doc, kind);
                } else {
                    self.selected = vec![doc.add_track(kind, after)];
                }
            }
        }
    }
}
