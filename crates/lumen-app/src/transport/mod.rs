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

use std::time::Instant;

use lumen_core::time::{TempoMap, Ticks};
use lumen_engine::{Command, MetronomeSettings, Region, Timeline, TransportSnapshot};
use lumen_project::model::Project;

use crate::audio::{Audio, Choice};

pub use bar::punch_menu;
pub use fit::divider;
pub use midi_chip::{Lights as MidiLights, View as MidiView};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DiskReading {
    pub load: f32,
    pub underruns: u64,
    pub failed: u32,
}

pub struct Transport {
    pub audio: Audio,
    pub timeline: Timeline,
    timeline_pending: bool,
    pub loop_region: Region,
    clip_loop: Option<Region>,
    sent_loop: Region,
    editing: Option<String>,
    error: Option<String>,
    settle_until: Option<Instant>,
    pub clip_memory: usize,
    pub metronome: MetronomeSettings,
    metronome_gated: bool,
    pub follow: crate::settings::Follow,
    pub follow_paused: bool,
    follow_last: crate::settings::Follow,
    pub time_format: crate::timefmt::TimeFormat,
    test_tone: bool,
    midi_asked: Vec<crate::midi::Action>,
    pub open_preferences: Option<crate::preferences::Page>,
    pub disk: Option<DiskReading>,
    pub gate: Option<hold::Gate>,
    held: Option<hold::Held>,
    pub midi_lights: MidiLights,
    cpu_peak: f32,
    cpu_taken: Option<Instant>,
    pub(crate) budget: budget::Budget,
    dropouts: dropouts::Dropouts,
    late: dropouts::Dropouts,
    disk_base: u64,
    clock: crate::playhead::Clock,
    head: Option<Ticks>,
    located: Option<(Ticks, Ticks)>,
    pub played_from_stop: bool,
    pub panic_midi: bool,
    pub(crate) info: Option<crate::app::info::Info>,
    pub(crate) keyboard_readout: Option<String>,
    pub(crate) keyboard_clicked: bool,
    pub(crate) taps: usize,
    pub(crate) keyboard_readout_shown: bool,
    fit: fit::Fit,
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
        let loop_region = Region::new(Ticks::ZERO, timeline.meter.bar_start(4), false);
        Self {
            audio: Audio::start(choice, timeline.clone()),
            timeline,
            timeline_pending: false,
            loop_region,
            clip_loop: None,
            sent_loop: loop_region,
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
            taps: 0,
            keyboard_readout_shown: false,
            fit: Default::default(),
            requests: Vec::new(),
        }
    }

    pub fn state(&self) -> TransportSnapshot {
        self.audio.engine.transport()
    }

    pub fn advance_playhead(&mut self, now: f64) {
        let state = self.state();
        let rate = state.sample_rate;
        let region = self.played_loop();
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

    pub fn playhead(&self) -> Ticks {
        self.head.unwrap_or_else(|| self.state().ticks)
    }

    pub fn animating(&self) -> bool {
        let state = self.state();
        state.playing
            || state.recording
            || self.held.is_some()
            || self.settle_until.is_some_and(|t| Instant::now() < t)
    }

    fn settle(&mut self) {
        self.settle_until = Some(Instant::now() + crate::motion::ENGINE_SETTLE);
    }

    fn send(&mut self, mut command: Command) {
        if let Command::SetMetronome(settings) = &mut command
            && self.metronome_gated
        {
            settings.enabled = false;
        }
        crate::trace::log(|| format!("send {command:?}"));
        if matches!(command, Command::Stop) {
            self.audio.stop_sound_check();
        }
        self.settle();
        let _ = self.audio.engine.send(command);
    }

    fn start(&mut self, command: Command, at: Ticks) {
        self.drop_held();
        let Some(gate) = &self.gate else {
            self.send(command);
            return;
        };
        let sample = gate.sample_at(at);
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

    fn drop_held(&mut self) -> bool {
        let dropped = self.held.take().is_some();
        if dropped && let Some(gate) = &self.gate {
            gate.cuer.cue(None);
        }
        dropped
    }

    fn release(&mut self) {
        if let Some(held) = &self.held
            && held.due(self.gate.as_ref(), Instant::now())
        {
            let held = self.held.take().expect("checked above");
            self.send(held.command);
        }
    }

    pub fn panic(&mut self) {
        self.drop_held();
        self.send(Command::Panic);
        self.panic_midi = true;
    }

    pub fn play_stop(&mut self) {
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

    pub fn restore_metronome(&mut self, settings: MetronomeSettings) {
        self.metronome = settings;
        self.send(Command::SetMetronome(settings));
    }

    pub fn record(&mut self, count_in: u32) {
        let state = self.state();
        if state.playing {
            self.send(Command::Record { count_in });
        } else {
            self.played_from_stop = true;
            self.start(Command::Record { count_in }, state.marker);
        }
    }

    pub fn stop(&mut self) {
        self.drop_held();
        self.send(Command::Stop);
    }

    pub fn end_record(&mut self) {
        self.send(Command::EndRecord);
    }

    pub fn gate_metronome(&mut self, gated: bool) {
        if gated != self.metronome_gated {
            self.metronome_gated = gated;
            self.send(Command::SetMetronome(self.metronome));
        }
    }

    pub fn pause_continue(&mut self) {
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
            Some(_) if state.playing => self.start(Command::Locate(at), at),
            Some(gate) => {
                let held = self.held.take();
                gate.cuer.cue(Some(gate.sample_at(at)));
                self.send(Command::Locate(at));
                if let Some(held) = held {
                    self.start(held.command, at);
                }
            }
            None => self.send(Command::Locate(at)),
        }
    }

    pub fn position(&self) -> Ticks {
        let ticks = self.state().ticks;
        match self.located {
            Some((at, before)) if ticks == before => at,
            _ => ticks,
        }
    }

    pub fn scrub(&mut self, at: Ticks, style: lumen_engine::ScrubStyle) {
        self.send(Command::Scrub { at, style });
    }

    pub fn end_scrub(&mut self) {
        self.send(Command::EndScrub);
    }

    pub fn played_loop(&self) -> Region {
        self.clip_loop.unwrap_or(self.loop_region)
    }

    pub fn set_clip_loop(&mut self, region: Option<Region>) {
        self.clip_loop = region;
    }

    pub fn sent_loop(&self) -> Region {
        self.sent_loop
    }

    pub fn clip_looping(&self) -> bool {
        self.clip_loop.is_some()
    }

    pub fn follow_loop(&mut self, state: &lumen_project::model::TransportState) {
        self.loop_region = Region::new(state.loop_start, state.loop_end, state.loop_enabled);
        let played = self.played_loop();
        if played != self.sent_loop {
            self.sent_loop = played;
            self.send(Command::SetLoop(played));
        }
    }

    pub fn toggle_metronome(&mut self) {
        self.metronome.enabled = !self.metronome.enabled;
        self.send(Command::SetMetronome(self.metronome));
    }

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

    pub fn restore_follow(&mut self, follow: crate::settings::Follow) {
        self.follow = follow;
        if follow != crate::settings::Follow::Off {
            self.follow_last = follow;
        }
    }

    pub fn timeline_changed(&mut self, project: &Project) {
        let timeline = timeline_of(project);
        if timeline == self.timeline {
            return;
        }
        self.timeline = timeline;
        self.timeline_pending = true;
        self.settle();
    }

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
