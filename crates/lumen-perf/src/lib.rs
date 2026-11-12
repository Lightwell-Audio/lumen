//! Performance regression gates (ADR-0022).
//!
//! Each scenario sets up its data, then runs its measured work inside a
//! function named `measured_<scenario>`. `perf-gate` runs each scenario under
//! Valgrind's Callgrind, counting instructions only inside that function
//! (`--toggle-collect`), and compares the count with `perf/baseline.json`.
//!
//! Instruction counts do not depend on machine load, so unlike wall-clock
//! benchmarks they can gate CI on shared runners. Wall-clock numbers still
//! come from the divan benchmarks and the quality-bar measurements.
//!
//! Scenarios must be deterministic: fixed seeds, fixed iteration counts, no
//! I/O and no threads in the measured part.
//!
//! The setup of each scenario is in [`scenarios`], the measured functions in
//! [`measured`], and the name-to-scenario dispatch in [`dispatch`].

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
        // The project scenarios are slow in debug builds; run the cheap ones
        // here, and check the names of the rest resolve.
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
