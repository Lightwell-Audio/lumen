use lumen_core::time::Ticks;
use lumen_project::model::Id;

use super::MeterShown;
use crate::audio::Status;
use crate::{App, settings};

impl App {
    pub fn audio_running(&self) -> bool {
        matches!(self.transport.audio.status, Status::Running { .. })
    }

    pub fn audio_driver(&self) -> &str {
        &self.transport.audio.driver
    }

    pub fn audio_off_reason(&self) -> Option<&str> {
        match &self.transport.audio.status {
            Status::Off(why) => Some(why),
            _ => None,
        }
    }

    pub fn audio_notice(&self) -> Option<&str> {
        self.transport.audio.notice.as_deref()
    }

    pub fn audio_can_change(&self) -> bool {
        self.transport.audio.can_change()
    }

    pub fn audio_can_switch(&self) -> bool {
        self.transport.audio.can_switch()
    }

    pub fn audio_switched_unread(&mut self) -> bool {
        self.transport.audio.switched_unread()
    }

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

    pub fn transport_state(&self) -> lumen_engine::TransportSnapshot {
        self.transport.state()
    }

    pub fn output_latency_frames(&self) -> i64 {
        self.transport
            .audio
            .info
            .as_ref()
            .map_or(0, |info| i64::from(info.output_latency))
    }

    pub fn drawn_playhead(&self) -> Ticks {
        self.transport.playhead()
    }

    pub fn cause_dropouts(&mut self, n: u64) {
        if let Some(meters) = self.transport.audio.meters() {
            meters.count_dropouts(n);
        }
    }

    pub fn set_test_latency(&mut self, id: Id, frames: u32) {
        let latent = &mut self.lanes.devices.latent;
        if frames == 0 {
            latent.remove(&id);
        } else {
            latent.insert(id, frames);
        }
    }

    pub fn compensation(&self) -> u32 {
        self.transport.audio.mixer.compensation().frames
    }

    pub fn heard(&mut self) -> Vec<(f32, f32)> {
        self.heard_frames()
            .into_iter()
            .map(|[l, r, ..]| (l, r))
            .collect()
    }

    pub fn heard_all(&mut self) -> Vec<[f32; 4]> {
        self.heard_frames()
    }

    pub fn meter(&self, id: Id) -> MeterShown {
        let reading = self.arrangement.meters.reading(id.0);
        MeterShown {
            peak_db: reading.peak[0].max(reading.peak[1]),
            rms_db: reading.rms[0].max(reading.rms[1]),
            hold_db: reading.hold_db(),
            clipped: reading.clipped,
        }
    }

    pub fn open_inputs(&self) -> usize {
        self.transport.audio.open_inputs().len()
    }

    pub fn set_audio_input(&mut self, id: Id, input: lumen_project::model::AudioInput) {
        self.document.set_audio_input(&[id], Some(input));
    }

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
