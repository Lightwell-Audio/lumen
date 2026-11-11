//! The warp marker edits the interface asks for (M9.5 S5b): add, remove and
//! clear, one undo step each, the two commands, and the words the span chip
//! says. The arithmetic is `lumen_project::edit::warp_markers`.

use lumen_core::warp;
use lumen_project::edit::warp::{Refused, within};
use lumen_project::edit::warp_markers::MarkerEdit;
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
}
