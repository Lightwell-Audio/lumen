//! Test seams for playing and recording MIDI (M5): live input, recording
//! takes, auditioning, the computer keyboard and the MIDI lights.

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::Id;

use crate::App;

impl App {
    /// Whether a recording is under way, or counting in to one (M5 slice 6).
    pub fn recording(&self) -> bool {
        self.is_recording()
    }

    /// The take as the arrangement draws it each frame (after MT-MIDI-7):
    /// where it begins and ends, and the notes played so far on the first
    /// track recording. None when nothing records.
    pub fn live_take(&self) -> Option<(Ticks, Ticks, usize)> {
        self.take_so_far()
    }

    /// The pitch range the first track recording is laid out to in what the
    /// arrangement draws this frame, the take included
    /// (`piano_roll::mini::Fit`). None before a note is played.
    pub fn live_range(&self) -> Option<(u8, u8)> {
        let shown = self.arrangement.preview.as_ref()?;
        let track = shown
            .tracks
            .iter()
            .find(|t| t.armed && t.kind == lumen_project::model::TrackKind::Midi)?;
        crate::piano_roll::mini::track_range(&track.clips)
    }

    /// The clips of track `index` as the arrangement draws them this frame:
    /// while recording, the take placed as far as it has gone.
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

    /// The pitch range every MIDI clip on track `index` is laid out to.
    pub fn track_pitch_range(&self, index: usize) -> Option<(u8, u8)> {
        crate::piano_roll::mini::track_range(&self.project().tracks.get(index)?.clips)
    }

    /// Whether Overdub is on: a take merges into the clip under it (M5
    /// slice 6; `settings.record`).
    pub fn overdub_on(&self) -> bool {
        self.settings.record.overdub
    }

    /// The count-in's length in bars, zero for none (M5 slice 6).
    pub fn count_in_bars(&self) -> u32 {
        self.settings.record.count_in.bars()
    }

    /// The notice banner's text, such as "Arm a MIDI track to record."
    /// (M5 slice 6).
    pub fn record_notice(&self) -> Option<&str> {
        self.file_notice.as_deref()
    }

    /// Sets MIDI From on the selected MIDI tracks, as the menu would.
    pub fn set_midi_from(&mut self, from: lumen_project::model::MidiFrom) {
        let ids = self.selected_midi_tracks();
        self.document.set_midi_from(&ids, from);
    }

    /// Sets Monitor on the selected MIDI tracks, as the menu would.
    pub fn set_monitor_of_selected(&mut self, monitor: lumen_project::model::Monitor) {
        let ids = self.selected_midi_tracks();
        self.document.set_monitor(&ids, monitor);
    }

    /// Plays `msg` live, as the computer keyboard does (M5 slice 2).
    pub fn play_live(&mut self, msg: lumen_core::midi::Msg) -> bool {
        self.play_key(msg)
    }

    /// Previews `pitch` on `track` as the piano roll does (M5 slice 3d).
    pub fn audition(&mut self, track: Id, pitch: u8, velocity: u8) -> bool {
        self.audition_play(track, pitch, velocity)
    }

    /// Previews a chord of `(pitch, velocity)` notes on `track`, as a press
    /// on picked notes does: each note its own tone, so one on a key the
    /// track's clip is sounding on hardware is skipped and the rest play.
    pub fn audition_chord(&mut self, track: Id, tones: &[(u8, u8)]) -> bool {
        let tones: Vec<crate::audition::Tone> = tones
            .iter()
            .map(|&(pitch, velocity)| crate::audition::Tone { pitch, velocity })
            .collect();
        self.audition_tones(track, &tones, true)
    }

    /// Lets go of the preview (the end of a gesture).
    pub fn release_audition(&mut self) {
        self.audition_release();
    }

    /// Whether a previewed note is sounding.
    pub fn audition_sounding(&self) -> bool {
        self.audition.is_sounding()
    }

    /// Turns the piano roll's audition on or off, in the settings.
    pub fn set_audition_setting(&mut self, on: bool) {
        self.settings.midi.audition = on;
    }

    /// Whether the computer keyboard mode is on (M5 slice 2).
    pub fn computer_keyboard_on(&self) -> bool {
        self.keyboard.is_on()
    }

    /// The notes the computer keyboard holds now, lowest first.
    pub fn computer_keyboard_held(&self) -> Vec<u8> {
        self.keyboard.held_notes()
    }

    /// The messages the computer keyboard sent since the last call.
    pub fn computer_keyboard_sent(&mut self) -> Vec<lumen_core::midi::Msg> {
        self.keyboard.take_sent()
    }

    /// How bright the MIDI chip's IN and OUT lights were when last drawn
    /// (0 at rest, 1 at a flash, fading over `motion::NOTE`).
    pub fn midi_chip_lights(&self) -> (f32, f32) {
        (
            self.transport.midi_lights.input,
            self.transport.midi_lights.output,
        )
    }

    /// How bright the light after a MIDI track's name was when last drawn
    /// (0 at rest, and always 0 while the track does not listen).
    pub fn header_light(&self, ctx: &egui::Context, track: Id) -> f32 {
        crate::arrangement::listen_light_shown(ctx, track)
    }

    /// How bright the popover's light for a port was when last drawn
    /// (`None` when the popover is not showing it).
    pub fn midi_row_light(&self, direction: lumen_midi::Direction, name: &str) -> Option<f32> {
        self.transport
            .midi_lights
            .rows
            .iter()
            .find(|(id, _)| id.direction == direction && id.name == name)
            .map(|(_, b)| *b)
    }

    /// The MIDI switches the settings hold: (direction, name, track, remote).
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
