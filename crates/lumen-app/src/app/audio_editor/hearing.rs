//! Hearing a clip while it is edited (M9.5 S2c, LC A): Loop clip plays one
//! pass of the opened audio clip, or the range picked on it, on repeat with
//! the other tracks heard. It overrides what the engine loops and nothing
//! more: the project's loop is not touched, nothing is saved, and there is
//! nothing to undo.

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_engine::Region;
use lumen_project::edit::clip_loop;
use lumen_project::model::{Clip, Id};

use crate::App;
use crate::audio_editor::AudioEditor;

/// One pass of `clip`, in the song: from its start for its length, or for
/// one turn of its loop when that is shorter, so the pass holds every frame
/// of the loop even when the clip starts part way through it.
pub(super) fn pass_of(tempo: &lumen_core::time::TempoMap, clip: &Clip) -> (Ticks, Ticks) {
    let start = clip.start;
    let end = clip.start + clip.length;
    let end = match clip_loop::turn(tempo, clip) {
        Some(turn) => end.min(tempo.ticks_at_seconds(tempo.seconds_at(start) + turn.seconds)),
        None => end,
    };
    (start, end.max(start))
}

/// What plays on repeat: the range where it overlaps the pass, else the
/// pass.
pub(super) fn looped_part(pass: (Ticks, Ticks), range: Option<(Ticks, Ticks)>) -> (Ticks, Ticks) {
    match range {
        Some((from, to)) => {
            let (from, to) = (from.max(pass.0), to.min(pass.1));
            if to > from { (from, to) } else { pass }
        }
        None => pass,
    }
}

impl App {
    /// The part of clip `id` that Loop clip repeats, in the song.
    fn clip_loop_part(&self, id: Id) -> Option<(Ticks, Ticks)> {
        let clip = self.shown_clip(id)?;
        let pass = pass_of(&self.transport.timeline.tempo, &clip);
        // Only a range that includes the clip's own track is "picked on it".
        let track = self.clip_track_index(id).map(|(_, track)| track);
        let range = self
            .arrangement
            .range
            .as_ref()
            .filter(|r| track.is_some_and(|t| r.tracks.contains(&t)))
            .map(|r| (r.start, r.end));
        Some(looped_part(pass, range))
    }

    /// Loop clip on or off for the opened clip. Turning it on moves the
    /// playhead to the start of what repeats when it is outside, and does
    /// not start playback.
    pub(crate) fn toggle_loop_clip(&mut self) {
        if self.audio_editor.loop_clip.take().is_some() {
            return;
        }
        let Some(id) = self.detail_audio_clip() else {
            return;
        };
        if self.is_recording() {
            return;
        }
        let Some((from, to)) = self.clip_loop_part(id) else {
            return;
        };
        self.audio_editor.loop_clip = Some(id);
        let head = self.transport.position();
        if head < from || head >= to {
            self.transport.locate(from);
        }
    }

    /// Once a frame, before the engine is told its loop: keeps the override
    /// in step with the clip and the range, and drops it when another clip
    /// opens, the editor is hidden (the panel closed or on another tab, with no
    /// window of its own) or recording starts.
    pub(crate) fn sync_clip_loop(&mut self) {
        let shown = self.detail.audio_popped
            || (self.show_detail && self.detail_tab == crate::app::detail::DetailTab::Clip);
        let wanted = self
            .audio_editor
            .loop_clip
            .filter(|id| shown && self.detail_audio_clip() == Some(*id) && !self.is_recording());
        let part = wanted.and_then(|id| self.clip_loop_part(id));
        if part.is_none() {
            self.audio_editor.loop_clip = None;
        }
        self.transport
            .set_clip_loop(part.map(|(from, to)| Region::new(from, to, true)));
    }
}

impl App {
    /// The editor ruler's pointer (D22): a click moves the playhead; a drag
    /// listens as the arrangement's strip does (the grid step under the
    /// pointer on repeat, or tape with Alt), and while playing jumps along
    /// from step to step. The first move of a drag pauses Follow, so the view
    /// does not run away under the hand.
    pub(super) fn editor_scrub(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        ed: &mut AudioEditor,
        origin: f32,
    ) {
        use lumen_engine::ScrubStyle;
        let free = ctx.input(|i| i.modifiers.alt);
        let playing = self.transport.state().playing;
        ed.scrub_drawn = true;
        let zoom = ed.time.zoom;
        let at = |p: egui::Pos2| super::view::ticks_at(ed.time, origin, p.x).max(Ticks::ZERO);
        if response.dragged()
            && let Some(p) = response.interact_pointer_pos()
        {
            self.transport.follow_paused = true;
            let raw = at(p);
            let next = if playing {
                let target = self.snapped_at_zoom(raw, free, zoom);
                if ed.scrub_sent.is_none_or(|(sent, _)| sent != target) {
                    self.transport.locate(target);
                }
                (target, ScrubStyle::Tape)
            } else if free {
                if ed.scrub_sent.is_none_or(|(sent, _)| sent != raw) {
                    self.transport.scrub(raw, ScrubStyle::Tape);
                }
                (raw, ScrubStyle::Tape)
            } else {
                let (start, end) = self.grid_cell_at_zoom(raw, zoom);
                let style = ScrubStyle::Loop {
                    length: end - start,
                };
                if ed.scrub_sent != Some((start, style)) {
                    self.transport.scrub(start, style);
                }
                (start, style)
            };
            ed.scrub_sent = Some(next);
            ctx.request_repaint();
        } else if response.drag_stopped() || ed.scrub_sent.is_some() {
            // A scrub that is not being dragged by this ruler ends: the
            // release, or another clip's ruler (a new widget) drawn under a
            // held pointer.
            if !playing && ed.scrub_sent.take().is_some() {
                self.transport.end_scrub();
            }
            ed.scrub_sent = None;
        } else if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let target = self.snapped_at_zoom(at(p), free, zoom);
            self.transport.locate(target);
            self.transport.follow_paused = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(q: i64) -> Ticks {
        Ticks::quarters(q)
    }

    #[test]
    fn a_range_that_overlaps_the_pass_is_what_repeats_and_one_that_misses_it_is_not() {
        let pass = (t(4), t(12));
        assert_eq!(looped_part(pass, None), pass);
        assert_eq!(looped_part(pass, Some((t(6), t(9)))), (t(6), t(9)));
        assert_eq!(
            looped_part(pass, Some((t(2), t(6)))),
            (t(4), t(6)),
            "cut to it"
        );
        assert_eq!(looped_part(pass, Some((t(20), t(24)))), pass, "elsewhere");
    }
}
