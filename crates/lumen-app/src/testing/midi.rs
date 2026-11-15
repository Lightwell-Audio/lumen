use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use crate::App;

impl App {
    pub fn recording(&self) -> bool {
        self.is_recording()
    }

    pub fn live_take(&self) -> Option<(Ticks, Ticks, usize)> {
        self.take_so_far()
    }

    pub fn keeps_played_notes(&self) -> bool {
        self.settings.record.keep_played
    }

    pub fn played_messages(&self) -> usize {
        self.record.played.len()
    }

    pub fn live_range(&self) -> Option<(u8, u8)> {
        let shown = self.arrangement.preview.as_ref()?;
        let track = shown
            .tracks
            .iter()
            .find(|t| t.armed && t.kind == lumen_project::model::TrackKind::Midi)?;
        crate::piano_roll::mini::track_range(&track.clips)
    }

    pub fn drawn_clips(&self, index: usize) -> Vec<std::sync::Arc<lumen_project::model::Clip>> {
        let project = self
            .arrangement
            .preview
            .as_deref()
            .unwrap_or_else(|| self.project());
        project
            .tracks
            .get(index)
            .map(|t| t.clips.clone())
            .unwrap_or_default()
    }

    pub fn track_pitch_range(&self, index: usize) -> Option<(u8, u8)> {
        crate::piano_roll::mini::track_range(&self.project().tracks.get(index)?.clips)
    }

    pub fn overdub_on(&self) -> bool {
        self.settings.record.overdub
    }

    pub fn count_in_bars(&self) -> u32 {
        self.settings.record.count_in.bars()
    }

    pub fn record_notice(&self) -> Option<&str> {
        self.file_notice.as_deref()
    }

    pub fn set_midi_from(&mut self, from: lumen_project::model::MidiFrom) {
        let ids = self.selected_midi_tracks();
        self.document.set_midi_from(&ids, from);
    }

    pub fn set_monitor_of_selected(&mut self, monitor: lumen_project::model::Monitor) {
        let ids = self.selected_midi_tracks();
        self.document.set_monitor(&ids, monitor);
    }

    pub fn play_live(&mut self, msg: lumen_core::midi::Msg) -> bool {
        self.play_key(msg)
    }

    pub fn audition(&mut self, track: Id, pitch: u8, velocity: u8) -> bool {
        self.audition_play(track, pitch, velocity)
    }

    pub fn audition_chord(&mut self, track: Id, tones: &[(u8, u8)]) -> bool {
        let tones: Vec<crate::audition::Tone> = tones
            .iter()
            .map(|&(pitch, velocity)| crate::audition::Tone { pitch, velocity })
            .collect();
        self.audition_tones(track, &tones, true)
    }

    pub fn release_audition(&mut self) {
        self.audition_release();
    }

    pub fn audition_sounding(&self) -> bool {
        self.audition.is_sounding()
    }

    pub fn set_audition_setting(&mut self, on: bool) {
        self.settings.midi.audition = on;
    }

    pub fn computer_keyboard_on(&self) -> bool {
        self.keyboard.is_on()
    }

    pub fn computer_keyboard_held(&self) -> Vec<u8> {
        self.keyboard.held_notes()
    }

    pub fn computer_keyboard_sent(&mut self) -> Vec<lumen_core::midi::Msg> {
        self.keyboard.take_sent()
    }

    pub fn midi_chip_lights(&self) -> (f32, f32) {
        (
            self.transport.midi_lights.input,
            self.transport.midi_lights.output,
        )
    }

    pub fn header_light(&self, ctx: &egui::Context, track: Id) -> f32 {
        crate::arrangement::listen_light_shown(ctx, track)
    }

    pub fn midi_row_light(&self, direction: lumen_midi::Direction, name: &str) -> Option<f32> {
        self.transport
            .midi_lights
            .rows
            .iter()
            .find(|(id, _)| id.direction == direction && id.name == name)
            .map(|(_, b)| *b)
    }

    pub fn saved_midi_ports(&self) -> Vec<(String, String, bool, bool)> {
        let mut ports: Vec<_> = self
            .settings
            .midi
            .ports
            .iter()
            .map(|p| (p.direction.clone(), p.name.clone(), p.track, p.remote))
            .collect();
        ports.sort();
        ports
    }
}
