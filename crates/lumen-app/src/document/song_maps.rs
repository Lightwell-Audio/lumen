//! The song's tempo and time-signature maps (M7 slice A6, format v24): the
//! changes the two lanes show, and the fields of the transport bar. Each is
//! one undo step; a drag passes a merge key so a whole gesture is one.
//!
//! The pure rules (clamps, bar lines, what a meter edit does to the changes
//! after it) are `lumen_project::edit::maps`.

use lumen_core::meter::Meter;
use lumen_core::time::Ticks;
use lumen_project::edit::maps;

use super::{Document, MAX_BEATS_PER_BAR};

impl Document {
    /// Sets the tempo of the segment `at` falls in, clamped to 20 to 999 BPM
    /// (`merge` joins the changes of one drag into one step). With one tempo
    /// in the song this is the song's tempo.
    pub fn set_tempo_at(&mut self, at: Ticks, bpm: f64, merge: Option<&str>) {
        self.history.apply("Change tempo", merge, |project| {
            maps::set_tempo_in(&mut project.tempo, at, bpm);
        });
    }

    /// Puts a tempo at `at`: a new change there, or the value of the one
    /// already there. False when the song holds as many as it may.
    pub fn put_tempo_change(&mut self, at: Ticks, bpm: f64, merge: Option<&str>) -> bool {
        let mut room = true;
        self.history.apply("Add tempo change", merge, |project| {
            room = maps::put_tempo(&mut project.tempo, at, bpm);
        });
        room
    }

    /// Moves tempo change `index` to `at` and `bpm`: the first stays at
    /// zero, another stays between its neighbours.
    pub fn move_tempo_change(&mut self, index: usize, at: Ticks, bpm: f64, merge: Option<&str>) {
        self.history.apply("Move tempo change", merge, |project| {
            maps::move_tempo(&mut project.tempo, index, at, bpm);
        });
    }

    /// Makes tempo change `index` glide to the next change's tempo (a ramp)
    /// or hold its own until the next change (a step). Nothing happens for
    /// the last change, which has no next one.
    pub fn set_tempo_ramp(&mut self, index: usize, ramp: bool) {
        let name = if ramp { "Ramp tempo" } else { "Step tempo" };
        self.history.apply(name, None, |project| {
            maps::set_ramp(&mut project.tempo, index, ramp);
        });
    }

    /// Removes tempo change `index`; the first stays.
    pub fn remove_tempo_change(&mut self, index: usize) {
        self.history.apply("Remove tempo change", None, |project| {
            maps::remove_tempo(&mut project.tempo, index);
        });
    }

    /// Sets the time signature of the segment `at` falls in (M3 review M2):
    /// beats per bar clamped to 1 to 64 and a beat unit that is a power of
    /// two up to 64, the range a project may hold (`lumen_project::validate`);
    /// another unit keeps the current one. Changes after it move to the
    /// nearest bar line of the new bars. `merge` joins the changes of one
    /// drag.
    pub fn set_time_signature_at(
        &mut self,
        at: Ticks,
        numerator: u32,
        denominator: u32,
        merge: Option<&str>,
    ) {
        let meter = self.fitted_meter(at, numerator, denominator);
        self.history
            .apply("Change time signature", merge, |project| {
                maps::set_meter_in(&mut project.meter, at, meter);
            });
    }

    /// The meter `numerator` and `denominator` make in the segment at `at`,
    /// as a project can hold it.
    fn fitted_meter(&self, at: Ticks, numerator: u32, denominator: u32) -> Meter {
        let current = self.project().meter_map().meter_at(at).denominator;
        let denominator = Some(denominator)
            .filter(|d| d.is_power_of_two() && *d <= 64)
            .unwrap_or(current);
        Meter {
            numerator: numerator.clamp(1, MAX_BEATS_PER_BAR),
            denominator,
        }
    }

    /// Puts a time signature on the bar line nearest `at`: a new change, or
    /// the meter of the one there. False when the song holds as many as it
    /// may.
    pub fn put_meter_change(&mut self, at: Ticks, numerator: u32, denominator: u32) -> bool {
        let meter = self.fitted_meter(at, numerator, denominator);
        let mut room = true;
        self.history.apply("Add time signature", None, |project| {
            room = maps::put_meter(&mut project.meter, at, meter);
        });
        room
    }

    /// Sets the time signature of change `index` (the first included).
    pub fn set_meter_change(
        &mut self,
        index: usize,
        numerator: u32,
        denominator: u32,
        merge: Option<&str>,
    ) {
        let Some(at) = self.project().meter.get(index).map(|p| p.at) else {
            return;
        };
        let meter = self.fitted_meter(at, numerator, denominator);
        self.history
            .apply("Change time signature", merge, |project| {
                maps::set_meter_of(&mut project.meter, index, meter);
            });
    }

    /// Moves time-signature change `index` to the bar line nearest `at`,
    /// between its neighbours; the first stays at zero.
    pub fn move_meter_change(&mut self, index: usize, at: Ticks, merge: Option<&str>) {
        self.history.apply("Move time signature", merge, |project| {
            maps::move_meter(&mut project.meter, index, at);
        });
    }

    /// Removes time-signature change `index`; the first stays.
    pub fn remove_meter_change(&mut self, index: usize) {
        self.history
            .apply("Remove time signature", None, |project| {
                maps::remove_meter(&mut project.meter, index);
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::PPQ;

    fn doc() -> Document {
        Document::new(crate::empty_project())
    }

    fn signature(doc: &Document, at: Ticks) -> (u32, u32) {
        let m = doc.project().meter_map().meter_at(at);
        (m.numerator, m.denominator)
    }

    #[test]
    fn the_tempo_is_clamped_and_a_drag_is_one_step() {
        let mut doc = doc();
        doc.set_tempo_at(Ticks::ZERO, 140.0, None);
        for bpm in [141.0, 142.0, 2000.0] {
            doc.set_tempo_at(Ticks::ZERO, bpm, Some("drag"));
        }
        assert_eq!(doc.project().tempo[0].bpm, 999.0);
        doc.undo();
        assert_eq!(doc.project().tempo[0].bpm, 140.0, "the drag was one step");
        doc.set_tempo_at(Ticks::ZERO, 1.0, None);
        assert_eq!(doc.project().tempo[0].bpm, 20.0);
    }

    #[test]
    fn a_tempo_field_changes_the_segment_under_the_playhead() {
        let mut doc = doc();
        doc.put_tempo_change(Ticks::quarters(8), 90.0, None);
        doc.set_tempo_at(Ticks::quarters(10), 100.0, None);
        let bpm: Vec<f64> = doc.project().tempo.iter().map(|p| p.bpm).collect();
        assert_eq!(bpm, [120.0, 100.0]);
        doc.set_tempo_at(Ticks::quarters(2), 110.0, None);
        assert_eq!(doc.project().tempo[0].bpm, 110.0);
        assert_eq!(doc.project().tempo.len(), 2, "no point was added");
    }

    #[test]
    fn a_tempo_change_is_one_undo_step_each_and_a_drag_is_one() {
        let mut doc = doc();
        assert!(doc.put_tempo_change(Ticks::quarters(4), 100.0, None));
        for q in [5, 6, 7] {
            doc.move_tempo_change(1, Ticks::quarters(q), 100.0 + q as f64, Some("drag"));
        }
        assert_eq!(doc.project().tempo[1].at, Ticks::quarters(7));
        doc.undo();
        assert_eq!(doc.project().tempo[1].at, Ticks::quarters(4), "one step");
        doc.remove_tempo_change(1);
        assert_eq!(doc.project().tempo.len(), 1);
        doc.undo();
        assert_eq!(doc.project().tempo.len(), 2);
        doc.remove_tempo_change(0);
        assert_eq!(doc.project().tempo.len(), 2, "the first stays");
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn a_ramp_is_one_undo_step_and_the_last_change_cannot_ramp() {
        let mut doc = doc();
        doc.put_tempo_change(Ticks::quarters(8), 60.0, None);
        doc.set_tempo_ramp(0, true);
        assert!(doc.project().tempo[0].ramp);
        doc.set_tempo_ramp(1, true);
        assert!(!doc.project().tempo[1].ramp, "no next change to ramp to");
        doc.set_tempo_ramp(0, false);
        assert!(!doc.project().tempo[0].ramp);
        assert_eq!(doc.undo().as_deref(), Some("Step tempo"));
        assert!(doc.project().tempo[0].ramp, "undo gives the ramp back");
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn the_time_signature_stays_one_a_project_can_hold() {
        let mut doc = doc();
        doc.set_time_signature_at(Ticks::ZERO, 7, 8, None);
        assert_eq!(signature(&doc, Ticks::ZERO), (7, 8));
        // Past what a project may hold, a saved song would not open again.
        doc.set_time_signature_at(Ticks::ZERO, 99, 4, Some("drag"));
        assert_eq!(signature(&doc, Ticks::ZERO), (64, 4));
        doc.set_time_signature_at(Ticks::ZERO, 0, 3, Some("drag"));
        assert_eq!(
            signature(&doc, Ticks::ZERO),
            (1, 4),
            "3 is no beat unit: 4 stays"
        );
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
        doc.undo();
        assert_eq!(
            signature(&doc, Ticks::ZERO),
            (7, 8),
            "the drag was one step"
        );
    }

    #[test]
    fn a_meter_change_is_added_on_a_bar_line_and_undone_in_one_step() {
        let mut doc = doc();
        assert!(doc.put_meter_change(Ticks::quarters(7) + Ticks(PPQ / 4), 7, 8));
        assert_eq!(doc.project().meter[1].at, Ticks::quarters(8));
        assert_eq!(signature(&doc, Ticks::quarters(9)), (7, 8));
        assert_eq!(signature(&doc, Ticks::quarters(4)), (4, 4));
        // The bar fields change the segment they are in.
        doc.set_time_signature_at(Ticks::quarters(9), 3, 4, None);
        assert_eq!(signature(&doc, Ticks::quarters(9)), (3, 4));
        assert_eq!(signature(&doc, Ticks::ZERO), (4, 4));
        doc.undo();
        doc.undo();
        assert_eq!(doc.project().meter.len(), 1);
    }

    #[test]
    fn changing_a_meter_moves_the_changes_after_it_in_the_same_step() {
        let mut doc = doc();
        doc.put_meter_change(Ticks::quarters(8), 3, 4);
        // 5/4 bars: the change at 8 moves to the nearest bar line, 10.
        doc.set_meter_change(0, 5, 4, None);
        assert_eq!(doc.project().meter[1].at, Ticks::quarters(10));
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
        doc.undo();
        assert_eq!(doc.project().meter[1].at, Ticks::quarters(8));
        assert_eq!(signature(&doc, Ticks::ZERO), (4, 4));
    }

    #[test]
    fn a_meter_change_moves_and_removes_leaving_the_first() {
        let mut doc = doc();
        doc.put_meter_change(Ticks::quarters(8), 3, 4);
        for q in [12, 16, 20] {
            doc.move_meter_change(1, Ticks::quarters(q), Some("drag"));
        }
        assert_eq!(doc.project().meter[1].at, Ticks::quarters(20));
        doc.undo();
        assert_eq!(doc.project().meter[1].at, Ticks::quarters(8), "one step");
        doc.remove_meter_change(0);
        assert_eq!(doc.project().meter.len(), 2, "the first stays");
        doc.remove_meter_change(1);
        assert_eq!(doc.project().meter.len(), 1);
    }
}
