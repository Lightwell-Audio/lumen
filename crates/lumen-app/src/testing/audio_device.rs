//! Test seams for the audio device and the transport: whether the
//! device runs, switching drivers, the playhead, what was heard and
//! what the meters show.

use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::MeterShown;
use crate::audio::Status;
use crate::{App, settings};

impl App {
    /// The audio device is running.
    pub fn audio_running(&self) -> bool {
        matches!(self.transport.audio.status, Status::Running { .. })
    }

    /// The driver model in use (`test`, `test-slow`, ...).
    pub fn audio_driver(&self) -> &str {
        &self.transport.audio.driver
    }

    /// Why audio is off, when it is.
    pub fn audio_off_reason(&self) -> Option<&str> {
        match &self.transport.audio.status {
            Status::Off(why) => Some(why),
            _ => None,
        }
    }

    /// What the audio page's notice says, if anything.
    pub fn audio_notice(&self) -> Option<&str> {
        self.transport.audio.notice.as_deref()
    }

    /// Whether the device can be changed now (no change is under way).
    pub fn audio_can_change(&self) -> bool {
        self.transport.audio.can_change()
    }

    /// Whether Audio On/Off can act now.
    pub fn audio_can_switch(&self) -> bool {
        self.transport.audio.can_switch()
    }

    /// Whether a switch of driver model has finished and its answer waits,
    /// unread: no frame has run since. Waiting on it draws no frame.
    pub fn audio_switched_unread(&mut self) -> bool {
        self.transport.audio.switched_unread()
    }

    /// Chooses the test device on `driver`, as the Audio page's device
    /// menu does, below the page: whether the change was taken (one under
    /// way refuses it).
    pub fn choose_audio_driver(&mut self, driver: &str) -> bool {
        self.transport.audio.apply(settings::AudioSettings {
            driver: driver.into(),
            device_id: "fake".into(),
            device_name: "Fake device".into(),
            sample_rate: None,
            buffer_frames: None,
            input_channels: Vec::new(),
            all_inputs: true,
            output_channels: vec![0, 1],
            cue_channels: Vec::new(),
        })
    }

    pub fn is_playing(&self) -> bool {
        self.transport.state().playing
    }

    pub fn playhead(&self) -> Ticks {
        self.transport.state().ticks
    }

    /// The engine's transport as of its last block: recording, count-in and
    /// playing flags, and the sample and tick positions (M5 slice 6).
    pub fn transport_state(&self) -> lumen_engine::TransportSnapshot {
        self.transport.state()
    }

    /// The output latency of the audio device, in frames: the playhead as
    /// the engine renders it leads what is heard by this much (M5 slice 6,
    /// the recorder's timing).
    pub fn output_latency_frames(&self) -> i64 {
        self.transport
            .audio
            .info
            .as_ref()
            .map_or(0, |info| i64::from(info.output_latency))
    }

    /// Where the playhead is drawn: smoothed while playing (`playhead.rs`).
    pub fn drawn_playhead(&self) -> Ticks {
        self.transport.playhead()
    }

    /// The test device drops `n` buffers, as a real one does under load.
    pub fn cause_dropouts(&mut self, n: u64) {
        if let Some(meters) = self.transport.audio.meters() {
            meters.count_dropouts(n);
        }
    }

    /// Puts the engine's synthetic latent device on track `id`, adding
    /// `frames` of latency (0 takes it off), as a device with latency will
    /// be from M6: what delay compensation is tested with (M4 slice 6).
    pub fn set_test_latency(&mut self, id: Id, frames: u32) {
        let latent = &mut self.lanes.devices.latent;
        if frames == 0 {
            latent.remove(&id);
        } else {
            latent.insert(id, frames);
        }
    }

    /// The delay compensation the engine was last sent, in frames.
    pub fn compensation(&self) -> u32 {
        self.transport.audio.mixer.compensation().frames
    }

    /// What the test device has played since the last call, as (left,
    /// right) frames: the audio a user would hear.
    pub fn heard(&mut self) -> Vec<(f32, f32)> {
        self.heard_frames()
            .into_iter()
            .map(|[l, r, ..]| (l, r))
            .collect()
    }

    /// What the test device has played since the last call, frame by
    /// frame: the main pair's left and right, then the cue pair's (M4
    /// slice 4; silent without a cue pair).
    pub fn heard_all(&mut self) -> Vec<[f32; 4]> {
        self.heard_frames()
    }

    /// What track `id`'s meter shows (M4 slice 5): the louder side's
    /// peak, RMS and hold line in dB, and the clip light.
    pub fn meter(&self, id: Id) -> MeterShown {
        let reading = self.arrangement.meters.reading(id.0);
        MeterShown {
            peak_db: reading.peak[0].max(reading.peak[1]),
            rms_db: reading.rms[0].max(reading.rms[1]),
            hold_db: reading.hold_db(),
            clipped: reading.clipped,
        }
    }

    /// How many device inputs are open (M9 slice R1).
    pub fn open_inputs(&self) -> usize {
        self.transport.audio.open_inputs().len()
    }

    /// Routes audio track `id` to `input`, as the In chip's menu does.
    pub fn set_audio_input(&mut self, id: Id, input: lumen_project::model::AudioInput) {
        self.document.set_audio_input(&[id], Some(input));
    }

    /// What device input `channel`'s meter shows (M9 slice R1).
    pub fn input_meter(&self, channel: usize) -> MeterShown {
        let reading = self.arrangement.meters.input_reading(channel);
        MeterShown {
            peak_db: reading.peak[0].max(reading.peak[1]),
            rms_db: reading.rms[0].max(reading.rms[1]),
            hold_db: reading.hold_db(),
            clipped: reading.clipped,
        }
    }
}
