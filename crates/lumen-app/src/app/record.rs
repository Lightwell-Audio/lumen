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
//! The take's range is its start to where the playhead was (so a clip runs
//! exactly start to stop); with Loop on it is the loop's whole region, and
//! every pass layers into it. Punch in and out (the brace's switches) cut
//! the take, and punch out ends recording at the brace's end while playback
//! carries on.

use std::time::{Duration, Instant};

use lumen_core::time::Ticks;
use lumen_engine::Captured;
use lumen_project::edit::record::Mode;
use lumen_project::model::{Id, TrackKind};

use crate::App;
use crate::arrangement::live_take::{LiveNote, LiveTake, LiveTrack};
use crate::arrangement::recording_look::RecordingLook;
use crate::document::Recording;
use crate::recorder::Capture;

/// How often a growing take is committed to the document.
const COMMIT_EVERY: Duration = Duration::from_secs(1);
/// Autosave's delay while recording: a crash costs seconds, not a minute.
const AUTOSAVE_DELAY: Duration = Duration::from_secs(10);
/// How long the engine has to start after Record before it is given up on
/// (it is not running).
const ENGINE_WAIT: Duration = Duration::from_secs(2);

/// What the app keeps for recording.
#[derive(Default)]
pub(crate) struct Record {
    /// What the engine kept, taken from the audio side once.
    queue: Option<rtrb::Consumer<Captured>>,
    session: Option<Session>,
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
    /// Where the take begins.
    start: Ticks,
    /// The loop region, when the take layers its passes.
    looped: Option<(Ticks, Ticks)>,
    /// The punch range that cuts the take.
    punch: Option<(Ticks, Ticks)>,
    asked: Instant,
    /// The engine has reported recording.
    seen: bool,
    committed: Instant,
    /// Where the playhead was last frame, to see the loop wrap.
    last: Ticks,
    wrapped: bool,
    /// Punch out has been told to the engine.
    punched_out: bool,
}

impl Session {
    /// What the arrangement draws as being recorded.
    fn look(&self, head: Ticks, bar: Ticks) -> RecordingLook {
        let reached = self.end(head);
        let bar = bar.0.max(1);
        RecordingLook {
            tracks: self.tracks.clone(),
            from: self.start,
            until: Ticks((reached.0 / bar + 1) * bar),
            overdub: self.mode == Mode::Overdub,
            take_clips: self
                .tracks
                .iter()
                .filter_map(|&track| self.rec.take_clip(track))
                .collect(),
        }
    }

    /// The take as far as it has gone, for the arrangement to draw every
    /// frame: the notes played (a held one up to the playhead) and, in
    /// Overdub, where it is over no clip.
    fn live(&self, head: Ticks) -> LiveTake {
        let now = head.max(self.start);
        let (from, to) = (self.start, self.end(head));
        // Punch cuts what is kept, so what is drawn.
        let (from, to) = match self.punch {
            Some((a, b)) => (from.max(a), to.min(b)),
            None => (from, to),
        };
        let overdub = self.mode == Mode::Overdub;
        let tracks = self
            .tracks
            .iter()
            .map(|&track| {
                let take = self.capture.so_far(track.0, (self.start, to), now);
                let notes = take
                    .notes
                    .iter()
                    .filter(|n| n.start + n.length > from && n.start < to.max(now))
                    .map(|n| LiveNote {
                        start: n.start,
                        end: n.start + n.length,
                        pitch: n.pitch,
                        velocity: n.velocity,
                    })
                    .collect();
                let gaps = match self.rec.base_track(track) {
                    Some(base) if overdub && to > from => {
                        lumen_project::edit::record::gaps(base, (from, to))
                    }
                    _ => Vec::new(),
                };
                LiveTrack { track, notes, gaps }
            })
            .collect();
        LiveTake {
            from,
            to: to.max(from),
            overdub,
            tracks,
        }
    }

    /// Where the take ends now: the playhead, or the whole loop once it has
    /// wrapped.
    fn end(&self, head: Ticks) -> Ticks {
        match self.looped {
            Some((_, end)) if self.wrapped => end,
            _ => head.max(self.start),
        }
    }
}

impl App {
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
        if tracks.is_empty() {
            self.file_notice = Some("Arm a MIDI track to record.".into());
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
        let begin = match looped {
            Some((from, _)) if start >= from => from,
            _ => start,
        };
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
                .beats(u32::from(project.time_signature.numerator))
        };
        let now = Instant::now();
        let rec = self.document.begin_recording();
        self.record.session = Some(Session {
            rec,
            capture: Capture::looping(looped),
            mode: if settings.overdub {
                Mode::Overdub
            } else {
                Mode::Replace
            },
            document: self.document.session(),
            tracks,
            start: begin,
            looped,
            punch,
            asked: now,
            seen: false,
            committed: now - COMMIT_EVERY,
            last: begin,
            wrapped: false,
            punched_out: false,
        });
        if let Some(autosave) = self.autosave.as_mut() {
            autosave.set_delay(AUTOSAVE_DELAY);
        }
        self.transport.record(beats);
    }

    /// Every frame: records (see [`Self::step_recording`]) and tells the
    /// arrangement which clips are being recorded.
    pub(crate) fn update_recording(&mut self) {
        self.step_recording();
        let head = self.transport.state().ticks;
        let signature = self.document.project().time_signature;
        let bar = lumen_core::meter::Meter {
            numerator: u32::from(signature.numerator),
            denominator: u32::from(signature.denominator),
        }
        .bar();
        self.arrangement.recording = self
            .record
            .session
            .as_ref()
            .map(|session| session.look(head, bar));
        self.arrangement.live_take = self
            .record
            .session
            .as_ref()
            .filter(|_| self.transport.state().playing)
            .map(|session| session.live(head));
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
        let Some(mut session) = self.record.session.take() else {
            if let Some(queue) = self.record.queue.as_mut() {
                while queue.pop().is_ok() {}
            }
            return;
        };
        if session.document != self.document.session() {
            // Another project was opened: nothing of this take belongs to it.
            self.abandon(session);
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
            self.punch_out(&mut session, head);
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

    /// Turns what the engine kept into notes and events.
    fn keep_captured(&mut self, session: &mut Session, rate: u32) {
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
        while let Ok(captured) = queue.pop() {
            if rate > 0 {
                session.capture.push(&captured, latency, &ticks);
            }
        }
    }

    /// Punch out ends recording at the brace's end; playback carries on.
    fn punch_out(&mut self, session: &mut Session, head: Ticks) {
        let region = self.document.project().transport;
        if region.punch_out && !session.punched_out && (head >= region.loop_end || session.wrapped)
        {
            session.punched_out = true;
            self.transport.end_record();
        }
    }

    /// Commits the take so far to the document, one undo step for the whole
    /// recording. False when a locked clip is in the way.
    fn commit(&mut self, session: &mut Session, end: Ticks) -> bool {
        for &track in &session.tracks {
            let take = session.capture.take(track.0, (session.start, end));
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
        let end = session.end(head);
        // Notes still held end where the playhead is, in the pass it is in.
        let held_until = match session.looped {
            Some((from, to)) if session.wrapped => head.clamp(from, to),
            _ => end,
        };
        session.capture.close_all(held_until);
        if !self.commit(&mut session, end) {
            self.file_notice = Some(
                "A locked clip is in the way of the take, so the end of it was not kept.".into(),
            );
        }
        self.restore_autosave();
    }

    /// A locked clip stopped the recording: it ends, and says why.
    fn finish_blocked(&mut self, session: Session) {
        self.file_notice = Some(
            "Recording stopped: a locked clip is in the way. Unlock it, or record elsewhere."
                .into(),
        );
        self.transport.end_record();
        self.abandon(session);
    }

    /// Drops a recording without a last commit (what was committed stays).
    fn abandon(&mut self, session: Session) {
        drop(session);
        self.transport.end_record();
        self.restore_autosave();
    }

    fn restore_autosave(&mut self) {
        if let Some(autosave) = self.autosave.as_mut() {
            autosave.set_delay(crate::autosave::DELAY);
        }
    }
}
