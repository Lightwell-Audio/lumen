//! Controls that follow their lane (M7 slice A4a, D5): a fader, pan, trim,
//! send or mute whose lane plays shows the value the lane has at the playhead,
//! stopped or playing, and one the user has taken over (D7) shows its own value
//! and an amber ring, the look the owner chose (A). The interface asks the lane
//! (`Lane::value_at`); it never reads the value back from the engine.

use eframe::egui::{Color32, Rect, Stroke, StrokeKind, Ui};
use lumen_project::automation::Target;
use lumen_project::model::Track;

use super::Arrangement;
use super::auto_state::AutoView;
use crate::document::{Document, Overrides};
use crate::theme;

impl AutoView {
    /// The value `track`'s lane for `target` has at the playhead, when the lane
    /// plays: on, and not taken over.
    pub(crate) fn following(&self, over: &Overrides, track: &Track, target: Target) -> Option<f32> {
        let entry = track.lane(target).filter(|l| over.plays(track, l))?;
        entry.lane.value_at(self.at)
    }
}

impl Arrangement {
    /// What the control for `target` shows: its lane's value at the playhead
    /// when the lane plays, else `own`.
    pub(crate) fn shown(&self, doc: &Document, track: &Track, target: Target, own: f32) -> f32 {
        self.auto
            .following(doc.overrides(), track, target)
            .unwrap_or(own)
    }

    /// Whether the user has taken `track`'s lane for `target` over.
    /// A lane that is being written is taken over to be recorded, not to be
    /// left: it does not show as overridden.
    pub(crate) fn taken_over(&self, doc: &Document, track: &Track, target: Target) -> bool {
        doc.overrides().contains(track.id, target)
            && !self.auto.recording.contains(&(track.id, target))
    }

    /// Whether `track` has any lane taken over.
    pub(crate) fn any_taken_over(&self, doc: &Document, track: &Track) -> bool {
        doc.overrides()
            .targets_of(track.id)
            .any(|target| !self.auto.recording.contains(&(track.id, target)))
    }
}

/// The amber ring around a control whose lane the user has taken over.
pub(crate) fn ring(ui: &Ui, rect: Rect) {
    ring_with(ui, rect, theme::p().warning);
}

/// A ring of `color` around a control: amber for a lane taken over, record
/// red for one being written.
pub(crate) fn ring_with(ui: &Ui, rect: Rect, color: Color32) {
    ui.painter().rect_stroke(
        rect,
        f32::from(theme::radius::for_height(rect.height())),
        Stroke::new(theme::stroke::LIT, color),
        StrokeKind::Outside,
    );
}

/// The amber ring around a round control (a knob) whose lane the user has
/// taken over.
pub(crate) fn ring_round(ui: &Ui, rect: Rect) {
    ui.painter().circle_stroke(
        rect.center(),
        rect.width().min(rect.height()) / 2.0 + 1.5,
        Stroke::new(theme::stroke::LIT, theme::p().warning),
    );
}

/// [`ring_round`] in `color`.
pub(crate) fn ring_round_with(ui: &Ui, rect: Rect, color: Color32) {
    ui.painter().circle_stroke(
        rect.center(),
        rect.width().min(rect.height()) / 2.0 + 1.5,
        Stroke::new(theme::stroke::LIT, color),
    );
}

/// A mute lane's value as the button shows it: muted from one half up.
pub(crate) fn muted(value: f32) -> bool {
    value >= 0.5
}

#[cfg(test)]
mod tests {
    use lumen_core::automation::{Lane, Point};
    use lumen_core::time::Ticks;
    use lumen_project::model::TrackKind;

    use super::*;

    fn doc_with_lane() -> (Document, lumen_project::model::Id) {
        let mut doc = Document::new(crate::empty_project());
        let track = doc.add_track(TrackKind::Audio, None);
        let lane = Lane::new(vec![
            Point::new(Ticks::ZERO, -20.0),
            Point::new(Ticks::quarters(4), 0.0),
        ])
        .unwrap();
        doc.set_lane(track, Target::Volume, lane, false).unwrap();
        (doc, track)
    }

    fn held(doc: &Document, id: lumen_project::model::Id) -> Track {
        (**doc.project().tracks.iter().find(|t| t.id == id).unwrap()).clone()
    }

    #[test]
    fn a_control_shows_the_lane_at_the_playhead_stopped_or_playing() {
        let (doc, id) = doc_with_lane();
        let mut view = Arrangement::default();
        let track = held(&doc, id);
        view.auto.at = Ticks::ZERO;
        assert_eq!(view.shown(&doc, &track, Target::Volume, -3.0), -20.0);
        view.auto.at = Ticks::quarters(2);
        assert_eq!(view.shown(&doc, &track, Target::Volume, -3.0), -10.0);
        view.auto.at = Ticks::quarters(9);
        assert_eq!(
            view.shown(&doc, &track, Target::Volume, -3.0),
            0.0,
            "past the last point the lane holds its value"
        );
    }

    #[test]
    fn a_control_without_a_playing_lane_shows_its_own_value() {
        let (mut doc, id) = doc_with_lane();
        let view = Arrangement::default();
        let track = held(&doc, id);
        assert_eq!(
            view.shown(&doc, &track, Target::Pan, 0.4),
            0.4,
            "no pan lane"
        );
        doc.set_volume(id, -3.0);
        let track = held(&doc, id);
        assert!(view.taken_over(&doc, &track, Target::Volume));
        assert_eq!(view.shown(&doc, &track, Target::Volume, -3.0), -3.0);
        doc.reenable_lane(id, Target::Volume);
        assert!(!view.taken_over(&doc, &track, Target::Volume));
        assert_eq!(view.shown(&doc, &track, Target::Volume, -3.0), -20.0);
        doc.set_lane_enabled(id, Target::Volume, false).unwrap();
        let track = held(&doc, id);
        assert_eq!(view.shown(&doc, &track, Target::Volume, -3.0), -3.0, "off");
    }

    #[test]
    fn mute_reads_from_one_half() {
        assert!(!muted(0.0) && !muted(0.49) && muted(0.5) && muted(1.0));
    }
}
