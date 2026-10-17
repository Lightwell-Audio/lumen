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
    pub(crate) fn apply_plugin_edits(&mut self) {
        for news in self.hosting.take_edits() {
            let hosting = &self.hosting;
            let title = |id: u32| {
                hosting
                    .parameters(news.device)
                    .iter()
                    .find(|p| p.id == id)
                    .map_or_else(|| "parameter".to_owned(), |p| p.title.clone())
            };
            record(&mut self.document, news.device, &news.edits, title);
        }
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
/// gathered into one step.
pub(crate) fn record(
    doc: &mut Document,
    device: Id,
    edits: &[(Edit, bool)],
    title: impl Fn(u32) -> String,
) {
    let mut loose: Vec<(u32, f64)> = Vec::new();
    let flush = |doc: &mut Document, loose: &mut Vec<(u32, f64)>| {
        if loose.is_empty() {
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
                doc.seal();
            }
            EditKind::Perform if inside => {
                flush(doc, &mut loose);
                let label = format!("Change {}", title(edit.id));
                let key = gesture_key(device, edit.id);
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
                doc.seal();
            }
            // Not an edit of the project.
            EditKind::Output => {}
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
        record(&mut doc, device, &[e(3, 0.0, EditKind::Begin, true)], name);
        record(
            &mut doc,
            device,
            &[e(3, 0.2, EditKind::Perform, true)],
            name,
        );
        // The frame-by-frame seal does not run while the gesture is open.
        record(
            &mut doc,
            device,
            &[e(3, 0.6, EditKind::Perform, true)],
            name,
        );
        record(&mut doc, device, &[e(3, 0.0, EditKind::End, true)], name);
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
        record(&mut doc, device, &batch, name);
        assert_eq!(params(&doc, device).len(), 50);
        assert_eq!(doc.undo().as_deref(), Some("Change 50 parameters"));
        assert!(params(&doc, device).is_empty());
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
        );
        assert_eq!(doc.state(), before);
    }
}
