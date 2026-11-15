use lumen_core::time::Ticks;
use lumen_project::model::{Id, Route};

use crate::app::drum_routing::SoundOutput;
use crate::midi::play;
use crate::{App, midi};

impl App {
    pub fn sound_outputs_offered(&self, track: Id) -> Vec<String> {
        self.sound_output_choices(track)
            .into_iter()
            .map(|choice| match choice {
                SoundOutput::Main => "Main".to_owned(),
                SoundOutput::Pair { bus, name, told } => {
                    let how = if told { "" } else { ", set in the plugin" };
                    format!("Pair {bus} \u{b7} {name}{how}")
                }
                SoundOutput::Track { name, .. } => format!("Track \u{b7} {name}"),
            })
            .collect()
    }

    pub fn route_sound(&mut self, track: Id, map: Id, input: u8, to: &str) -> bool {
        let route = match to {
            "main" => Route::Main,
            other => match other.strip_prefix("output ") {
                Some(bus) => match bus.parse::<u32>() {
                    Ok(bus) => Route::Output(bus),
                    Err(_) => return false,
                },
                None => match other.strip_prefix("track ") {
                    Some(id) => match id.trim().parse::<u64>() {
                        Ok(id) => Route::Track(Id(id)),
                        Err(_) => return false,
                    },
                    None => return false,
                },
            },
        };
        self.document
            .route_drum_sound(track, map, input, route)
            .is_some()
    }

    pub fn sound_plays_on(&self, track: Id, map: Id, input: u8) -> Option<Id> {
        self.document.sound_playing_track(track, map, input)
    }

    pub fn sounds_shown_on(&self, track: Id) -> Vec<(u8, String, bool)> {
        self.sounds_on_track(track)
            .into_iter()
            .map(|sound| (sound.input, sound.name, sound.sounding))
            .collect()
    }

    pub fn sounds_sounding_now(&self, track: Id) -> Vec<u8> {
        self.sounds_sounding(track)
    }

    pub fn sounds_sounding_at(&self, track: Id, at: Ticks) -> Vec<u8> {
        let Some(held) = self
            .document
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
        else {
            return Vec::new();
        };
        midi::runs::sounding_inputs(self.document.project(), held, at)
    }

    pub fn track_shows_a_sound(&self, track: Id) -> bool {
        crate::app::drum_routing::made_for_sounds(self.document.project(), track)
    }

    pub fn pad_outputs_of(&self, track: Id) -> Vec<(u8, u32)> {
        let Some(held) = self
            .document
            .project()
            .tracks
            .iter()
            .find(|t| t.id == track)
        else {
            return Vec::new();
        };
        let pads = play::pad_outputs(self.document.project(), held);
        (0..=u8::MAX)
            .filter(|input| pads.pair_of(*input) != 0)
            .map(|input| (input, pads.pair_of(input)))
            .collect()
    }

    pub fn sound_pairs_offered(&self, track: Id) -> usize {
        self.sound_output_pairs(track).len()
    }
}
