//! The transport (spec §5): what drives `lumen-engine` (play, stop, locate,
//! scrub, Follow, the timeline it plays) here; the bar's controls (go to
//! start, stop, play, record, loop, metronome, tempo, time signature, the
//! position field) in `bar.rs`; the audio status, its panel and the
//! settings window in `status.rs`. Keys are commands (`crate::commands`).

mod bar;
pub(crate) mod budget;
mod budget_view;
mod compensation;
mod dropouts;
mod fit;
pub mod hold;
mod midi_chip;
pub(crate) mod record;
mod status;
#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use lumen_core::time::{TempoMap, Ticks};
use lumen_engine::{Command, MetronomeSettings, Region, Timeline, TransportSnapshot};
use lumen_project::model::Project;

use crate::audio::{Audio, Choice};

pub use bar::punch_menu;
pub use fit::divider;
pub use midi_chip::{Lights as MidiLights, View as MidiView};

/// What the disk thread reports, for the audio panel and the chip.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DiskReading {
    /// The share of the last second spent reading (0.0 to 1.0).
    pub load: f32,
    /// Underruns since the pool was made.
    pub underruns: u64,
    /// Pages that could not be read (a file gone or unreadable).
    pub failed: u32,
}

pub struct Transport {
    pub audio: Audio,
    /// The UI's copy of what the engine plays against.
    pub timeline: Timeline,
    /// A new timeline is waiting to be sent (the queue was full).
    timeline_pending: bool,
    pub loop_region: Region,
    /// Text while the position field is being edited.
    editing: Option<String>,
    error: Option<String>,
    /// Keep drawing every frame until then, so the result of a command shows
    /// at once rather than at the next idle repaint.
    settle_until: Option<Instant>,
    /// Bytes of clip audio in memory, shown in the audio panel.
    pub clip_memory: usize,
    pub metronome: MetronomeSettings,
    /// The metronome is held silent although it is on, because it sounds
    /// only while recording (M5 slice 6): the engine is sent `enabled` off.
    metronome_gated: bool,
    /// How the arrangement follows the playhead (saved in settings), and
    /// whether that is paused by scrolling by hand (until Play).
    pub follow: crate::settings::Follow,
    pub follow_paused: bool,
    /// The Follow mode to turn back on after Off.
    follow_last: crate::settings::Follow,
    /// How the time ruler and the position's time read (saved in settings).
    pub time_format: crate::timefmt::TimeFormat,
    test_tone: bool,
    /// What the MIDI chip's popover asked for, taken by the app.
    midi_asked: Vec<crate::midi::Action>,
    /// A Preferences page the audio panel or the MIDI chip asked to open,
    /// taken by the app (M9 R7).
    pub open_preferences: Option<crate::preferences::Page>,
    /// The disk thread's reading while audio plays from disk, set by the app
    /// each frame (M9 slice R6, the owner's D A).
    pub disk: Option<DiskReading>,
    /// What a start waits for while audio plays from disk, set by the app,
    /// and a start held for it (the owner's L A).
    pub gate: Option<hold::Gate>,
    held: Option<hold::Held>,
    /// What the chip's lights showed when last drawn.
    pub midi_lights: MidiLights,
    /// CPU load shown: the peak over the last second, refreshed once a second.
    cpu_peak: f32,
    cpu_taken: Option<Instant>,
    /// Where each block's time goes, for the audio panel (ADR-0075); the
    /// app feeds it the hosts' costs once a second.
    pub(crate) budget: budget::Budget,
    /// Dropouts told as episodes in the bar and counts in the panel.
    dropouts: dropouts::Dropouts,
    /// Underruns (pages the disk read too late) told the same way, counted
    /// from `disk_base`, the pool's count at the last Reset.
    late: dropouts::Dropouts,
    disk_base: u64,
    /// The playhead as drawn while playing: smooth, where the engine's
    /// position moves once per audio block (`playhead.rs`).
    clock: crate::playhead::Clock,
    head: Option<Ticks>,
    /// A locate sent, and where the engine's playhead was then: until the
    /// engine reports a move, [`Self::position`] is the locate's target.
    located: Option<(Ticks, Ticks)>,
    /// Playback started from stop since the app last looked: integrated
    /// loudness starts again (the owner's B5; Continue keeps it).
    pub played_from_stop: bool,
    /// Panic was pressed: the app also silences MIDI outputs directly (the
    /// engine may not be running to do it).
    pub panic_midi: bool,
    /// What the info line should say about the bar this frame, for the
    /// app to take (the delay compensation readout).
    pub(crate) info: Option<crate::app::info::Info>,
    /// The computer keyboard's readout while its mode is on, set by the app
    /// each frame; `None` when off (M5 slice 2).
    pub(crate) keyboard_readout: Option<String>,
    /// Its button was clicked, for the app to take.
    pub(crate) keyboard_clicked: bool,
    /// The readout fitted in the bar this frame; when it did not (the
    /// smallest window) the info line carries it.
    pub(crate) keyboard_readout_shown: bool,
    /// What fits in the bar (`fit.rs`).
    fit: fit::Fit,
    /// Commands the bar's controls asked for this frame (Record, Overdub,
    /// the count-in, record-only metronome), for the app to run: the bar has
    /// no access to the app's settings or its command dispatch.
    pub(crate) requests: Vec<crate::commands::Command>,
}

fn timeline_of(project: &Project) -> Timeline {
    let tempo = TempoMap::new(project.tempo.clone())
        .unwrap_or_else(|_| TempoMap::constant(120.0).expect("120 BPM is valid"));
    Timeline::new(tempo, project.meter_map())
}

impl Transport {
    pub fn new(project: &Project, choice: &Choice) -> Self {
        let timeline = timeline_of(project);
        // Until the ruler can set it (M3), the loop covers bars 1–4.
        let loop_region = Region::new(Ticks::ZERO, timeline.meter.bar_start(4), false);
        Self {
            audio: Audio::start(choice, timeline.clone()),
            timeline,
            timeline_pending: false,
            loop_region,
            editing: None,
            error: None,
            settle_until: None,
            metronome: MetronomeSettings::default(),
            metronome_gated: false,
            follow: crate::settings::Follow::Page,
            follow_paused: false,
            follow_last: crate::settings::Follow::Page,
            time_format: crate::timefmt::TimeFormat::MinSec,
            test_tone: false,
            midi_asked: Vec::new(),
            open_preferences: None,
            disk: None,
            gate: None,
            held: None,
            late: Default::default(),
            disk_base: 0,
            midi_lights: Default::default(),
            cpu_peak: 0.0,
            cpu_taken: None,
            budget: Default::default(),
            dropouts: Default::default(),
            clip_memory: 0,
            clock: Default::default(),
            head: None,
            located: None,
            played_from_stop: false,
            panic_midi: false,
            info: None,
            keyboard_readout: None,
            keyboard_clicked: false,
            keyboard_readout_shown: false,
            fit: Default::default(),
            requests: Vec::new(),
        }
    }

    pub fn state(&self) -> TransportSnapshot {
        self.audio.engine.transport()
    }

    /// Moves the drawn playhead on to `now` (seconds on the frame clock).
    /// Once per frame, before anything reads [`Self::playhead`].
    pub fn advance_playhead(&mut self, now: f64) {
        let state = self.state();
        let rate = state.sample_rate;
        let region = self.loop_region;
        let looped = (region.enabled && region.end > region.start).then(|| {
            (
                self.timeline.sample_at(region.start, rate),
                self.timeline.sample_at(region.end, rate),
            )
        });
        let sample = self.clock.update(
            now,
            crate::playhead::Reading {
                playing: state.playing,
                sample: state.sample,
                sample_rate: rate,
                looped,
            },
        );
        self.head = (state.playing && rate > 0).then(|| {
            self.timeline
                .tempo
                .ticks_at_seconds(sample / f64::from(rate))
        });
    }

    /// Where the playhead is drawn and followed: smooth while playing,
    /// exactly the engine's position otherwise.
    pub fn playhead(&self) -> Ticks {
        self.head.unwrap_or_else(|| self.state().ticks)
    }

    /// Whether the UI should redraw every frame: while playing, and briefly
    /// after a command until the engine has applied it.
    pub fn animating(&self) -> bool {
        let state = self.state();
        state.playing
            || state.recording
            || self.held.is_some()
            || self.settle_until.is_some_and(|t| Instant::now() < t)
    }

    fn settle(&mut self) {
        // The engine applies commands at its next block (a few ms); the
        // margin covers large buffers.
        self.settle_until = Some(Instant::now() + Duration::from_millis(300));
    }

    fn send(&mut self, mut command: Command) {
        if let Command::SetMetronome(settings) = &mut command
            && self.metronome_gated
        {
            settings.enabled = false;
        }
        crate::trace::log(|| format!("send {command:?}"));
        self.settle();
        // A full queue means the engine is not running; the UI shows why.
        let _ = self.audio.engine.send(command);
    }

    /// Sends `command`, which starts playback at `at`: while audio plays
    /// from disk, once what plays there is in memory, for at most
    /// [`hold::LONGEST`] (the owner's L A).
    fn start(&mut self, command: Command, at: Ticks) {
        // A new start replaces one still waiting, which never fires.
        self.drop_held();
        let Some(gate) = &self.gate else {
            self.send(command);
            return;
        };
        let sample = gate.sample_at(at);
        // The disk reads there first until the playhead has arrived, also
        // when the start goes at once: what follows the ready part is read
        // next.
        gate.cuer.cue(Some(sample));
        if gate.ready(sample) {
            self.send(command);
            return;
        }
        crate::trace::log(|| format!("hold {command:?} for the disk"));
        self.held = Some(hold::Held {
            command,
            sample,
            since: Instant::now(),
        });
        self.settle();
    }

    /// Forgets a start held for the disk, and the disk's cue for it.
    /// Whether one was held.
    fn drop_held(&mut self) -> bool {
        let dropped = self.held.take().is_some();
        if dropped && let Some(gate) = &self.gate {
            gate.cuer.cue(None);
        }
        dropped
    }

    /// Sends a held start once it is due.
    fn release(&mut self) {
        if let Some(held) = &self.held
            && held.due(self.gate.as_ref(), Instant::now())
        {
            // The disk keeps its cue until the playhead arrives there.
            let held = self.held.take().expect("checked above");
            self.send(held.command);
        }
    }

    /// Stops and silences everything, MIDI notes included.
    pub fn panic(&mut self) {
        self.drop_held();
        self.send(Command::Panic);
        self.panic_midi = true;
    }

    pub fn play_stop(&mut self) {
        // Pressed again while Play waits for the disk: it never starts. A
        // locate held while playing is dropped, and playback stops.
        if self.drop_held() && !self.state().playing {
            return;
        }
        if self.state().playing {
            self.send(Command::Stop);
        } else {
            self.played_from_stop = true;
            let marker = self.state().marker;
            self.start(Command::Play, marker);
        }
    }

    /// Applies saved metronome settings.
    pub fn restore_metronome(&mut self, settings: MetronomeSettings) {
        self.metronome = settings;
        self.send(Command::SetMetronome(settings));
    }

    /// Starts recording from the play marker, after `count_in` beats of
    /// clicks when stopped (M5 slice 6).
    pub fn record(&mut self, count_in: u32) {
        let state = self.state();
        if state.playing {
            self.send(Command::Record { count_in });
        } else {
            self.played_from_stop = true;
            self.start(Command::Record { count_in }, state.marker);
        }
    }

    /// Stops (a count-in too), without the Stop-twice rule of [`Self::play_stop`].
    pub fn stop(&mut self) {
        self.drop_held();
        self.send(Command::Stop);
    }

    /// Ends recording; playback carries on.
    pub fn end_record(&mut self) {
        self.send(Command::EndRecord);
    }

    /// Holds the metronome silent (or lets it sound) while it is on but only
    /// meant for recording. Resends it when this changes.
    pub fn gate_metronome(&mut self, gated: bool) {
        if gated != self.metronome_gated {
            self.metronome_gated = gated;
            self.send(Command::SetMetronome(self.metronome));
        }
    }

    /// Shift+Space: stop where playback is, or continue from the playhead.
    pub fn pause_continue(&mut self) {
        // As Play: a held start is dropped; a held locate is dropped and
        // playback pauses.
        if self.drop_held() && !self.state().playing {
            return;
        }
        let state = self.state();
        if state.playing {
            self.send(Command::Pause);
        } else {
            self.start(Command::Continue, state.ticks);
        }
    }

    pub fn locate(&mut self, at: Ticks) {
        let state = self.state();
        self.located = Some((at, state.ticks));
        match &self.gate {
            // While playing, the playhead holds at the old place until the
            // new one is ready (L A); stopped, the disk reads there first.
            Some(_) if state.playing => self.start(Command::Locate(at), at),
            Some(gate) => {
                let held = self.held.take();
                gate.cuer.cue(Some(gate.sample_at(at)));
                self.send(Command::Locate(at));
                // A start waiting for the disk waits for the new place.
                if let Some(held) = held {
                    self.start(held.command, at);
                }
            }
            None => self.send(Command::Locate(at)),
        }
    }

    /// Where the playhead is, counting a locate the audio thread has not
    /// played a block since: what edits at the playhead use (a paste right
    /// after a click on the lanes landed at the old place, owner after M4
    /// slice 7).
    pub fn position(&self) -> Ticks {
        let ticks = self.state().ticks;
        match self.located {
            Some((at, before)) if ticks == before => at,
            _ => ticks,
        }
    }

    /// Scrubs at `at` while stopped (slice 5e); the engine ignores it while
    /// playing.
    pub fn scrub(&mut self, at: Ticks, style: lumen_engine::ScrubStyle) {
        self.send(Command::Scrub { at, style });
    }

    pub fn end_scrub(&mut self) {
        self.send(Command::EndScrub);
    }

    /// Plays the project's loop (its brace and switch are document edits,
    /// slice 5c): sends it to the engine when it differs from what plays.
    pub fn follow_loop(&mut self, state: &lumen_project::model::TransportState) {
        let region = Region::new(state.loop_start, state.loop_end, state.loop_enabled);
        if region != self.loop_region {
            self.loop_region = region;
            self.send(Command::SetLoop(region));
        }
    }

    /// The metronome on or off (the button and `transport.metronome`).
    pub fn toggle_metronome(&mut self) {
        self.metronome.enabled = !self.metronome.enabled;
        self.send(Command::SetMetronome(self.metronome));
    }

    /// Follow on, off, or back on after a pause (the button and Ctrl+Shift+F).
    pub fn toggle_follow(&mut self) {
        use crate::settings::Follow;
        if self.follow == Follow::Off {
            self.follow = self.follow_last;
            self.follow_paused = false;
        } else if self.follow_paused {
            self.follow_paused = false;
        } else {
            self.follow_last = self.follow;
            self.follow = Follow::Off;
        }
    }

    /// Applies the saved Follow mode.
    pub fn restore_follow(&mut self, follow: crate::settings::Follow) {
        self.follow = follow;
        if follow != crate::settings::Follow::Off {
            self.follow_last = follow;
        }
    }

    /// Rebuilds the timeline from the project and sends it to the engine
    /// (after an edit, undo or redo that may have changed tempo or meter).
    /// Most edits (arming, volume, pan, renaming) leave tempo and meter as
    /// they were; those send nothing.
    pub fn timeline_changed(&mut self, project: &Project) {
        let timeline = timeline_of(project);
        if timeline == self.timeline {
            return;
        }
        self.timeline = timeline;
        self.timeline_pending = true;
        self.settle();
    }

    /// Per-frame work: device events and pending timeline swaps. Returns
    /// true if something visible changed.
    pub fn update(&mut self) -> bool {
        if self
            .located
            .is_some_and(|(_, before)| self.state().ticks != before)
        {
            self.located = None;
        }
        self.release();
        if self.timeline_pending {
            self.timeline_pending = self
                .audio
                .engine
                .set_timeline(self.timeline.clone())
                .is_err();
        }
        self.audio.poll()
    }
}
