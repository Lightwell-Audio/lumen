//! What plugins do inside themselves becomes edits of the project (M6 slice
//! 4, the owner's P1): a control moved in a plugin's own window is a gesture
//! (begin, values, end) and one undo step, and values a plugin changes outside
//! a gesture (loading a patch) are one step for the whole batch.

use lumen_hosting::{Edit, EditKind};
use lumen_project::model::Id;

use crate::App;
use crate::document::Document;

impl App {
    /// Records the plugins' own edits since the last frame. Called every
    /// frame after the hosts have been polled.
    pub(crate) fn apply_plugin_edits(&mut self, ctx: &eframe::egui::Context) {
        self.poll_presets(ctx);
        self.apply_presets();
        let all = self.hosting.take_edits();
        let touched: Vec<(Id, Vec<u32>)> = all
            .iter()
            .map(|n| (n.device, crate::chain::editor::touched(&n.edits)))
            .filter(|(_, ids)| !ids.is_empty())
            .collect();
        for news in all {
            let hosting = &self.hosting;
            let title = |id: u32| {
                hosting
                    .parameters(news.device)
                    .iter()
                    .find(|p| p.id == id)
                    .map_or_else(|| "parameter".to_owned(), |p| p.title.clone())
            };
            // While a program the user chose is being loaded, what the plugin
            // sets in answer is part of that choice.
            let loading = hosting.program_loading(news.device).map(str::to_owned);
            record(
                &mut self.document,
                news.device,
                &news.edits,
                title,
                loading.as_deref(),
            );
        }
        if self.hosting.take_inside_changes() {
            self.document.mark_changed_inside();
        }
        self.follow_editors(ctx, &touched);
        self.follow_chosen_track();
        for typed in self.hosting.take_typed() {
            let Some(value) = typed.value else {
                self.say(format!(
                    "The plugin did not understand \u{201c}{}\u{201d}.",
                    typed.text
                ));
                continue;
            };
            let title = self
                .hosting
                .parameters(typed.device)
                .iter()
                .find(|p| p.id == typed.id)
                .map_or_else(|| "parameter".to_owned(), |p| p.title.clone());
            // A typed value is a step of its own, not part of a drag.
            self.document.override_param(typed.device, typed.id);
            self.document.seal();
            let _ = self.document.set_device_params(
                typed.device,
                &[(typed.id, value)],
                &format!("Change {title}"),
                None,
            );
            self.document.seal();
            self.hosting.ask_text(typed.device, typed.id, value);
        }
    }
}

/// The merge key of one parameter's gesture.
fn gesture_key(device: Id, parameter: u32) -> String {
    format!("param:{}:{parameter}", device.0)
}

/// Applies `edits`, which a plugin made in this order, to `device`. A
/// gesture's values merge into the step its begin opens; the rest are
/// gathered into one step. While `loading` names a program the user chose,
/// every edit joins that choice's step instead, whatever its shape.
pub(crate) fn record(
    doc: &mut Document,
    device: Id,
    edits: &[(Edit, bool)],
    title: impl Fn(u32) -> String,
    loading: Option<&str>,
) {
    let mut loose: Vec<(u32, f64)> = Vec::new();
    let flush = |doc: &mut Document, loose: &mut Vec<(u32, f64)>| {
        if loose.is_empty() {
            return;
        }
        if let Some(name) = loading {
            let _ = doc.set_device_params(
                device,
                loose,
                &format!("Load {name}"),
                Some(&super::presets::load_key(device)),
            );
            loose.clear();
            return;
        }
        let label = match loose.as_slice() {
            [(id, _)] => format!("Change {}", title(*id)),
            many => format!("Change {} parameters", many.len()),
        };
        doc.seal();
        let _ = doc.set_device_params(device, loose, &label, None);
        doc.seal();
        loose.clear();
    };
    for &(edit, inside) in edits {
        match edit.kind {
            EditKind::Begin => {
                flush(doc, &mut loose);
                if loading.is_none() {
                    doc.seal();
                }
            }
            EditKind::Perform if inside => {
                flush(doc, &mut loose);
                // A control a lane drives, moved in the plugin's own window,
                // takes the lane over (a program the user chose does not).
                if loading.is_none() {
                    doc.override_param(device, edit.id);
                }
                let (label, key) = match loading {
                    Some(name) => (format!("Load {name}"), super::presets::load_key(device)),
                    None => (
                        format!("Change {}", title(edit.id)),
                        gesture_key(device, edit.id),
                    ),
                };
                let _ = doc.set_device_params(device, &[(edit.id, edit.value)], &label, Some(&key));
            }
            EditKind::Perform => {
                // Several values for one parameter in a row: the last counts.
                match loose.iter_mut().find(|(id, _)| *id == edit.id) {
                    Some(slot) => slot.1 = edit.value,
                    None => loose.push((edit.id, edit.value)),
                }
            }
            EditKind::End => {
                flush(doc, &mut loose);
                if loading.is_none() {
                    doc.seal();
                }
            }
            // Not an edit of the project.
            EditKind::Output | EditKind::Dirty => {}
        }
    }
    flush(doc, &mut loose);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Slot;
    use lumen_project::model::{DeviceKind, PluginRef, TrackKind};

    fn with_device() -> (Document, Id) {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Audio, None);
        let id = doc
            .add_device(
                track,
                PluginRef::Vst3 {
                    class_id: "ABCDEF0123456789ABCDEF0123456789".into(),
                    name: "Verb".into(),
                    vendor: "V".into(),
                    kind: DeviceKind::Effect,
                },
                Slot::End,
            )
            .unwrap();
        doc.seal();
        (doc, id)
    }

    fn e(id: u32, value: f64, kind: EditKind, inside: bool) -> (Edit, bool) {
        (Edit { id, value, kind }, inside)
    }

    fn params(doc: &Document, device: Id) -> Vec<(u32, f64)> {
        doc.project()
            .tracks
            .iter()
            .flat_map(|t| &t.devices)
            .find(|d| d.id == device)
            .unwrap()
            .params
            .iter()
            .map(|(&k, &v)| (k, v))
            .collect()
    }

    fn name(id: u32) -> String {
        format!("P{id}")
    }

    #[test]
    fn a_gesture_in_the_plugins_window_is_one_undo_step() {
        let (mut doc, device) = with_device();
        record(
            &mut doc,
            device,
            &[
                e(3, 0.0, EditKind::Begin, true),
                e(3, 0.1, EditKind::Perform, true),
                e(3, 0.4, EditKind::Perform, true),
                e(3, 0.9, EditKind::Perform, true),
                e(3, 0.0, EditKind::End, true),
            ],
            name,
            None,
        );
        assert_eq!(params(&doc, device), [(3, 0.9)]);
        assert_eq!(doc.undo().as_deref(), Some("Change P3"));
        assert!(params(&doc, device).is_empty(), "the whole gesture undone");
        doc.redo();
        assert_eq!(params(&doc, device), [(3, 0.9)]);
    }

    #[test]
    fn a_gesture_that_spans_several_calls_is_still_one_step() {
        let (mut doc, device) = with_device();
        record(
            &mut doc,
            device,
            &[e(3, 0.0, EditKind::Begin, true)],
            name,
            None,
        );
        record(
            &mut doc,
            device,
            &[e(3, 0.2, EditKind::Perform, true)],
            name,
            None,
        );
        // The frame-by-frame seal does not run while the gesture is open.
        record(
            &mut doc,
            device,
            &[e(3, 0.6, EditKind::Perform, true)],
            name,
            None,
        );
        record(
            &mut doc,
            device,
            &[e(3, 0.0, EditKind::End, true)],
            name,
            None,
        );
        doc.undo();
        assert!(params(&doc, device).is_empty());
    }

    #[test]
    fn two_gestures_on_the_same_control_are_two_steps() {
        let (mut doc, device) = with_device();
        for v in [0.3, 0.8] {
            record(
                &mut doc,
                device,
                &[
                    e(3, 0.0, EditKind::Begin, true),
                    e(3, v, EditKind::Perform, true),
                    e(3, 0.0, EditKind::End, true),
                ],
                name,
                None,
            );
        }
        doc.undo();
        assert_eq!(params(&doc, device), [(3, 0.3)]);
        doc.undo();
        assert!(params(&doc, device).is_empty());
    }

    #[test]
    fn values_outside_a_gesture_are_one_step_for_the_batch() {
        let (mut doc, device) = with_device();
        let batch: Vec<_> = (0..50)
            .map(|i| e(i, f64::from(i) / 100.0, EditKind::Perform, false))
            .collect();
        record(&mut doc, device, &batch, name, None);
        assert_eq!(params(&doc, device).len(), 50);
        assert_eq!(doc.undo().as_deref(), Some("Change 50 parameters"));
        assert!(params(&doc, device).is_empty());
    }

    #[test]
    fn a_gesture_on_a_parameter_a_lane_drives_takes_the_lane_over_but_a_program_does_not() {
        use lumen_core::automation::{Lane, Point};
        use lumen_project::automation::Target;
        let (mut doc, device) = with_device();
        let track = doc.project().tracks[0].id;
        let lane = Lane::new(vec![Point::new(lumen_core::time::Ticks::ZERO, 0.5)]).unwrap();
        let target = Target::Device { device, param: 3 };
        doc.set_lane(track, target, lane, false).unwrap();
        // A program the user chose sets the parameter: the lane plays on.
        record(
            &mut doc,
            device,
            &[e(3, 0.9, EditKind::Perform, true)],
            name,
            Some("Warm"),
        );
        assert!(!doc.overrides().contains(track, target));
        // Another parameter, moved by hand in the plugin's window, is no lane's.
        record(
            &mut doc,
            device,
            &[
                e(4, 0.0, EditKind::Begin, true),
                e(4, 0.4, EditKind::Perform, true),
                e(4, 0.0, EditKind::End, true),
            ],
            name,
            None,
        );
        assert!(!doc.overrides().contains(track, target));
        // Taking hold of the driven one without moving it does nothing; moving
        // it takes the lane over.
        record(
            &mut doc,
            device,
            &[
                e(3, 0.0, EditKind::Begin, true),
                e(3, 0.0, EditKind::End, true),
            ],
            name,
            None,
        );
        assert!(!doc.overrides().contains(track, target));
        record(
            &mut doc,
            device,
            &[
                e(3, 0.0, EditKind::Begin, true),
                e(3, 0.7, EditKind::Perform, true),
                e(3, 0.0, EditKind::End, true),
            ],
            name,
            None,
        );
        assert!(doc.overrides().contains(track, target));
    }

    #[test]
    fn what_the_processor_reports_is_not_an_edit() {
        let (mut doc, device) = with_device();
        let before = doc.state();
        record(
            &mut doc,
            device,
            &[e(9, 0.5, EditKind::Output, false)],
            name,
            None,
        );
        assert_eq!(doc.state(), before);
    }
}
