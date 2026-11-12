//! Runs one scenario by name: its setup, then its measured function.

use lumen_engine::testing::latent;
use lumen_project::to_bytes;

use super::measured::*;
use super::scenarios::*;

/// Runs one scenario. Returns false for an unknown name.
pub fn run(name: &str) -> bool {
    match name {
        "engine_crossfades_64" => {
            let mut engine = crossfading_clips();
            measured_engine_crossfades_64(&mut engine);
        }
        "engine_clips_64" => {
            let mut engine = playing_clips(64, Mix::Direct);
            measured_engine_clips_64(&mut engine);
        }
        "engine_routed_64" => {
            let mut engine = playing_clips(64, Mix::Routed);
            measured_engine_routed_64(&mut engine);
        }
        "engine_latent_64" => {
            let mut engine = playing_clips(64, Mix::Latent);
            measured_engine_latent_64(&mut engine);
        }
        "engine_latent_swaps_64" => {
            let (mut routing, mut engine) = playing_clips_routed(64, Mix::Latent);
            // Two plans with the same shape and different levels
            // (`latent`'s variants 0 and 6), compiled outside the measure.
            let plans = [latent(64, 0), latent(64, 6)];
            for block in 0..1_000 {
                assert!(routing.set(&plans[block % 2]).expect("a valid routing"));
                measured_engine_latent_swaps_64(&mut engine);
                routing.collect_garbage();
            }
        }
        "engine_automated_64" => {
            let mut engine = playing_clips(64, Mix::Automated);
            measured_engine_automated_64(&mut engine);
        }
        "engine_synth_64" => {
            let mut engine = playing_synths(64);
            measured_engine_synth_64(&mut engine);
        }
        "engine_hosted_64" => {
            let (_routing, mut engine) = playing_hosted(64, false);
            measured_engine_hosted_64(&mut engine);
        }
        "engine_plugin_lanes_64" => {
            let (_routing, mut engine) = playing_hosted(64, true);
            measured_engine_plugin_lanes_64(&mut engine);
        }
        "engine_transport" => {
            let (handle, mut engine) = transport_only();
            measured_engine_transport(handle, &mut engine);
        }
        "engine_streamed_64" => {
            let (_pool, mut engine) = playing_streams();
            measured_engine_streamed_64(&mut engine);
        }
        "stream_plan_64" => {
            let scene = streamed_scene();
            measured_stream_plan_64(&scene);
        }
        "tempo_conversions" => {
            let map = busy_tempo_map();
            measured_tempo_conversions(&map);
        }
        "project_serialize" => {
            let project = quality_bar_project();
            measured_project_serialize(&project);
        }
        "project_parse" => {
            let bytes = to_bytes(&quality_bar_project()).expect("valid project");
            measured_project_parse(&bytes);
        }
        _ => return false,
    }
    true
}
