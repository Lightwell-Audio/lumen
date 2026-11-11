//! The warp marker edits the interface asks for (M9.5 S5b): add, remove,
//! clear and fix a hit, one undo step each, the commands, and the words the
//! span chip says. The arithmetic is `lumen_project::edit::warp_markers`.

use lumen_core::warp;
use lumen_media::detect::Analysis;
use lumen_project::edit::warp::{Refused, within};
use lumen_project::edit::warp_markers::{MarkerEdit, fix_hit};
use lumen_project::model::{Clip, Id};

use super::OUTSIDE_FILE;
use crate::App;
use crate::audio_editor::AudioEditor;

/// The percentage a span plays at, as it reads: "112%".
fn percent(ratio: f64) -> String {
    format!("{:.0}%", ratio * 100.0)
}

/// What the span chip says for marker `index` of `clip`: "Before 112% · after
/// 94%", with only the side a marker has.
pub(crate) fn span_words(clip: &Clip, index: usize) -> Option<String> {
    let w = clip.warp.as_ref()?;
    let (before, after) = warp::ratios_beside(&w.markers, w.original_bpm, index);
    Some(match (before, after) {
        (Some(a), Some(b)) => format!("Before {} \u{00B7} after {}", percent(a), percent(b)),
        (Some(a), None) => format!("Before {}", percent(a)),
        (None, Some(b)) => format!("After {}", percent(b)),
        (None, None) => "Only marker".to_owned(),
    })
}

impl App {
    /// Adds a marker (a ghost's second, or the cursor's): one undo step.
    /// The new marker is picked and takes the keyboard.
    pub(crate) fn add_warp_marker(&mut self, id: Id, change: MarkerEdit, ed: &mut AudioEditor) {
        let base = self.document.project().clone();
        match self.document.edit_warp_marker(&base, id, change, None) {
            Ok(done) => {
                if let Some(i) = done.index {
                    ed.markers.picked = Some(i);
                    ed.markers.reveal = Some(i);
                    ed.markers.focus_next = Some(i);
                }
            }
            Err(Refused::Unchanged) => {
                self.say("There is a warp marker here already".to_owned());
            }
            Err(_) => self.say(format!("Can't add a warp marker: {OUTSIDE_FILE}")),
        }
    }

    /// Deletes marker `index`. The keyboard goes to the one before it.
    pub(crate) fn remove_warp_marker(&mut self, id: Id, index: usize, ed: &mut AudioEditor) {
        let base = self.document.project().clone();
        match self
            .document
            .edit_warp_marker(&base, id, MarkerEdit::Remove(index), None)
        {
            Ok(_) => {
                ed.markers.picked = None;
                ed.markers.focus_next = index.checked_sub(1).or(Some(0));
            }
            Err(_) => self.say("A warped clip keeps at least one warp marker".to_owned()),
        }
    }

    /// Removes every warp marker but one: the clip plays at the file's tempo.
    pub(crate) fn clear_warp_markers(&mut self, id: Id, ed: &mut AudioEditor) {
        let base = self.document.project().clone();
        if self
            .document
            .edit_warp_marker(&base, id, MarkerEdit::Clear, None)
            .is_ok()
        {
            ed.markers.picked = None;
        }
    }

    /// The Add warp marker command: a marker at the edit cursor of the opened
    /// clip, the playhead when there is none.
    pub(crate) fn add_warp_marker_command(&mut self) {
        let Some(id) = self.detail_audio_clip() else {
            return;
        };
        if !self.opened_clip_takes_edits(id) {
            return;
        }
        let Some(clip) = self.shown_clip(id) else {
            return;
        };
        let mut ed = std::mem::take(&mut self.audio_editor);
        let at = ed.cursor.unwrap_or_else(|| self.transport.playhead());
        self.select_opened_clip(id);
        // A cursor in a later repeat of a loop is the same place in the first.
        self.add_warp_marker(id, MarkerEdit::Add(within(&clip, at)), &mut ed);
        self.audio_editor = ed;
    }

    /// The Remove all warp markers command, for the opened clip.
    pub(crate) fn clear_warp_markers_command(&mut self) {
        let Some(id) = self.detail_audio_clip() else {
            return;
        };
        if !self.opened_clip_takes_edits(id) {
            return;
        }
        let mut ed = std::mem::take(&mut self.audio_editor);
        self.select_opened_clip(id);
        self.clear_warp_markers(id, &mut ed);
        self.audio_editor = ed;
    }

    /// Why a marker command cannot run, if it cannot: the opened clip is
    /// warped.
    pub(crate) fn markers_unavailable(&self) -> Result<(), &'static str> {
        let Some(id) = self.detail_audio_clip() else {
            return Err("Open a warped audio clip in the editor first");
        };
        match self.clip(id) {
            Some(clip) if clip.warp.is_some() => Ok(()),
            _ => Err("Warp the clip first"),
        }
    }

    /// "Fix this hit": pins the hits either side of `source` and adds a marker
    /// on it, one undo step, so only this hit moves when its marker is dragged
    /// (M9.5 review M2, the owner's C). The hit's marker takes the keyboard, as
    /// a new marker does, so the nudge keys move it on.
    pub(crate) fn fix_warp_hit(
        &mut self,
        id: Id,
        source: f64,
        sides: (Option<f64>, Option<f64>),
        ed: &mut AudioEditor,
    ) {
        match self.document.fix_warp_hit(id, source, sides) {
            Ok(done) => {
                if let Some(index) = done.index {
                    ed.markers.picked = Some(index);
                    ed.markers.reveal = Some(index);
                    ed.markers.focus_next = Some(index);
                    ed.markers.keyboard = Some(index);
                }
            }
            Err(Refused::Unchanged) => self.say(
                "Nothing to fix at this hit: the markers either side hold it already".to_owned(),
            ),
            Err(_) => {
                self.say("That hit can't be fixed: the clip's map would leave its file".to_owned());
            }
        }
    }

    /// The Fix this hit command: the focused warp marker's own hit, or the
    /// transient nearest the edit cursor in the opened clip.
    pub(crate) fn fix_warp_hit_command(&mut self) {
        let Ok(plan) = self.fix_hit_plan() else {
            return;
        };
        let mut ed = std::mem::take(&mut self.audio_editor);
        self.select_opened_clip(plan.clip);
        self.fix_warp_hit(plan.clip, plan.source, plan.sides, &mut ed);
        self.audio_editor = ed;
    }

    /// Why Fix this hit cannot run, if it cannot.
    pub(crate) fn fix_warp_hit_unavailable(&self) -> Result<(), &'static str> {
        self.fix_hit_plan().map(|_| ())
    }

    /// Where Fix this hit would act: the opened clip, the hit's second of the
    /// file, and the transients either side of it. `Err` says why it cannot.
    pub(crate) fn fix_hit_plan(&self) -> Result<HitFix, &'static str> {
        let Some(id) = self.detail_audio_clip() else {
            return Err("Open a warped audio clip in the editor first");
        };
        let Some(clip) = self.shown_clip(id) else {
            return Err("Open a warped audio clip in the editor first");
        };
        let Some(warp) = clip.warp.as_ref() else {
            return Err("Warp the clip first");
        };
        if clip.locked {
            return Err("Unlock the clip first");
        }
        let found = self.found_for(&clip);
        let Some(seen) = found.as_ref().map(|(_, a, s)| (&**a, *s)) else {
            return Err("Wait for the clip's hits to be found");
        };
        let source = match self
            .audio_editor
            .markers
            .keyboard
            .and_then(|index| warp.markers.get(index))
        {
            Some(marker) => marker.source,
            None => {
                let at = self
                    .audio_editor
                    .cursor
                    .ok_or("Put the edit cursor on a hit, or focus a warp marker")?;
                let heard = warp.source_at(within(&clip, at));
                let (analysis, sensitivity) = seen;
                analysis
                    .nearest(heard, sensitivity, analysis.seconds)
                    .map(|t| t.seconds)
                    .ok_or("Put the edit cursor on a hit, or focus a warp marker")?
            }
        };
        let sides = fix_target(&clip, source, Some(seen))
            .ok_or("Nothing to fix at this hit: its markers are all there")?;
        Ok(HitFix {
            clip: id,
            source,
            sides,
        })
    }
}

/// What Fix this hit would do: the clip it acts on, the hit's second of the
/// file, and the transients either side of it.
pub(crate) struct HitFix {
    pub clip: Id,
    pub source: f64,
    pub sides: (Option<f64>, Option<f64>),
}

/// The transients either side of the hit at `source` seconds of the file, at
/// `sensitivity`: the nearest below and the nearest above (either is absent
/// at the file's first or last hit).
fn sides(analysis: &Analysis, sensitivity: u8, source: f64) -> (Option<f64>, Option<f64>) {
    let least = lumen_media::detect::threshold(sensitivity);
    let (mut before, mut after) = (None, None);
    for t in &analysis.transients {
        if t.strength < least {
            continue;
        }
        if t.seconds < source {
            before = Some(t.seconds);
        } else if t.seconds > source {
            after = Some(t.seconds);
            break;
        }
    }
    (before, after)
}

/// What "Fix this hit" would pass for `clip`'s hit at `source`: the transients
/// either side of it, when the edit would add a marker. `None` when the clip
/// is locked or not warped, no findings are in, or every marker it would add
/// stands already: the menu lists no row for it, and the command cannot run.
pub(super) fn fix_target(
    clip: &Clip,
    source: f64,
    found: Option<(&Analysis, u8)>,
) -> Option<(Option<f64>, Option<f64>)> {
    if clip.locked {
        return None;
    }
    let (analysis, sensitivity) = found?;
    let sides = sides(analysis, sensitivity, source);
    fix_hit(clip, source, sides).ok()?;
    Some(sides)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::time::Ticks;
    use lumen_project::model::Warp;

    fn warped(markers: &[(f64, i64)]) -> Clip {
        use lumen_project::model::{ClipContent, Id};
        let mut warp = Warp::new(120.0, 0.0);
        warp.markers = markers
            .iter()
            .map(|(s, q)| warp::Marker::new(*s, Ticks::quarters(*q)))
            .collect();
        Clip {
            id: Id(1),
            name: "Loop".into(),
            start: Ticks::ZERO,
            length: Ticks::quarters(8),
            muted: false,
            color: None,
            locked: false,
            crossfade: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            takes: None,
            warp: Some(warp),
            content: ClipContent::Audio {
                media: Id(9),
                offset: 0,
                gain_db: 0.0,
            },
        }
    }

    #[test]
    fn the_chip_says_the_speed_of_each_side_as_a_percentage() {
        // Half a second in a beat is the file's own speed at 120 BPM.
        let clip = warped(&[(0.0, 0), (0.5, 1), (1.5, 2)]);
        assert_eq!(
            span_words(&clip, 1).as_deref(),
            Some("Before 100% \u{00B7} after 200%")
        );
        assert_eq!(span_words(&clip, 0).as_deref(), Some("After 100%"));
        assert_eq!(span_words(&clip, 2).as_deref(), Some("Before 200%"));
        assert_eq!(
            span_words(&warped(&[(0.0, 0)]), 0).as_deref(),
            Some("Only marker")
        );
    }

    #[test]
    fn a_hits_neighbours_are_the_nearest_hits_either_side_of_it() {
        use lumen_media::detect::Transient;
        let at = |seconds: f64, strength: f32| Transient { seconds, strength };
        let found = Analysis {
            seconds: 5.0,
            transients: vec![at(0.5, 0.9), at(1.0, 0.9), at(2.0, 0.9), at(2.5, 0.9)],
            tempo: None,
        };
        assert_eq!(
            sides(&found, 100, 2.0),
            (Some(1.0), Some(2.5)),
            "the nearest below and above"
        );
        assert_eq!(sides(&found, 100, 0.2), (None, Some(0.5)), "the first hit");
        assert_eq!(sides(&found, 100, 9.0), (Some(2.5), None), "the last hit");
        // A hit under the sensitivity is not offered, as it is not on the
        // strip either.
        let weak = Analysis {
            seconds: 5.0,
            transients: vec![at(1.0, 0.3), at(2.0, 0.9), at(3.0, 0.3)],
            tempo: None,
        };
        assert_eq!(sides(&weak, 50, 2.0), (None, None));
        assert_eq!(sides(&weak, 100, 2.0), (Some(1.0), Some(3.0)));
    }
}
