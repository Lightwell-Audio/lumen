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
                let shown: Vec<_> = take
                    .notes
                    .iter()
                    .filter(|n| n.start + n.length > from && n.start < to.max(now))
                    .collect();
                let base = self.rec.base_track(track);
                let gaps = match base {
                    Some(base) if overdub && to > from => {
                        lumen_project::edit::record::gaps(base, (from, to))
                    }
                    _ => Vec::new(),
                };
                let clips = match base {
                    Some(base) if overdub => base.clips.as_slice(),
                    _ => &[],
                };
                // The gaps as far as the notes reach (a held note's start
                // can be ahead of `to` by a frame).
                let reach = match base {
                    Some(base) if overdub && to.max(now) > from => {
                        lumen_project::edit::record::gaps(base, (from, to.max(now)))
                    }
                    _ => Vec::new(),
                };
                let starts: Vec<(Ticks, u8)> = shown.iter().map(|n| (n.start, n.pitch)).collect();
                let ranges = live_ranges(&starts, overdub, clips, &reach);
                let notes = shown
                    .iter()
                    .zip(ranges)
                    .map(|(n, range)| LiveNote {
                        start: n.start,
                        end: n.start + n.length,
                        pitch: n.pitch,
                        velocity: n.velocity,
                        range,
                    })
                    .collect();
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

/// The pitch range each of a take's live notes is drawn to (`notes`: where
/// each starts and its pitch), so it sits where it will once committed
/// (`piano_roll::mini::Fit`). A Replace take is one clip, so one range for
/// all. An Overdub merges into the MIDI clips under it, each keeping a range
/// of its own notes and the new ones, and each gap between them (`gaps`)
/// becomes a clip of its own.
fn live_ranges(
    notes: &[(Ticks, u8)],
    overdub: bool,
    clips: &[std::sync::Arc<lumen_project::model::Clip>],
    gaps: &[(Ticks, Ticks)],
) -> Vec<(u8, u8)> {
    use crate::piano_roll::mini::{pitch_range, union};
    use crate::piano_roll::notes::ClipNotes;
    use lumen_project::model::ClipContent;
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Region {
        Clip(usize),
        Gap(usize),
        Alone,
    }
    let region = |at: Ticks| {
        if !overdub {
            return Region::Gap(0);
        }
        if let Some(i) = clips.iter().position(|c| {
            matches!(c.content, ClipContent::Midi { .. })
                && c.start <= at
                && at < c.start + c.length
        }) {
            return Region::Clip(i);
        }
        gaps.iter()
            .position(|&(a, b)| a <= at && at < b)
            .map_or(Region::Alone, Region::Gap)
    };
    let regions: Vec<Region> = notes.iter().map(|&(at, _)| region(at)).collect();
    let mut known: Vec<(Region, Option<(u8, u8)>)> = Vec::new();
    let mut out = Vec::with_capacity(notes.len());
    for (&(_, pitch), &r) in notes.iter().zip(&regions) {
        if r == Region::Alone {
            out.push((pitch, pitch));
            continue;
        }
        let range = match known.iter().find(|(k, _)| *k == r) {
            Some(&(_, range)) => range,
            None => {
                let own = match r {
                    Region::Clip(i) => match &clips[i].content {
                        ClipContent::Midi { notes, .. } => pitch_range(&ClipNotes {
                            start: clips[i].start,
                            length: clips[i].length,
                            loop_length: clips[i].loop_length,
                            notes,
                        }),
                        ClipContent::Audio { .. } => None,
                    },
                    _ => None,
                };
                let played = notes
                    .iter()
                    .zip(&regions)
                    .filter(|&(_, &k)| k == r)
                    .fold(None, |range, (&(_, p), _)| union(range, Some((p, p))));
                let range = union(own, played);
                known.push((r, range));
                range
            }
        };
        out.push(range.unwrap_or((pitch, pitch)));
    }
    out
}

impl App {
    /// The tracks a Replace take is recording over: what they held from the
    /// take's start on is not played, so the old notes do not sound under
    /// the new ones.
    pub(crate) fn replacing(&self) -> Vec<crate::midi_play::Replacing> {
        let Some(session) = self
            .record
            .session
            .as_ref()
            .filter(|s| s.mode == Mode::Replace)
        else {
            return Vec::new();
        };
        session
            .tracks
            .iter()
            .map(|&track| crate::midi_play::Replacing {
                track,
                from: session
                    .punch
                    .map_or(session.start, |(a, _)| session.start.max(a)),
                take: session.rec.take_clip(track),
            })
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::model::{Clip, ClipContent, Note};
    use std::sync::Arc;

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn clip(start: i64, length: i64, pitches: &[u8]) -> Arc<Clip> {
        Arc::new(Clip {
            id: lumen_project::model::Id(1),
            name: "c".into(),
            start: q(start),
            length: q(length),
            muted: false,
            color: None,
            locked: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            content: ClipContent::Midi {
                notes: pitches
                    .iter()
                    .map(|&p| Note::new(q(0), q(1), p, 100))
                    .collect(),
                events: Vec::new(),
            },
        })
    }

    #[test]
    fn a_replace_take_is_one_range() {
        let notes = [(q(0), 60), (q(8), 72), (q(9), 64)];
        let clips = [clip(0, 4, &[30])];
        assert_eq!(
            live_ranges(&notes, false, &clips, &[]),
            vec![(60, 72); 3],
            "Replace ignores the clips it cuts away"
        );
    }

    #[test]
    fn an_overdub_fits_each_clip_with_its_own_notes_and_each_gap_alone() {
        // A clip from 0 to 4 holding 40 and 50; a gap from 4 to 8.
        let clips = [clip(0, 4, &[40, 50])];
        let gaps = [(q(4), q(8))];
        let notes = [(q(1), 60), (q(5), 70), (q(6), 72), (q(9), 80)];
        assert_eq!(
            live_ranges(&notes, true, &clips, &gaps),
            vec![(40, 60), (70, 72), (70, 72), (80, 80)]
        );
    }
}
