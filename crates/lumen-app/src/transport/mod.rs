//! The transport (spec §5): what drives `lumen-engine` (play, stop, locate,
//! scrub, Follow, the timeline it plays) here; the bar's controls (go to
//! start, stop, play, record, loop, metronome, tempo, time signature, the
//! position field) in `bar.rs`; the audio status, its panel and the
//! settings window in `status.rs`. Keys are commands (`crate::commands`).

mod bar;
mod status;

use std::time::{Duration, Instant};

use lumen_core::meter::{Meter, MeterMap};
use lumen_core::time::{TempoMap, Ticks};
use lumen_engine::{Command, MetronomeSettings, Region, Timeline, TransportSnapshot};
use lumen_project::model::Project;

use crate::audio::{Audio, Choice};

pub use bar::punch_menu;

const DENOMINATORS: [u32; 7] = [1, 2, 4, 8, 16, 32, 64];

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
    /// How the arrangement follows the playhead (saved in settings), and
    /// whether that is paused by scrolling by hand (until Play).
    pub follow: crate::settings::Follow,
    pub follow_paused: bool,
    /// The Follow mode to turn back on after Off.
    follow_last: crate::settings::Follow,
    /// How the time ruler and the position's time read (saved in settings).
    pub time_format: crate::timefmt::TimeFormat,
    test_tone: bool,
    pub settings_window: crate::audio_settings::Window,
    /// CPU load shown: the peak over the last second, refreshed once a second.
    cpu_peak: f32,
    cpu_taken: Option<Instant>,
    /// The playhead as drawn while playing: smooth, where the engine's
    /// position moves once per audio block (`playhead.rs`).
    clock: crate::playhead::Clock,
    head: Option<Ticks>,
}

fn timeline_of(project: &Project) -> Timeline {
    let tempo = TempoMap::new(project.tempo.clone())
        .unwrap_or_else(|_| TempoMap::constant(120.0).expect("120 BPM is valid"));
    let sig = project.time_signature;
    let meter = MeterMap::constant(Meter {
        numerator: sig.numerator.into(),
        denominator: sig.denominator.into(),
    })
    .unwrap_or_else(|_| MeterMap::constant(Meter::FOUR_FOUR).expect("4/4 is valid"));
    Timeline::new(tempo, meter)
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
            follow: crate::settings::Follow::Page,
            follow_paused: false,
            follow_last: crate::settings::Follow::Page,
            time_format: crate::timefmt::TimeFormat::MinSec,
            test_tone: false,
            settings_window: Default::default(),
            cpu_peak: 0.0,
            cpu_taken: None,
            clip_memory: 0,
            clock: Default::default(),
            head: None,
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
        self.state().playing || self.settle_until.is_some_and(|t| Instant::now() < t)
    }

    fn settle(&mut self) {
        // The engine applies commands at its next block (a few ms); the
        // margin covers large buffers.
        self.settle_until = Some(Instant::now() + Duration::from_millis(300));
    }

    fn send(&mut self, command: Command) {
        crate::trace::log(|| format!("send {command:?}"));
        self.settle();
        // A full queue means the engine is not running; the UI shows why.
        let _ = self.audio.engine.send(command);
    }

    pub fn play_stop(&mut self) {
        let command = if self.state().playing {
            Command::Stop
        } else {
            Command::Play
        };
        self.send(command);
    }

    /// Applies saved metronome settings.
    pub fn restore_metronome(&mut self, settings: MetronomeSettings) {
        self.metronome = settings;
        self.send(Command::SetMetronome(settings));
    }

    /// Shift+Space: stop where playback is, or continue from the playhead.
    pub fn pause_continue(&mut self) {
        let command = if self.state().playing {
            Command::Pause
        } else {
            Command::Continue
        };
        self.send(command);
    }

    pub fn locate(&mut self, at: Ticks) {
        self.send(Command::Locate(at));
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
