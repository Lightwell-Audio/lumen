//! Test seams for per-sound routing (M8 slice D4): what the map editor's
//! Output column offers, what a track made for sounds shows, which sounds
//! are sounding, and the pad pairs the sampler (B5) is built with — driven
//! without drawing any of it.

use lumen_core::time::Ticks;
use lumen_project::model::{Id, Route};

use crate::app::drum_routing::SoundOutput;
use crate::midi::play;
use crate::{App, midi};

impl App {
    /// The outputs the map editor offers the sound of `map` on `track`, as
    /// the column words them: "Main", "Pair 2 · Hats", "set in the plugin",
    /// "Track · Kick 2".
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

    /// Routes the sound at `input` of `map` the way the Output column does
    /// (one undo step): "Main", "output 2", or "track <id>". False when the
    /// route cannot be given.
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

    /// The track the sound at `input` of `map` is heard on now, if the track
    /// it belongs to still has it.
    pub fn sound_plays_on(&self, track: Id, map: Id, input: u8) -> Option<Id> {
        self.document.sound_playing_track(track, map, input)
    }

    /// Every sound heard on `track` although another track holds it, as
    /// (input note, name, lit now): a header's icon and its light.
    pub fn sounds_shown_on(&self, track: Id) -> Vec<(u8, String, bool)> {
        self.sounds_on_track(track)
            .into_iter()
            .map(|sound| (sound.input, sound.name, sound.sounding))
            .collect()
    }

    /// The input notes of `track`'s own sounds sounding at the playhead.
    pub fn sounds_sounding_now(&self, track: Id) -> Vec<u8> {
        self.sounds_sounding(track)
    }

    /// The input notes of `track`'s sounds sounding at `at` (an arrangement
    /// position, whether or not the transport is there): the light's rule
    /// without waiting for playback.
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

    /// Whether `track` is a track made for sounds (D4): its header shows the
    /// first sound's icon in place of the colour chip.
    pub fn track_shows_a_sound(&self, track: Id) -> bool {
        crate::app::drum_routing::made_for_sounds(self.document.project(), track)
    }

    /// The pair each of `track`'s drum sounds goes to, as the sampler (B5) is
    /// told when it is built: (input note, pair), only the sounds that leave
    /// by a pair. Empty for a track with no map or no such sound.
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

    /// How many of the project's sounds the instrument of `track` offers a
    /// pair for: what "Route all sounds to tracks" would hand a track each.
    pub fn sound_pairs_offered(&self, track: Id) -> usize {
        self.sound_output_pairs(track).len()
    }
}
