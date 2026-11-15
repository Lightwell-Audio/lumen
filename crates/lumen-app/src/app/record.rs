use std::time::{Duration, Instant};

use lumen_core::time::Ticks;
use lumen_engine::{Captured, Heard};
use lumen_project::edit::record::Mode;
use lumen_project::model::{Id, TrackKind};

use crate::App;
use crate::arrangement::recording_look::RecordingLook;
use crate::document::Recording;
use crate::played::Played;
use crate::recording::auto_record::Mode as AutoMode;
use crate::recording::record_span::{self, Kind, Span};
use crate::recording::recorder::Capture;

use super::audio_record::{AudioRecord, Lane};

const COMMIT_EVERY: Duration = Duration::from_secs(1);
pub(crate) const AUTOSAVE_DELAY: Duration = Duration::from_secs(10);
const ENGINE_WAIT: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct Record {
    queue: Option<rtrb::Consumer<Captured>>,
    session: Option<Session>,
    pub(super) audio: AudioRecord,
    pub(crate) played: Played,
    played_for: u64,
}

impl Record {
    pub(crate) fn active(&self) -> bool {
        self.session.is_some()
    }
}

struct Session {
    rec: Recording,
    capture: Capture,
    mode: Mode,
    document: u64,
    tracks: Vec<Id>,
    audio: Vec<Lane>,
    latched: Vec<Id>,
    start: Ticks,
    looped: Option<(Ticks, Ticks)>,
    punch: Option<(Ticks, Ticks)>,
    asked: Instant,
    seen: bool,
    committed: Instant,
    last: Ticks,
    wrapped: bool,
}

impl Session {
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

    fn from(&self) -> Ticks {
        match self.looped {
            Some((start, _)) if self.wrapped => start,
            Some((start, _)) => self.start.max(start),
            _ => self.start,
        }
    }

    fn window(&self) -> (Ticks, Ticks) {
        let from = self.punch.map_or(self.from(), |(a, _)| self.from().max(a));
        let mut until = self.punch.map_or(Ticks(i64::MAX), |(_, b)| b);
        if let Some((_, end)) = self.looped {
            until = until.min(end);
        }
        (from, until)
    }

    fn takes_so_far(&self, head: Ticks) -> Vec<(Id, lumen_project::edit::record::Take)> {
        let now = head;
        let range = (self.from(), self.end(head));
        self.tracks
            .iter()
            .map(|&track| (track, self.capture.so_far(track.0, range, now)))
            .collect()
    }

    fn end(&self, head: Ticks) -> Ticks {
        match self.looped {
            Some((_, end)) if self.wrapped => end,
            Some((_, end)) => head.max(self.start).min(end),
            _ => head.max(self.start),
        }
    }
}

impl Session {
    fn span(&self, kind: Kind) -> Span {
        record_span::span(kind, self.start, self.looped, self.punch)
    }

    fn recorded_over(&self, head: Ticks) -> Vec<crate::midi::play::Replacing> {
        let wrapped = self.wrapped || (self.looped.is_some() && head < self.last);
        let Some(span) = self.span(Kind::Audio).recorded(self.start, head, wrapped) else {
            return Vec::new();
        };
        self.audio
            .iter()
            .map(|lane| replacing(lane.track, span))
            .collect()
    }
}

fn replacing(track: Id, span: Span) -> crate::midi::play::Replacing {
    crate::midi::play::Replacing {
        track,
        from: span.from,
        until: span.until,
    }
}

impl App {
    pub(crate) fn replacing(&self) -> Vec<crate::midi::play::Replacing> {
        let Some(session) = self.record.session.as_ref() else {
            return self.finishing_replaced();
        };
        let audio = session.audio.iter().map(|lane| (lane.track, Kind::Audio));
        let midi = (session.mode == Mode::Replace)
            .then_some(session.tracks.iter().map(|&track| (track, Kind::Midi)))
            .into_iter()
            .flatten();
        midi.chain(audio)
            .map(|(track, kind)| replacing(track, session.span(kind)))
            .collect()
    }

    pub(crate) fn record_window(&self) -> Option<(Ticks, Ticks)> {
        self.record.session.as_ref().map(Session::window)
    }

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

    pub(crate) fn is_recording(&self) -> bool {
        self.record.active()
    }

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
        let audio = self.begin_audio_takes(punch, looped);
        if tracks.is_empty() && audio.is_empty() {
            return;
        }
        let now = Instant::now();
        let rec = self.document.begin_recording(&tracks);
        let latched = self.latch_mapped(&tracks);
        let mut capture = Capture::recording(looped, start);
        for &track in &tracks {
            let map = self
                .document
                .project()
                .tracks
                .iter()
                .find(|t| t.id == track)
                .and_then(|t| self.document.project().drum_map_of(t));
            capture.set_store_map(track.0, map);
        }
        self.record.session = Some(Session {
            rec,
            capture,
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
            autosave.hurry(AUTOSAVE_DELAY);
        }
        let inputs = self.listen_inputs();
        let rate = self.lanes.rate();
        self.lanes.send_clips(
            &self.document,
            &mut self.transport.audio.clips,
            rate,
            &inputs,
        );
        self.audio_editor.loop_clip = None;
        self.sync_clip_loop();
        self.transport
            .follow_loop(&self.document.project().transport);
        self.transport.record(beats);
    }

    pub(crate) fn update_recording(&mut self, ctx: &eframe::egui::Context) {
        self.step_recording();
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
        let drawn = if lanes.is_empty() {
            self.finishing_lanes()
        } else {
            lanes
        };
        self.arrangement.audio_takes = self.audio_take_looks(&drawn);
        let head = self.transport.state().ticks;
        let bar = self.document.project().meter_map().meter_at(head).bar();
        self.arrangement.recording = self
            .record
            .session
            .as_ref()
            .map(|session| session.look(head, bar));
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

    pub(crate) fn take_so_far(&self) -> Option<(Ticks, Ticks, usize)> {
        let session = self.record.session.as_ref()?;
        self.arrangement.preview.as_ref()?;
        let head = self.transport.state().ticks;
        let takes = session.takes_so_far(head);
        let notes = takes.first().map_or(0, |(_, t)| t.notes.len());
        Some((session.from(), session.end(head), notes))
    }

    fn step_recording(&mut self) {
        if self.record.queue.is_none() {
            self.record.queue = self.transport.audio.midi.take_captured();
        }
        let snap = self.transport.state();
        self.transport
            .gate_metronome(self.settings.record.only_while_recording && !snap.recording);
        self.tidy_played();
        let Some(mut session) = self.record.session.take() else {
            self.keep_played_only();
            return;
        };
        if session.document != self.document.session() {
            self.abandon(session, true);
            return;
        }
        self.keep_captured(&mut session, snap.sample_rate);
        session.seen |= snap.recording;
        let head = snap.ticks;
        if snap.playing && snap.recording {
            if head < session.last && session.looped.is_some() {
                session.wrapped = true;
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

    fn tidy_played(&mut self) {
        let document = self.document.session();
        let record = &mut self.record;
        if record.played_for != document || !self.settings.record.keep_played {
            record.played.clear();
            record.played_for = document;
        }
        record.played.forget(Instant::now());
    }

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

    fn keep_captured(&mut self, session: &mut Session, rate: u32) {
        let keep = self.settings.record.keep_played;
        let Some(queue) = self.record.queue.as_mut() else {
            return;
        };
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
                if keep {
                    self.record.played.push(&captured, now);
                }
            } else if rate > 0 {
                session.capture.push(&captured, latency, &ticks);
            }
        }
    }

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

    fn finish(&mut self, mut session: Session, head: Ticks) {
        self.unlatch(&session.latched);
        let over = session.recorded_over(head);
        self.end_audio_takes(std::mem::take(&mut session.audio), false, over);
        let end = session.end(head);
        session.capture.close_all(head);
        if !self.commit(&mut session, end) {
            self.file_notice = Some(
                "A locked clip is in the way of the take, so the end of it was not kept.".into(),
            );
        }
        self.restore_autosave();
    }

    fn finish_blocked(&mut self, session: Session) {
        self.unlatch(&session.latched);
        self.file_notice = Some(
            "Recording stopped: a locked clip is in the way. Unlock it, or record elsewhere."
                .into(),
        );
        self.transport.end_record();
        self.abandon(session, false);
    }

    fn abandon(&mut self, mut session: Session, discard: bool) {
        self.unlatch(&session.latched);
        let over = if discard {
            Vec::new()
        } else {
            session.recorded_over(self.transport.state().ticks)
        };
        self.end_audio_takes(std::mem::take(&mut session.audio), discard, over);
        drop(session);
        self.transport.end_record();
        self.restore_autosave();
    }

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

    fn unlatch(&mut self, latched: &[Id]) {
        for &id in latched {
            if self.arrangement.auto.mode_of(id) == AutoMode::Latch {
                self.arrangement.auto.set_mode(id, AutoMode::Read);
            }
        }
    }

    fn restore_autosave(&mut self) {
        if let Some(autosave) = self.autosave.as_mut() {
            autosave.unhurry();
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
