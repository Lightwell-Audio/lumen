//! The scenarios' setup: deterministic data and fixed iteration counts, one
//! module per kind of work.

mod engine;
mod files;

pub(crate) use engine::{
    crossfading_clips, playing_clips, playing_clips_routed, playing_hosted, playing_streams,
    playing_synths, transport_only,
};
pub(crate) use files::{busy_tempo_map, quality_bar_project, streamed_scene};

use lumen_engine::{ClipPlayer, Engine, Mixer};

/// Every scenario, with what it measures.
pub const SCENARIOS: &[(&str, &str)] = &[
    (
        "engine_crossfades_64",
        "1,000 blocks of 128 frames with 64 simultaneous stereo equal-power crossfades between overlapping clips",
    ),
    (
        "engine_clips_64",
        "1,000 blocks of 128 frames with 64 stereo clips sounding",
    ),
    (
        "engine_routed_64",
        "1,000 blocks of 128 frames: 64 tracks in 8 groups, each with two sends",
    ),
    (
        "engine_latent_64",
        "1,000 blocks of 128 frames: the routed mix with delay compensation and a sidechain",
    ),
    (
        "engine_latent_swaps_64",
        "1,000 blocks of 128 frames: the compensated mix with new levels every block (a fader drag)",
    ),
    (
        "engine_automated_64",
        "1,000 blocks of 128 frames: the routed mix with a fader, pan, send, trim and mute lane on its 64 tracks",
    ),
    (
        "engine_synth_64",
        "1,000 blocks of 128 frames: 64 MIDI tracks each playing a four-note chord on the synth, through the routed mix",
    ),
    (
        "engine_hosted_64",
        "1,000 blocks of 128 frames: 64 MIDI tracks, each a hosted instrument and a hosted effect (fake hosts that answer inside submit), through the routed mix",
    ),
    (
        "engine_plugin_lanes_64",
        "1,000 blocks of 128 frames: the hosted mix with a curved and a stepped parameter lane on each of its 64 hosted effects",
    ),
    (
        "engine_transport",
        "2,000 blocks of 128 frames: loop wraps, locates and the metronome",
    ),
    (
        "engine_streamed_64",
        "1,000 blocks of 128 frames with 64 stereo clips sounding, played from disk pages all in memory",
    ),
    (
        "stream_plan_64",
        "50 disk-thread plans of 64 streamed files with a loop, markers and a cue",
    ),
    (
        "tempo_conversions",
        "100,000 tick/sample conversions on a 64-point tempo map",
    ),
    (
        "project_serialize",
        "Serialize a quality-bar project (200 tracks, about 20,000 clips)",
    ),
    (
        "project_parse",
        "Parse, migrate and validate the same project",
    ),
];

pub(crate) const RATE: u32 = 48_000;
pub(crate) const FRAMES: usize = 128;

pub(crate) type Player = Engine<Mixer<ClipPlayer>>;
/// The engine with the MIDI instruments beside the audio clips.
pub(crate) type SynthPlayer =
    Engine<Mixer<lumen_engine::Layered<ClipPlayer, lumen_engine::MidiPlayer>>>;

/// How the clips of a scenario are mixed.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mix {
    /// All on one track, straight to the device.
    Direct,
    /// Each on its own track in the routed mix.
    Routed,
    /// Each on its own track in the latent mix.
    Latent,
    /// Each on its own track in the automated mix.
    Automated,
}
