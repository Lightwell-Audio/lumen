//! Auditioning notes while editing (M5 slice 3d, the owner's Live A): the
//! piano roll plays a short preview of a note when it is drawn or its pitch
//! changes, on the edited clip's track whether or not that track listens.
//! The playing clip changes only when the gesture ends (one document edit).
//!
//! [`Audition`] is the service: one previewed note at a time (a new one
//! lets go of the last), let go on its own after [`LENGTH`], or by
//! [`Audition::release`]. It sends through the live feed as its own source
//! (`lumen_engine::AUDITION`), so a preview on a pitch that a held key or a
//! clip is playing never ends their note, and theirs never ends it.
//! Everything that should silence a preview calls [`Audition::release`]:
//! the end or cancel of a gesture, the editor or clip changing, losing the
//! window's focus, the transport stopping and Panic ([`Audition::forget`]).
//!
//! The app owns one (`App::audition`) and exposes it as
//! `App::audition_play(track, pitch, velocity)` and
//! `App::audition_release()`; the frame calls `App::update_audition`.

use std::time::{Duration, Instant};

use lumen_core::midi::Msg;
use lumen_project::model::Id;

/// How long a previewed note sounds if nothing lets it go sooner.
pub(crate) const LENGTH: Duration = Duration::from_millis(250);

/// Where an audition's messages go: the live feed's keys, or a test.
pub(crate) trait Sink {
    /// Plays `msg` as an audition on `track`. False if it could not go (the
    /// queue is full).
    fn send_to(&mut self, track: u64, msg: Msg) -> bool;
}

impl Sink for crate::live::LiveKeys {
    fn send_to(&mut self, track: u64, msg: Msg) -> bool {
        crate::live::LiveKeys::send_to(self, track, msg)
    }
}

/// The note being previewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sounding {
    track: u64,
    pitch: u8,
    /// When it lets go by itself.
    until: Instant,
}

/// The preview service.
#[derive(Debug, Clone)]
pub(crate) struct Audition {
    /// The setting (`settings.midi.audition`): off plays nothing.
    enabled: bool,
    sounding: Option<Sounding>,
    /// A note-off the queue could not take yet; tried again every frame.
    owed: Option<(u64, Msg)>,
    playing: bool,
    focused: bool,
}

impl Default for Audition {
    fn default() -> Self {
        Self {
            enabled: true,
            sounding: None,
            owed: None,
            playing: false,
            focused: true,
        }
    }
}

impl Audition {
    /// Turns auditioning on or off; a note sounding when it goes off is
    /// let go by the next [`Audition::update`].
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Previews `pitch` at `velocity` on `track` for [`LENGTH`], letting go
    /// of the one before. Nothing when auditioning is off. True if a note
    /// started.
    pub fn play(
        &mut self,
        sink: &mut dyn Sink,
        track: Id,
        pitch: u8,
        velocity: u8,
        now: Instant,
    ) -> bool {
        if !self.enabled {
            return false;
        }
        self.release(sink);
        let pitch = pitch.min(127);
        let started = sink.send_to(track.0, Msg::note_on(0, pitch, velocity.clamp(1, 127)));
        if started {
            self.sounding = Some(Sounding {
                track: track.0,
                pitch,
                until: now + LENGTH,
            });
        }
        started
    }

    /// Lets go of the previewed note, if one sounds.
    pub fn release(&mut self, sink: &mut dyn Sink) {
        self.retry(sink);
        if let Some(note) = self.sounding.take() {
            let off = Msg::note_off(0, note.pitch);
            if !sink.send_to(note.track, off) {
                self.owed = Some((note.track, off));
            }
        }
    }

    /// Forgets the preview without sending (Panic silences the engine).
    pub fn forget(&mut self) {
        self.sounding = None;
        self.owed = None;
    }

    /// Whether a note sounds now (tests).
    pub fn is_sounding(&self) -> bool {
        self.sounding.is_some()
    }

    fn retry(&mut self, sink: &mut dyn Sink) {
        if let Some((track, off)) = self.owed.take()
            && !sink.send_to(track, off)
        {
            self.owed = Some((track, off));
        }
    }

    /// Called every frame: lets go when the note's time is up, the window
    /// lost focus or the transport stopped. Returns when to be called
    /// again if a note still sounds.
    pub fn update(
        &mut self,
        sink: &mut dyn Sink,
        now: Instant,
        playing: bool,
        focused: bool,
    ) -> Option<Duration> {
        let stopped = self.playing && !playing;
        let blurred = self.focused && !focused;
        let off = !self.enabled;
        self.playing = playing;
        self.focused = focused;
        self.retry(sink);
        if stopped || blurred || off {
            self.release(sink);
        }
        let note = self.sounding?;
        if now >= note.until {
            self.release(sink);
            return self.owed.map(|_| Duration::from_millis(16));
        }
        Some(note.until - now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Log {
        sent: Vec<(u64, Msg)>,
        full: bool,
    }

    impl Sink for Log {
        fn send_to(&mut self, track: u64, msg: Msg) -> bool {
            if self.full {
                return false;
            }
            self.sent.push((track, msg));
            true
        }
    }

    fn on(pitch: u8) -> Msg {
        Msg::note_on(0, pitch, 90)
    }

    #[test]
    fn a_new_note_lets_go_of_the_last_and_one_sounds_at_a_time() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play(&mut log, Id(4), 60, 90, now);
        audition.play(&mut log, Id(4), 62, 90, now);
        assert_eq!(
            log.sent,
            vec![(4, on(60)), (4, Msg::note_off(0, 60)), (4, on(62)),]
        );
        audition.release(&mut log);
        audition.release(&mut log);
        assert_eq!(log.sent.len(), 4, "a second release sends nothing");
        assert!(!audition.is_sounding());
    }

    #[test]
    fn it_lets_go_by_itself_after_its_length() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play(&mut log, Id(1), 60, 90, now);
        let wait = audition.update(&mut log, now + LENGTH / 2, false, true);
        assert_eq!(wait, Some(LENGTH / 2));
        assert_eq!(log.sent.len(), 1);
        assert_eq!(audition.update(&mut log, now + LENGTH, false, true), None);
        assert_eq!(log.sent.len(), 2);
    }

    #[test]
    fn stop_and_losing_focus_let_go_and_off_plays_nothing() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.update(&mut log, now, true, true);
        audition.play(&mut log, Id(1), 60, 90, now);
        audition.update(&mut log, now, false, true);
        assert!(!audition.is_sounding(), "the transport stopped");
        audition.play(&mut log, Id(1), 61, 90, now);
        audition.update(&mut log, now, false, false);
        assert!(!audition.is_sounding(), "focus lost");
        let before = log.sent.len();
        audition.set_enabled(false);
        assert!(!audition.play(&mut log, Id(1), 62, 90, now));
        assert_eq!(log.sent.len(), before);
    }

    #[test]
    fn a_full_queue_owes_the_note_off_until_it_can_go() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play(&mut log, Id(1), 60, 90, now);
        log.full = true;
        audition.release(&mut log);
        assert_eq!(log.sent.len(), 1);
        log.full = false;
        audition.update(&mut log, now, false, true);
        assert_eq!(log.sent.last(), Some(&(1, Msg::note_off(0, 60))));
    }
}
