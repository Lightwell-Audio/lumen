//! Recording MIDI (M5 slice 6c, ADR-0055): the app's side of Record.
//!
//! Record (F9) asks the engine to play from the play marker, after the
//! count-in, and to keep what armed tracks are sent. Each frame this drains
//! what the engine kept, turns it into takes (`recorder::Capture`) and
//! commits them to the document about once a second through
//! `Document::record_take`, so a recording is one undo step and an autosave
//! can hold a crashed take (autosave runs every ten seconds meanwhile).
//! Stopping, or Record again, ends it: held notes close at the stop and
//! the last commit makes the clip run exactly from where recording began to
//! where it ended.
//!
//! The take's range is where recording began (the playhead) to where the
//! playhead is (so a clip runs exactly start to stop), with Loop on too;
//! once the loop wraps it is the loop's whole region, and every pass layers
//! into it. Punch in and out (the brace's switches) mask the take's visible
//! range. Capture continues outside it until Stop or Record-off, keeping
//! the full performance as hidden clip content.

use std::time::{Duration, Instant};

use lumen_core::time::Ticks;
use lumen_engine::{Captured, Heard};
use lumen_project::edit::record::Mode;
use lumen_project::model::{Id, TrackKind};

use crate::App;
use crate::arrangement::recording_look::RecordingLook;
use crate::auto_record::Mode as AutoMode;
use crate::document::Recording;
use crate::played::Played;
use crate::recorder::Capture;

use super::audio_record::{AudioRecord, Lane};

/// How often a growing take is committed to the document.
const COMMIT_EVERY: Duration = Duration::from_secs(1);
/// Autosave's delay while recording: a crash costs seconds, not a minute.
pub(crate) const AUTOSAVE_DELAY: Duration = Duration::from_secs(10);
/// How long the engine has to start after Record before it is given up on
/// (it is not running).
const ENGINE_WAIT: Duration = Duration::from_secs(2);

/// What the app keeps for recording.
#[derive(Default)]
pub(crate) struct Record {
    /// What the engine kept, taken from the audio side once.
    queue: Option<rtrb::Consumer<Captured>>,
    session: Option<Session>,
    /// The audio half: files being closed after a recording (M9 slice R2).
    pub(super) audio: AudioRecord,
    /// What was played while not recording, for Capture MIDI (M9 slice R3).
    pub(super) played: Played,
    /// The document session the buffer belongs to: another one empties it.
    played_for: u64,
}

impl Record {
    /// Whether a recording is under way (or counting in).
    pub(crate) fn active(&self) -> bool {
        self.session.is_some()
    }
}

/// One recording, from Record to the stop.
struct Session {
    rec: Recording,
    capture: Capture,
    mode: Mode,
    /// The document it began in: another one ends it.
    document: u64,
    /// The armed MIDI tracks it records on.
    tracks: Vec<Id>,
    /// The armed audio tracks it records on, written by the take worker.
    audio: Vec<Lane>,
    /// The tracks it put in Latch because a controller is mapped to one of
    /// their controls, which go back to Read when it ends (M7 slice A5).
    latched: Vec<Id>,
    /// Where recording began: the playhead (or the play marker).
    start: Ticks,
    /// The loop region, when the take layers its passes.
    looped: Option<(Ticks, Ticks)>,
    /// The punch range that masks the take.
    punch: Option<(Ticks, Ticks)>,
    asked: Instant,
    /// The engine has reported recording.
    seen: bool,
    committed: Instant,
    /// Where the playhead was last frame, to see the loop wrap.
    last: Ticks,
    wrapped: bool,
}

impl Session {
    /// What the arrangement draws as being recorded.
    fn look(&self, head: Ticks, bar: Ticks) -> RecordingLook {
        let reached = self.end(head);
        let bar = bar.0.max(1);
        let from = self.punch.map_or(self.from(), |(a, _)| self.from().max(a));
        let mut until = Ticks((reached.0 / bar + 1) * bar);
        if let Some((_, end)) = self.punch {
            until = until.min(end);
        }
        if let Some((_, end)) = self.looped {
            until = until.min(end);
        }
        if reached <= from {
            until = from;
        }
        RecordingLook {
            tracks: self.tracks.clone(),
            from,
            until,
            overdub: self.mode == Mode::Overdub,
            take_clips: self
                .tracks
                .iter()
                .filter_map(|&track| self.rec.take_clip(track))
                .collect(),
        }
    }

    /// Where the visible take begins: no earlier than the loop start; after
    /// a wrap each pass covers the whole loop. Capture keeps earlier data.
    fn from(&self) -> Ticks {
        match self.looped {
            Some((start, _)) if self.wrapped => start,
            Some((start, _)) => self.start.max(start),
            _ => self.start,
        }
    }

    /// Where the take shows what is played (the punch and saved loop
    /// range), as the commits place it: only there does a key
    /// played win its pitch over the clips' notes in the engine. Played
    /// outside it, a key is kept hidden and the clips play on under it.
    fn window(&self) -> (Ticks, Ticks) {
        let from = self.punch.map_or(self.from(), |(a, _)| self.from().max(a));
        let mut until = self.punch.map_or(Ticks(i64::MAX), |(_, b)| b);
        if let Some((_, end)) = self.looped {
            until = until.min(end);
        }
        (from, until)
    }

    /// Each track's take as far as it has gone, the notes still held running
    /// to the playhead: what the arrangement draws on every frame.
    fn takes_so_far(&self, head: Ticks) -> Vec<(Id, lumen_project::edit::record::Take)> {
        let now = head;
        let range = (self.from(), self.end(head));
        self.tracks
            .iter()
            .map(|&track| (track, self.capture.so_far(track.0, range, now)))
            .collect()
    }

    /// Where the take ends now: the playhead bounded by the saved loop,
    /// or the whole loop once it has wrapped.
    fn end(&self, head: Ticks) -> Ticks {
        match self.looped {
            Some((_, end)) if self.wrapped => end,
            Some((_, end)) => head.max(self.start).min(end),
            _ => head.max(self.start),
        }
    }
}

impl App {
    /// The tracks a Replace take is recording over: what they held from the
    /// take's start on is not played, so the old notes do not sound under
    /// the new ones.
    pub(crate) fn replacing(&self) -> Vec<crate::midi_play::Replacing> {
        let Some(session) = self.record.session.as_ref() else {
            return Vec::new();
        };
        let from = session
            .punch
            .map_or(session.from(), |(a, _)| session.from().max(a));
        let until = session
            .punch
            .map(|(_, end)| end)
            .filter(|&end| end < Ticks(i64::MAX / 4))
            .or_else(|| session.looped.map(|(_, end)| end));
        // Recorded audio always goes in as a clip that cuts what it covers,
        // whatever the MIDI mode is, so the old audio is not played under it.
        let audio = session.audio.iter().map(|lane| lane.track);
        let midi = (session.mode == Mode::Replace)
            .then_some(session.tracks.iter().copied())
            .into_iter()
            .flatten();
        midi.map(|track| (track, session.rec.take_clip(track)))
            .chain(audio.map(|track| (track, None)))
            .map(|(track, take)| crate::midi_play::Replacing {
                track,
                from,
                take,
                until,
            })
            .collect()
    }

    /// Where the recording under way shows what is played (see
    /// `Session::window`), if one is.
    pub(crate) fn record_window(&self) -> Option<(Ticks, Ticks)> {
        self.record.session.as_ref().map(Session::window)
    }

    /// The capture coordinates used by the engine before the next UI frame.
    pub(crate) fn record_timing(&self) -> Option<lumen_engine::RecordTiming> {
        let session = self.record.session.as_ref()?;
        Some(lumen_engine::RecordTiming {
            start: session.start,
            latency_samples: self
                .transport
                .audio
                .info
                .as_ref()
                .map_or(0, |info| i64::from(info.output_latency)),
            looped: session.looped,
            punch: session.punch,
        })
    }

    /// Whether a recording is under way. Undo and redo wait for its end.
    pub(crate) fn is_recording(&self) -> bool {
        self.record.active()
    }

    /// Record (F9): starts recording on the armed MIDI tracks, or ends it
    /// (playback carries on; a count-in is cancelled).
    pub(crate) fn toggle_record(&mut self) {
        if self.record.active() {
            if self.transport.state().count_in_total > 0 {
                self.transport.stop();
            } else {
                self.transport.end_record();
            }
        } else {
            self.start_recording();
        }
    }

    fn start_recording(&mut self) {
        let project = self.document.project();
        let tracks: Vec<Id> = project
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Midi && t.armed)
            .map(|t| t.id)
            .collect();
        if tracks.is_empty() && !project.tracks.iter().any(|t| t.armed) {
            self.file_notice = Some("Arm a track to record.".into());
            return;
        }
        let state = self.transport.state();
        let region = project.transport;
        let settings = self.settings.record;
        let start = if state.playing {
            state.ticks
        } else {
            state.marker
        };
        let looped = (region.loop_enabled && region.loop_end > region.loop_start)
            .then_some((region.loop_start, region.loop_end))
            .filter(|&(_, end)| start < end);
        let punch = (region.punch_in || region.punch_out).then_some((
            if region.punch_in {
                region.loop_start
            } else {
                Ticks::ZERO
            },
            if region.punch_out {
                region.loop_end
            } else {
                Ticks(i64::MAX / 4)
            },
        ));
        let beats = if state.playing {
            0
        } else {
            settings
                .count_in
                .beats(project.meter_map().meter_at(start).numerator)
        };
        let audio = self.begin_audio_takes(punch, looped.is_some());
        if tracks.is_empty() && audio.is_empty() {
            return;
        }
        let now = Instant::now();
        let rec = self.document.begin_recording(&tracks);
        let latched = self.latch_mapped(&tracks);
        self.record.session = Some(Session {
            rec,
            capture: Capture::recording(looped, start),
            mode: if settings.overdub {
                Mode::Overdub
            } else {
                Mode::Replace
            },
            document: self.document.session(),
            tracks,
            audio,
            latched,
            start,
            looped,
            punch,
            asked: now,
            seen: false,
            committed: now - COMMIT_EVERY,
            last: start,
            wrapped: false,
        });
        if let Some(autosave) = self.autosave.as_mut() {
            autosave.set_delay(AUTOSAVE_DELAY);
        }
        self.transport.record(beats);
    }

    /// Every frame: records (see [`Self::step_recording`]) and tells the
    /// arrangement which clips are being recorded.
    pub(crate) fn update_recording(&mut self, ctx: &eframe::egui::Context) {
        self.step_recording();
        // While a take is written into a track, edits to that track's clips
        // would be undone by the next commit: they are refused, with a note.
        let recording: Vec<Id> = self
            .record
            .session
            .as_ref()
            .map_or_else(Vec::new, |session| {
                let audio = session.audio.iter().map(|lane| lane.track);
                session.tracks.iter().copied().chain(audio).collect()
            });
        self.document.guard_recording(&recording);
        self.poll_audio_takes(ctx);
        let lanes: Vec<Lane> = self
            .record
            .session
            .as_ref()
            .map_or_else(Vec::new, |session| session.audio.clone());
        self.watch_audio_takes(&lanes);
        self.arrangement.audio_takes = self.audio_take_looks(&lanes);
        let head = self.transport.state().ticks;
        let bar = self.document.project().meter_map().meter_at(head).bar();
        self.arrangement.recording = self
            .record
            .session
            .as_ref()
            .map(|session| session.look(head, bar));
        // The project as the next commit will make it, drawn on every frame:
        // the take and the clips it records over or into move with the
        // playhead, not once a commit (MT-MIDI-7).
        self.arrangement.preview = self
            .record
            .session
            .as_ref()
            .filter(|_| self.transport.state().playing && self.transport.state().recording)
            .map(|session| {
                std::sync::Arc::new(self.document.preview_takes(
                    &session.rec,
                    &session.takes_so_far(head),
                    session.mode,
                    session.punch,
                ))
            });
    }

    /// The take under way for tests: where its range begins and ends, and
    /// how many notes the first recording track has played (a held one
    /// counted). None when nothing records.
    pub(crate) fn take_so_far(&self) -> Option<(Ticks, Ticks, usize)> {
        let session = self.record.session.as_ref()?;
        self.arrangement.preview.as_ref()?;
        let head = self.transport.state().ticks;
        let takes = session.takes_so_far(head);
        let notes = takes.first().map_or(0, |(_, t)| t.notes.len());
        Some((session.from(), session.end(head), notes))
    }

    /// Keeps what the engine kept, commits the take when it is due, and ends
    /// the recording when the engine has stopped it.
    fn step_recording(&mut self) {
        if self.record.queue.is_none() {
            self.record.queue = self.transport.audio.midi.take_captured();
        }
        let snap = self.transport.state();
        // The metronome sounds only while recording, when so asked.
        self.transport
            .gate_metronome(self.settings.record.only_while_recording && !snap.recording);
        self.tidy_played();
        let Some(mut session) = self.record.session.take() else {
            self.keep_played_only();
            return;
        };
        if session.document != self.document.session() {
            // Another project was opened: nothing of this take belongs to it.
            self.abandon(session, true);
            return;
        }
        self.keep_captured(&mut session, snap.sample_rate);
        session.seen |= snap.recording;
        let head = snap.ticks;
        if snap.playing && snap.recording {
            if head < session.last && session.looped.is_some() {
                session.wrapped = true;
                // The next pass hears this one: commit what it played now.
                session.committed = Instant::now() - COMMIT_EVERY;
            }
            session.last = head;
        }
        let ended = session.seen && !snap.recording;
        let never_started = !session.seen && session.asked.elapsed() > ENGINE_WAIT;
        if ended || never_started {
            self.finish(session, head);
            return;
        }
        if snap.playing && snap.recording && session.committed.elapsed() >= COMMIT_EVERY {
            let end = session.end(head);
            if !self.commit(&mut session, end) {
                self.finish_blocked(session);
                return;
            }
            session.committed = Instant::now();
        }
        self.record.session = Some(session);
    }

    /// Empties the buffer of Capture MIDI when its setting is off or another
    /// project was opened.
    fn tidy_played(&mut self) {
        let document = self.document.session();
        let record = &mut self.record;
        if record.played_for != document || !self.settings.record.keep_played {
            record.played.clear();
            record.played_for = document;
        }
        record.played.forget(Instant::now());
    }

    /// With no recording, everything the engine kept is for Capture MIDI
    /// (or thrown away, when that is off).
    fn keep_played_only(&mut self) {
        let keep = self.settings.record.keep_played;
        let record = &mut self.record;
        let Some(queue) = record.queue.as_mut() else {
            return;
        };
        let now = Instant::now();
        while let Ok(captured) = queue.pop() {
            if keep && captured.state != Heard::Recording {
                record.played.push(&captured, now);
            }
        }
    }

    /// Turns what the engine kept into notes and events.
    fn keep_captured(&mut self, session: &mut Session, rate: u32) {
        let keep = self.settings.record.keep_played;
        let Some(queue) = self.record.queue.as_mut() else {
            return;
        };
        // The device's output latency, which the player heard the playhead
        // late by.
        let latency = self
            .transport
            .audio
            .info
            .as_ref()
            .map_or(0, |i| i64::from(i.output_latency));
        let timeline = &self.transport.timeline;
        let ticks = |sample: i64| timeline.ticks_at(sample, rate.max(1));
        let now = Instant::now();
        while let Ok(captured) = queue.pop() {
            if captured.state != Heard::Recording {
                // Played during the count-in: for Capture MIDI, not the take.
                if keep {
                    self.record.played.push(&captured, now);
                }
            } else if rate > 0 {
                session.capture.push(&captured, latency, &ticks);
            }
        }
    }

    /// Commits the take so far to the document, one undo step for the whole
    /// recording. False when a locked clip is in the way.
    fn commit(&mut self, session: &mut Session, end: Ticks) -> bool {
        for &track in &session.tracks {
            let take = session.capture.take(track.0, (session.from(), end));
            if self
                .document
                .record_take(&mut session.rec, track, &take, session.mode, session.punch)
                .is_err()
            {
                return false;
            }
        }
        true
    }

    /// The engine has stopped recording: closes held notes at the stop and
    /// commits the last take.
    fn finish(&mut self, mut session: Session, head: Ticks) {
        self.unlatch(&session.latched);
        self.end_audio_takes(std::mem::take(&mut session.audio), false);
        let end = session.end(head);
        // Notes still held end where the playhead is, in the pass it is in.
        // The visible loop window is a snapshot, but Loop can be switched
        // off during capture. A held note then ends at the actual head,
        // including any tail beyond the old loop end.
        session.capture.close_all(head);
        if !self.commit(&mut session, end) {
            self.file_notice = Some(
                "A locked clip is in the way of the take, so the end of it was not kept.".into(),
            );
        }
        self.restore_autosave();
    }

    /// A locked clip stopped the recording: it ends, and says why.
    fn finish_blocked(&mut self, session: Session) {
        self.unlatch(&session.latched);
        self.file_notice = Some(
            "Recording stopped: a locked clip is in the way. Unlock it, or record elsewhere."
                .into(),
        );
        self.transport.end_record();
        self.abandon(session, false);
    }

    /// Drops a recording without a last commit (what was committed stays).
    fn abandon(&mut self, mut session: Session, discard: bool) {
        self.unlatch(&session.latched);
        self.end_audio_takes(std::mem::take(&mut session.audio), discard);
        drop(session);
        self.transport.end_record();
        self.restore_autosave();
    }

    /// A track that records and has a controller mapped to one of its
    /// controls writes that control's moves as automation for the take: a
    /// track in Read goes to Latch (it starts at the first move and holds the
    /// last value to the stop). A track the user put in another mode is left
    /// as it is. Returns the tracks it changed.
    fn latch_mapped(&mut self, tracks: &[Id]) -> Vec<Id> {
        let mappings = &self.document.project().mappings;
        let changed: Vec<Id> = tracks
            .iter()
            .copied()
            .filter(|id| mappings.iter().any(|m| m.track == *id))
            .filter(|id| self.arrangement.auto.mode_of(*id) == AutoMode::Read)
            .collect();
        for &id in &changed {
            self.arrangement.auto.set_mode(id, AutoMode::Latch);
        }
        if !changed.is_empty() {
            let names: Vec<String> = changed
                .iter()
                .filter_map(|id| {
                    self.document
                        .project()
                        .tracks
                        .iter()
                        .find(|t| t.id == *id)
                        .map(|t| t.name.clone())
                })
                .collect();
            self.say(format!(
                "Automation is on for {} while you record (Latch)",
                names.join(", ")
            ));
        }
        changed
    }

    /// Puts the tracks [`Self::latch_mapped`] changed back in Read, unless the
    /// user chose another mode meanwhile.
    fn unlatch(&mut self, latched: &[Id]) {
        for &id in latched {
            if self.arrangement.auto.mode_of(id) == AutoMode::Latch {
                self.arrangement.auto.set_mode(id, AutoMode::Read);
            }
        }
    }

    fn restore_autosave(&mut self) {
        if let Some(autosave) = self.autosave.as_mut() {
            autosave.set_delay(crate::autosave::DELAY);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::midi::Msg;

    fn session(start: Ticks, looped: Option<(Ticks, Ticks)>) -> Session {
        let mut doc = crate::document::Document::new(crate::empty_project());
        Session {
            rec: doc.begin_recording(&[Id(1)]),
            capture: Capture::looping(looped),
            mode: Mode::Overdub,
            document: doc.session(),
            tracks: vec![Id(1)],
            audio: Vec::new(),
            latched: Vec::new(),
            start,
            looped,
            punch: None,
            asked: Instant::now(),
            seen: true,
            committed: Instant::now(),
            last: start,
            wrapped: false,
        }
    }

    #[test]
    fn recording_look_stays_inside_punch_and_loop_windows() {
        let mut s = session(Ticks::ZERO, None);
        s.punch = Some((Ticks(100), Ticks(250)));
        let before = s.look(Ticks(50), Ticks(400));
        assert_eq!((before.from, before.until), (Ticks(100), Ticks(100)));
        let after = s.look(Ticks(900), Ticks(400));
        assert_eq!((after.from, after.until), (Ticks(100), Ticks(250)));
        s.looped = Some((Ticks(150), Ticks(200)));
        s.wrapped = true;
        let looped = s.look(Ticks(180), Ticks(400));
        assert_eq!((looped.from, looped.until), (Ticks(150), Ticks(200)));
    }

    #[test]
    fn the_engine_window_is_the_punch_and_the_loop_once_wrapped() {
        let mut s = session(Ticks(50), None);
        assert_eq!(s.window(), (Ticks(50), Ticks(i64::MAX)));
        s.punch = Some((Ticks(100), Ticks(250)));
        assert_eq!(s.window(), (Ticks(100), Ticks(250)));
        s.punch = None;
        s.looped = Some((Ticks(20), Ticks(200)));
        assert_eq!(s.window(), (Ticks(50), Ticks(200)));
        s.wrapped = true;
        assert_eq!(s.window(), (Ticks(20), Ticks(200)));
        s.punch = Some((Ticks(30), Ticks(150)));
        assert_eq!(s.window(), (Ticks(30), Ticks(150)));
    }

    #[test]
    fn disabling_loop_before_a_wrap_keeps_the_saved_upper_boundary() {
        let s = session(Ticks(50), Some((Ticks(20), Ticks(200))));
        assert_eq!(s.end(Ticks(300)), Ticks(200));
        assert_eq!(s.window(), (Ticks(50), Ticks(200)));
        let look = s.look(Ticks(300), Ticks(100));
        assert_eq!((look.from, look.until), (Ticks(50), Ticks(200)));
    }

    #[test]
    fn a_wrapped_held_note_preview_ends_at_the_current_pass_head() {
        let mut s = session(Ticks(300), Some((Ticks(100), Ticks(500))));
        s.wrapped = true;
        s.capture.push(
            &Captured {
                track: 1,
                source: 0,
                at: 150,
                pass: 2,
                frame: 0,
                state: Heard::Recording,
                msg: Msg::note_on(0, 60, 90),
            },
            0,
            &Ticks,
        );
        let take = &s.takes_so_far(Ticks(200))[0].1;
        assert_eq!(take.notes[0].start, Ticks(150));
        assert_eq!(take.notes[0].length, Ticks(50));
    }
}
