//! The track header menus (M3 slice 4a): a track's right-click menu and
//! the "Add … track" entries. Grouping joins them in M4 slice 3.

use eframe::egui::{self, Sense, Stroke, Ui, vec2};
use lumen_devices::Preset;
use lumen_project::model::{Id, TrackKind};

use super::{Arrangement, color_from, midi_io};
use crate::commands::{Command, Keymap};
use crate::document::Document;
use crate::{theme, widgets};

impl Arrangement {
    /// Right-click menu on a track, in the owner's order (the interface
    /// audit's M4, A): what it is called and looks like; how it sits among
    /// the others; what it plays and shows; what is made from it; adding
    /// tracks; and Delete last, apart. Rows that cannot act are left out.
    pub(super) fn track_menu(&mut self, ui: &mut Ui, doc: &mut Document, id: Id, keymap: &Keymap) {
        let targets = self.targets(id);
        let many = targets.len() > 1;
        let item = |ui: &mut Ui, label: &str, command: Command| {
            widgets::menu_item(ui, label, keymap.keys(command).as_ref(), true).clicked()
        };
        let plural = |one: &'static str, several: &'static str| if many { several } else { one };

        // Name and colour.
        if item(ui, "Rename", Command::Rename)
            && let Some(track) = doc.project().tracks.iter().find(|t| t.id == id)
        {
            self.renaming = Some((id, track.name.clone()));
        }
        widgets::submenu(ui, "Color", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(6.0 * 26.0);
                for (index, c) in theme::CLIP_COLORS.into_iter().enumerate() {
                    let (r, response) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                    let name = format!("Track color {}", index + 1);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name)
                    });
                    let response = response.on_hover_text(name);
                    ui.painter().rect_filled(r, theme::radius::SMALL, c);
                    widgets::raised::focus_ring(ui.painter(), &response, r);
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

        // Among the other tracks.
        if item(
            ui,
            plural("Duplicate", "Duplicate tracks"),
            Command::Duplicate,
        ) {
            self.selected = doc.duplicate_tracks(&targets);
        }
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
        if self.can_move_selected(doc, -1) && item(ui, "Move up", Command::MoveTracksUp) {
            self.move_selected(doc, -1);
        }
        if self.can_move_selected(doc, 1) && item(ui, "Move down", Command::MoveTracksDown) {
            self.move_selected(doc, 1);
        }
        ui.separator();

        // What it plays and what it shows.
        self.midi_menus(ui, doc, id, &targets);
        self.audio_menus(ui, doc, id);
        // The lane over the clips (the header's A button until ADR-0101).
        let lane = if self.auto.shown.contains_key(&id) {
            "Hide automation lane"
        } else {
            "Show automation lane"
        };
        if item(ui, lane, Command::AutomationShow) {
            self.selected = targets.clone();
            self.track_command = Some(Command::AutomationShow);
        }
        let fold = if self.expanded.contains(&id) {
            "Fold"
        } else {
            "Unfold"
        };
        if widgets::menu_item(ui, fold, None, true).clicked() {
            self.toggle_fold(id);
            ui.close();
        }
        if group {
            let label = if self.collapsed.contains(&id) {
                "Show its tracks"
            } else {
                "Hide its tracks"
            };
            if widgets::menu_item(ui, label, None, true).clicked() {
                self.toggle_collapsed(doc.project(), id);
                ui.close();
            }
        }
        ui.separator();

        // What is made from it.
        if item(ui, "Import audio or MIDI\u{2026}", Command::Import) {
            self.selected = vec![id];
            self.import_requested = true;
        }
        let bounces = targets.iter().any(|t| {
            doc.project()
                .tracks
                .iter()
                .any(|x| x.id == *t && matches!(x.kind, TrackKind::Audio | TrackKind::Midi))
        });
        if bounces && item(ui, "Bounce", Command::Bounce) {
            self.selected = targets.clone();
            self.bounce_requested = true;
        }
        // Freezing (M7b slice E5): offered where it can run.
        let on = |frozen: bool| {
            targets.iter().any(|t| {
                doc.project().tracks.iter().any(|x| {
                    x.id == *t
                        && x.frozen.is_some() == frozen
                        && (frozen || matches!(x.kind, TrackKind::Audio | TrackKind::Midi))
                })
            })
        };
        let (can_freeze, can_thaw) = (on(false), on(true));
        let freezing = [
            (
                can_freeze,
                plural("Freeze", "Freeze tracks"),
                Command::FreezeTracks,
            ),
            (
                can_thaw,
                plural("Unfreeze", "Unfreeze tracks"),
                Command::UnfreezeTracks,
            ),
            (
                can_thaw,
                plural("Flatten", "Flatten tracks"),
                Command::FlattenTracks,
            ),
        ];
        for (shown, label, command) in freezing {
            if shown && item(ui, label, command) {
                self.selected = targets.clone();
                self.track_command = Some(command);
            }
        }
        ui.separator();

        widgets::submenu(ui, "Add track", |ui| {
            self.add_menu(ui, doc, Some(id), keymap, AddMenu::Submenu);
        });
        ui.separator();
        if item(ui, plural("Delete", "Delete tracks"), Command::Delete) {
            doc.delete_tracks(&targets);
            self.selected.clear();
        }
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
        let from = track.midi_from.clone();
        let monitor = track.monitor;
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
        widgets::submenu(ui, "MIDI From", |ui| {
            midi_io::from_menu(ui, doc, &midi, &from);
        });
        widgets::submenu(ui, "MIDI To", |ui| {
            midi_io::to_menu(ui, doc, &midi, &to);
        });
        midi_io::monitor_row(ui, doc, &midi, monitor);
    }

    /// What an audio track records (M9 slice R1): its input and when it is
    /// heard. Only on audio tracks; applies to every selected audio track.
    fn audio_menus(&mut self, ui: &mut Ui, doc: &mut Document, id: Id) {
        let Some(track) = doc
            .project()
            .tracks
            .iter()
            .find(|t| t.id == id && t.kind == TrackKind::Audio)
        else {
            return;
        };
        let (input, monitor) = (track.input, track.monitor);
        let audio = self.audio_targets(doc, id);
        let mut choices = std::mem::take(&mut self.inputs);
        widgets::submenu(ui, "Input", |ui| {
            super::audio_in::menu(ui, doc, &audio, input, &mut choices);
        });
        self.inputs = choices;
        midi_io::monitor_row(ui, doc, &audio, monitor);
    }

    /// "Add … track" entries, adding after `after`, in the place `menu`.
    pub(super) fn add_menu(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        after: Option<Id>,
        keymap: &Keymap,
        menu: AddMenu,
    ) {
        for (kind, full_name, short_name, command) in [
            (
                TrackKind::Audio,
                "Add audio track",
                "Audio",
                Some(Command::AddAudioTrack),
            ),
            (
                TrackKind::Midi,
                "Add MIDI track",
                "MIDI",
                Some(Command::AddMidiTrack),
            ),
            (TrackKind::Group, "Add group track", "Group", None),
            (TrackKind::Return, "Add return track", "Return", None),
        ] {
            let text = if menu == AddMenu::Submenu {
                short_name
            } else {
                full_name
            };
            let keys = command.and_then(|c| keymap.keys(c));
            let full = kind == TrackKind::Return && doc.returns_full();
            // A context menu lists only what can apply (the audit's M3).
            if full && menu != AddMenu::Button {
                continue;
            }
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

/// Where the "Add … track" entries are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AddMenu {
    /// Under the "+ Add track ▾" button: a full project's return row stays,
    /// greyed, saying why.
    Button,
    /// The right-click menu of the empty area under the tracks.
    Context,
    /// The track menu's "Add track" submenu, named by kind alone.
    Submenu,
}
