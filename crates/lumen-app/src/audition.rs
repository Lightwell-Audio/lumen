//! Auditioning notes while editing (M5 slice 3d, the owner's Live A): the
//! piano roll plays a short preview of a note when it is drawn or its pitch
//! changes, on the edited clip's track whether or not that track listens.
//! The playing clip changes only when the gesture ends (one document edit).
//!
//! [`Audition`] is the service: one preview at a time, of one note or a
//! chord of moved notes (a new one lets go of the last; a moving chord
//! strikes only the notes whose pitch changed), let go on its own after
//! [`LENGTH`], or by [`Audition::release`]. It sends through the live feed as its own source
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

/// The MIDI channel previews play on. Reserved: a clip's notes on an
/// instrument track are on channel 0, so a preview of the same key never
/// shares a voice with them. A hardware track's messages are sent out on
/// its own channel by the engine, so this does not reach the hardware.
const CHANNEL: u8 = 15;

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

/// A note a preview plays: its pitch and velocity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tone {
    pub pitch: u8,
    pub velocity: u8,
}

/// The notes of one gesture's preview, on one track.
#[derive(Debug, Clone, Default)]
struct Chord {
    track: u64,
    /// The tones the last call was given, by position. A tone whose pitch
    /// is the same as its position's last time is not struck again.
    asked: Vec<Tone>,
    /// The pitches sounding now, each once (two tones on one pitch make one
    /// note, so its off is sent once).
    sounding: Vec<u8>,
    /// When the sounding notes let go by themselves.
    until: Option<Instant>,
}

/// The preview service.
#[derive(Debug, Clone)]
pub(crate) struct Audition {
    /// The setting (`settings.midi.audition`): off plays nothing.
    enabled: bool,
    chord: Option<Chord>,
    /// Note-offs the queue could not take yet, in order; tried again every
    /// frame.
    owed: Vec<(u64, Msg)>,
    playing: bool,
    focused: bool,
}

impl Default for Audition {
    fn default() -> Self {
        Self {
            enabled: true,
            chord: None,
            owed: Vec::new(),
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

    /// Starts a fresh preview of `tones` on `track` for [`LENGTH`], letting
    /// go of the one before. Nothing when auditioning is off. True if a note
    /// started.
    pub fn play_chord(
        &mut self,
        sink: &mut dyn Sink,
        track: Id,
        tones: &[Tone],
        now: Instant,
    ) -> bool {
        if !self.enabled {
            return false;
        }
        self.release(sink);
        self.change(sink, track, tones, now)
    }

    /// The gesture's notes now sound at `tones`, given in the same order as
    /// the last call. A tone whose pitch did not change keeps its note; one
    /// whose pitch changed is struck at its new pitch, and the notes that
    /// are no longer wanted let go first. A tone that is unchanged but has
    /// already let go by itself (its time ran out) is not struck again.
    /// Nothing when auditioning is off. True if a note started.
    pub fn change(&mut self, sink: &mut dyn Sink, track: Id, tones: &[Tone], now: Instant) -> bool {
        if !self.enabled {
            return false;
        }
        if self.chord.as_ref().is_some_and(|c| c.track != track.0) {
            self.release(sink);
        }
        let mut chord = self.chord.take().unwrap_or_else(|| Chord {
            track: track.0,
            ..Chord::default()
        });
        let tones: Vec<Tone> = tones
            .iter()
            .map(|t| Tone {
                pitch: t.pitch.min(127),
                velocity: t.velocity.clamp(1, 127),
            })
            .collect();
        // A pitch an unchanged tone holds keeps sounding; every other note
        // lets go before anything is struck.
        let held: Vec<u8> = tones
            .iter()
            .enumerate()
            .filter(|(i, t)| chord.asked.get(*i).is_some_and(|a| a.pitch == t.pitch))
            .map(|(_, t)| t.pitch)
            .filter(|pitch| chord.sounding.contains(pitch))
            .collect();
        for pitch in std::mem::take(&mut chord.sounding) {
            if held.contains(&pitch) {
                chord.sounding.push(pitch);
            } else {
                self.off(sink, chord.track, pitch);
            }
        }
        let mut started = false;
        for (i, tone) in tones.iter().enumerate() {
            let unchanged = chord.asked.get(i).is_some_and(|a| a.pitch == tone.pitch);
            if unchanged || chord.sounding.contains(&tone.pitch) {
                continue;
            }
            let on = Msg::note_on(CHANNEL, tone.pitch, tone.velocity);
            if sink.send_to(chord.track, on) {
                chord.sounding.push(tone.pitch);
                started = true;
            }
        }
        if started {
            chord.until = Some(now + LENGTH);
        }
        if chord.sounding.is_empty() {
            chord.until = None;
        }
        chord.asked = tones;
        self.chord = Some(chord);
        started
    }

    /// Lets go of every note the preview holds, and forgets the tones the
    /// gesture gave.
    pub fn release(&mut self, sink: &mut dyn Sink) {
        self.retry(sink);
        self.silence(sink);
        self.chord = None;
    }

    /// Sends the note-offs of what sounds. The chord keeps the tones it was
    /// asked for, so a timer letting go does not strike them again.
    fn silence(&mut self, sink: &mut dyn Sink) {
        let Some(mut chord) = self.chord.take() else {
            return;
        };
        for pitch in std::mem::take(&mut chord.sounding) {
            self.off(sink, chord.track, pitch);
        }
        chord.until = None;
        self.chord = Some(chord);
    }

    /// Sends a note-off, or owes it when the queue is full.
    fn off(&mut self, sink: &mut dyn Sink, track: u64, pitch: u8) {
        let msg = Msg::note_off(CHANNEL, pitch);
        if !sink.send_to(track, msg) {
            self.owed.push((track, msg));
        }
    }

    /// Forgets the preview without sending (Panic silences the engine).
    pub fn forget(&mut self) {
        self.chord = None;
        self.owed.clear();
    }

    /// Whether a note sounds now (tests).
    pub fn is_sounding(&self) -> bool {
        self.chord.as_ref().is_some_and(|c| !c.sounding.is_empty())
    }

    fn retry(&mut self, sink: &mut dyn Sink) {
        for (track, msg) in std::mem::take(&mut self.owed) {
            if !sink.send_to(track, msg) {
                self.owed.push((track, msg));
            }
        }
    }

    /// Called every frame: lets go when the notes' time is up, the window
    /// lost focus or the transport stopped. Returns when to be called again
    /// if a note still sounds or an off is still owed.
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
        if let Some(until) = self.chord.as_ref().and_then(|c| c.until)
            && now >= until
        {
            self.silence(sink);
        }
        let sounding = self
            .chord
            .as_ref()
            .and_then(|c| c.until)
            .map(|until| until.saturating_duration_since(now));
        let retry = (!self.owed.is_empty()).then_some(Duration::from_millis(16));
        match (sounding, retry) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
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
        Msg::note_on(CHANNEL, pitch, 90)
    }

    fn off(pitch: u8) -> Msg {
        Msg::note_off(CHANNEL, pitch)
    }

    fn tone(pitch: u8) -> Tone {
        Tone {
            pitch,
            velocity: 90,
        }
    }

    fn tones(pitches: &[u8]) -> Vec<Tone> {
        pitches.iter().map(|&p| tone(p)).collect()
    }

    /// The log is well formed: no key is struck while it sounds and none is
    /// let go while silent. With `quiet`, every note-on has had its off.
    fn check(log: &Log, quiet: bool) {
        let mut held = std::collections::BTreeSet::new();
        for (track, msg) in &log.sent {
            let [status, key, _] = msg.0;
            let note = (*track, key);
            match status & 0xF0 {
                0x90 => assert!(held.insert(note), "key {key} struck while it sounds"),
                0x80 => assert!(held.remove(&note), "key {key} let go while silent"),
                other => panic!("unexpected status {other:#x}"),
            }
        }
        if quiet {
            assert!(held.is_empty(), "stuck notes: {held:?}");
        }
    }

    #[test]
    fn a_new_note_lets_go_of_the_last_and_one_sounds_at_a_time() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play_chord(&mut log, Id(4), &tones(&[60]), now);
        audition.play_chord(&mut log, Id(4), &tones(&[62]), now);
        assert_eq!(log.sent, vec![(4, on(60)), (4, off(60)), (4, on(62))]);
        audition.release(&mut log);
        audition.release(&mut log);
        assert_eq!(log.sent.len(), 4, "a second release sends nothing");
        assert!(!audition.is_sounding());
        check(&log, true);
    }

    #[test]
    fn it_lets_go_by_itself_after_its_length() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play_chord(&mut log, Id(1), &tones(&[60]), now);
        let wait = audition.update(&mut log, now + LENGTH / 2, false, true);
        assert_eq!(wait, Some(LENGTH / 2));
        assert_eq!(log.sent.len(), 1);
        assert_eq!(audition.update(&mut log, now + LENGTH, false, true), None);
        assert_eq!(log.sent.len(), 2);
        check(&log, true);
    }

    #[test]
    fn stop_and_losing_focus_let_go_and_off_plays_nothing() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.update(&mut log, now, true, true);
        audition.play_chord(&mut log, Id(1), &tones(&[60]), now);
        audition.update(&mut log, now, false, true);
        assert!(!audition.is_sounding(), "the transport stopped");
        audition.play_chord(&mut log, Id(1), &tones(&[61]), now);
        audition.update(&mut log, now, false, false);
        assert!(!audition.is_sounding(), "focus lost");
        let before = log.sent.len();
        audition.set_enabled(false);
        assert!(!audition.play_chord(&mut log, Id(1), &tones(&[62]), now));
        assert!(!audition.change(&mut log, Id(1), &tones(&[63]), now));
        assert_eq!(log.sent.len(), before);
        check(&log, true);
    }

    #[test]
    fn a_full_queue_owes_the_note_off_until_it_can_go() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play_chord(&mut log, Id(1), &tones(&[60]), now);
        log.full = true;
        audition.release(&mut log);
        assert_eq!(log.sent.len(), 1);
        log.full = false;
        audition.update(&mut log, now, false, true);
        assert_eq!(log.sent.last(), Some(&(1, off(60))));
        check(&log, true);
    }

    #[test]
    fn a_chord_sounds_each_note_at_its_velocity_and_lets_go_of_each() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        let chord = [
            Tone {
                pitch: 60,
                velocity: 50,
            },
            Tone {
                pitch: 64,
                velocity: 120,
            },
        ];
        assert!(audition.play_chord(&mut log, Id(2), &chord, now));
        assert_eq!(
            log.sent,
            vec![
                (2, Msg::note_on(CHANNEL, 60, 50)),
                (2, Msg::note_on(CHANNEL, 64, 120)),
            ]
        );
        audition.release(&mut log);
        assert_eq!(&log.sent[2..], &[(2, off(60)), (2, off(64))]);
        check(&log, true);
    }

    #[test]
    fn moved_notes_are_struck_again_at_their_new_pitches_and_the_old_let_go_first() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play_chord(&mut log, Id(1), &tones(&[60, 64, 67]), now);
        let before = log.sent.len();
        assert!(audition.change(&mut log, Id(1), &tones(&[63, 67, 70]), now));
        assert_eq!(
            &log.sent[before..],
            &[
                (1, off(60)),
                (1, off(64)),
                (1, off(67)),
                (1, on(63)),
                (1, on(67)),
                (1, on(70)),
            ]
        );
        audition.release(&mut log);
        assert_eq!(
            log.sent.len(),
            before + 9,
            "three offs, three ons, three offs"
        );
        check(&log, true);
    }

    #[test]
    fn a_tone_whose_pitch_did_not_change_keeps_sounding() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play_chord(&mut log, Id(1), &tones(&[60, 64]), now);
        let before = log.sent.len();
        audition.change(&mut log, Id(1), &tones(&[60, 65]), now);
        assert_eq!(&log.sent[before..], &[(1, off(64)), (1, on(65))]);
        audition.release(&mut log);
        check(&log, true);
    }

    #[test]
    fn a_move_in_time_alone_strikes_nothing() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play_chord(&mut log, Id(1), &tones(&[60, 64, 67]), now);
        let before = log.sent.len();
        assert!(!audition.change(&mut log, Id(1), &tones(&[60, 64, 67]), now));
        assert_eq!(log.sent.len(), before);
        audition.release(&mut log);
        check(&log, true);
    }

    #[test]
    fn after_its_length_an_unmoved_tone_is_not_struck_again_but_a_moved_one_is() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play_chord(&mut log, Id(1), &tones(&[60, 64]), now);
        audition.update(&mut log, now + LENGTH, true, true);
        assert!(!audition.is_sounding(), "the time ran out");
        let before = log.sent.len();
        audition.change(&mut log, Id(1), &tones(&[60, 64]), now + LENGTH);
        assert_eq!(log.sent.len(), before, "nothing moved, nothing struck");
        audition.change(&mut log, Id(1), &tones(&[61, 64]), now + LENGTH);
        assert_eq!(&log.sent[before..], &[(1, on(61))]);
        audition.release(&mut log);
        check(&log, true);
    }

    #[test]
    fn a_change_of_track_lets_go_of_the_old_track_first() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play_chord(&mut log, Id(1), &tones(&[60]), now);
        audition.change(&mut log, Id(2), &tones(&[60]), now);
        assert_eq!(log.sent, vec![(1, on(60)), (1, off(60)), (2, on(60))]);
        audition.release(&mut log);
        check(&log, true);
    }

    #[test]
    fn two_tones_on_one_pitch_make_one_note_with_one_off() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        let twice = [
            Tone {
                pitch: 60,
                velocity: 50,
            },
            Tone {
                pitch: 60,
                velocity: 120,
            },
        ];
        audition.play_chord(&mut log, Id(1), &twice, now);
        assert_eq!(log.sent, vec![(1, Msg::note_on(CHANNEL, 60, 50))]);
        audition.release(&mut log);
        check(&log, true);
    }

    #[test]
    fn a_full_queue_strikes_nothing_and_leaves_nothing_sounding() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        log.full = true;
        assert!(!audition.play_chord(&mut log, Id(1), &tones(&[60, 64]), now));
        assert!(!audition.is_sounding());
        log.full = false;
        audition.release(&mut log);
        assert!(log.sent.is_empty());
        check(&log, true);
    }

    #[test]
    fn a_refused_off_is_sent_on_a_later_frame_and_the_rest_is_kept() {
        let (mut audition, mut log, now) = (Audition::default(), Log::default(), Instant::now());
        audition.play_chord(&mut log, Id(1), &tones(&[60, 64]), now);
        log.full = true;
        assert!(!audition.change(&mut log, Id(1), &tones(&[61, 64]), now));
        log.full = false;
        audition.update(&mut log, now, true, true);
        assert_eq!(log.sent.last(), Some(&(1, off(60))));
        audition.release(&mut log);
        check(&log, true);
    }
}
