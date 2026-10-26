//! Capture MIDI (M9 slice R3, ADR-0104): turns the notes last played on a
//! MIDI track, while not recording, into a clip. The buffer is
//! `played::Played`; `app/record.rs` fills it from the engine's queue.

use lumen_project::model::{ClipContent, Id, TrackKind};

use crate::App;
use crate::played::{self, Phrase};

impl App {
    /// The MIDI tracks to try, best first: the selected ones, then the armed
    /// ones, then the track played last.
    fn capture_candidates(&self) -> Vec<Id> {
        let project = self.document.project();
        let usable = |id: &Id| {
            project
                .tracks
                .iter()
                .any(|t| t.id == *id && t.kind == TrackKind::Midi && t.frozen.is_none())
        };
        let mut found: Vec<Id> = self.selected_midi_tracks();
        found.extend(
            project
                .tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Midi && t.armed)
                .map(|t| t.id),
        );
        found.extend(self.record.played.last_track().map(Id));
        found.retain(usable);
        let mut seen = Vec::new();
        found.retain(|id| {
            let new = !seen.contains(id);
            seen.push(*id);
            new
        });
        found
    }

    /// The phrase Capture MIDI would place, and the track it goes on.
    fn phrase_to_capture(&self) -> Option<(Id, Phrase)> {
        self.capture_candidates()
            .into_iter()
            .find_map(|id| self.record.played.newest(id.0).map(|p| (id, p)))
    }

    /// Whether Capture MIDI can run now, and why not if it cannot.
    pub(crate) fn capture_midi_ready(&self) -> Result<(), &'static str> {
        if self.record.active() {
            return Err("Not while recording");
        }
        if !self.settings.record.keep_played {
            return Err("Switch on \"Keep played notes for Capture MIDI\" first");
        }
        if self.phrase_to_capture().is_none() {
            return Err("Play some notes on a MIDI track first");
        }
        Ok(())
    }

    /// Places the newest phrase as a clip on its track.
    pub(crate) fn capture_midi(&mut self) {
        if let Err(why) = self.capture_midi_ready() {
            self.say(why.to_owned());
            return;
        }
        let Some((track, phrase)) = self.phrase_to_capture() else {
            return;
        };
        let snap = self.transport.state();
        let rate = snap.sample_rate.max(1);
        let latency = self
            .transport
            .audio
            .info
            .as_ref()
            .map_or(0, |i| i64::from(i.output_latency));
        let timeline = &self.transport.timeline;
        let playhead = timeline.sample_at(snap.ticks, rate);
        let ticks = |sample: i64| timeline.ticks_at(sample, rate);
        let Some(take) = played::take_of(&phrase, playhead, latency, &ticks, &timeline.meter)
        else {
            return;
        };
        let name = self.track_name(track);
        let bar = timeline.meter.bbt_at(take.start).bar + 1;
        match self.document.capture_midi(track, &take) {
            Ok(clip) => {
                self.record.played.remove(&phrase);
                let notes = self
                    .document
                    .project()
                    .tracks
                    .iter()
                    .flat_map(|t| t.clips.iter())
                    .find(|c| c.id == clip)
                    .and_then(|c| match &c.content {
                        ClipContent::Midi { notes, .. } => Some(notes.len()),
                        ClipContent::Audio { .. } => None,
                    });
                self.arrangement.clips = std::iter::once(clip).collect();
                self.arrangement.clip_cursor = None;
                let words = notes.map_or_else(String::new, |n| {
                    format!("{n} {} ", if n == 1 { "note" } else { "notes" })
                });
                self.say(format!(
                    "Captured {words}on {name} at bar {bar}. Undo takes it away."
                ));
            }
            Err(_) => self.say(format!(
                "A locked clip on {name} is in the way, so nothing was captured"
            )),
        }
    }
}
