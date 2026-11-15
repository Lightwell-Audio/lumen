mod dispatch;
mod measured;
mod scenarios;

pub use dispatch::run;
pub use measured::*;
pub use scenarios::SCENARIOS;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_scenario_runs() {
        for name in [
            "engine_crossfades_64",
            "engine_clips_64",
            "engine_routed_64",
            "engine_latent_64",
            "engine_latent_swaps_64",
            "engine_automated_64",
            "engine_synth_64",
            "engine_hosted_64",
            "engine_plugin_lanes_64",
            "engine_transport",
            "engine_streamed_64",
            "stream_plan_64",
            "tempo_conversions",
        ] {
            assert!(run(name), "{name}");
        }
        assert!(!run("no_such_scenario"));
        assert!(SCENARIOS.iter().all(|(name, _)| !name.contains(' ')));
    }
}
