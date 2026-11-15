pub mod analyzer;
pub mod delay;
pub mod dynamics;
pub mod effect;
pub mod eq;
pub mod hosted;
pub mod instrument;
mod line;
pub mod params;
pub mod reverb;
pub mod sampler;
pub mod synth;
pub mod utility;

pub use analyzer::{Read, Reader, Tap};
pub use dynamics::Reading as MeterReading;
pub use dynamics::{Compressor, Limiter, Reduction};
pub use effect::{Block, Description, Effect, Key, Stereo, Values};
pub use eq::Eq;
pub use hosted::{
    Cost, FAILED_AFTER_FRAMES, Guard, Hosted, HostedInstrument, MOST_FRAMES, MOST_POINTS,
    MOST_QUEUES, Mark, Outcome, ParamBlock, Refused, Reply, Status,
};
pub use instrument::Instrument;
pub use params::ParamInfo;
pub use sampler::Sampler;
pub use synth::{Preset, Synth};
pub use utility::Utility;

pub static INSTRUMENTS: &[(&str, &[ParamInfo])] = &[
    ("synth", synth::params::TABLE),
    (sampler::NAME, sampler::TABLE),
];

pub fn instrument_params(name: &str) -> Option<&'static [ParamInfo]> {
    INSTRUMENTS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, table)| *table)
}

pub static EFFECTS: &[&Description] = &[
    &utility::DESCRIPTION,
    &eq::DESCRIPTION,
    &delay::DESCRIPTION,
    &reverb::DESCRIPTION,
    &dynamics::compressor::DESCRIPTION,
    &dynamics::limiter::DESCRIPTION,
];

pub static SIDECHAINED: &[&str] = &[dynamics::compressor::NAME];

pub fn takes_sidechain(name: &str) -> bool {
    SIDECHAINED.contains(&name)
}

pub fn timing_parameter(name: &str, id: u32) -> bool {
    (name, id) == (dynamics::limiter::NAME, dynamics::limiter::LOOK)
}

pub fn automatable(name: &str, id: u32) -> bool {
    !timing_parameter(name, id)
}

pub fn effect_description(name: &str) -> Option<&'static Description> {
    EFFECTS
        .iter()
        .copied()
        .find(|description| description.name == name)
}

pub fn effect_params(name: &str) -> Option<&'static [ParamInfo]> {
    effect_description(name).map(|description| description.table)
}

pub fn effect_latency(name: &str, values: Values<'_>) -> Option<u32> {
    effect_description(name).map(|description| (description.latency)(values))
}

pub fn effect_tail(name: &str, values: Values<'_>) -> Option<u32> {
    effect_description(name).map(|description| (description.tail)(values))
}

pub fn effect_tail_at(name: &str, values: Values<'_>, bpm: f32, rate: u32) -> Option<u32> {
    match name {
        delay::NAME => Some(delay::tail_frames(values, bpm, rate)),
        reverb::NAME => Some(reverb::tail_frames(values, rate)),
        _ => effect_tail(name, values)
            .map(|frames| (u64::from(frames) * u64::from(rate) / 48_000) as u32),
    }
}

pub fn effect(name: &str) -> Option<Box<dyn Effect>> {
    effect_built(name, None).map(|(effect, _)| effect)
}

pub fn effect_with_meter(name: &str) -> Option<(Box<dyn Effect>, Option<Reduction>)> {
    effect_built(name, None).map(|(effect, published)| (effect, published.meter))
}

pub fn effect_tapped(name: &str, tap: Option<Tap>) -> Option<Box<dyn Effect>> {
    effect_built(name, tap).map(|(effect, _)| effect)
}

pub struct Published {
    pub meter: Option<Reduction>,
    pub onset: Option<delay::Onset>,
}

pub fn effect_built(name: &str, tap: Option<Tap>) -> Option<(Box<dyn Effect>, Published)> {
    let nothing = Published {
        meter: None,
        onset: None,
    };
    match name {
        utility::NAME => Some((Box::new(Utility::new()), nothing)),
        eq::NAME => Some((Box::new(Eq::with_tap(tap)), nothing)),
        delay::NAME => {
            let (device, onset) = delay::Delay::build();
            Some((
                Box::new(device),
                Published {
                    meter: None,
                    onset: Some(onset),
                },
            ))
        }
        reverb::NAME => Some((Box::new(reverb::Reverb::new()), nothing)),
        dynamics::compressor::NAME => {
            let (device, meter) = Compressor::build();
            Some((
                Box::new(device),
                Published {
                    meter: Some(meter),
                    ..nothing
                },
            ))
        }
        dynamics::limiter::NAME => {
            let (device, meter) = Limiter::build();
            Some((
                Box::new(device),
                Published {
                    meter: Some(meter),
                    ..nothing
                },
            ))
        }
        _ => None,
    }
}

pub fn watches_spectrum(name: &str) -> bool {
    name == eq::NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_effect_is_in_both_tables() {
        assert_eq!(
            EFFECTS.len(),
            6,
            "Prism, Iris, Parallax, Afterglow, Gravity and Horizon"
        );
        for (name, watched) in [
            (utility::NAME, false),
            (eq::NAME, true),
            (delay::NAME, false),
            (reverb::NAME, false),
            (dynamics::compressor::NAME, false),
            (dynamics::limiter::NAME, false),
        ] {
            assert!(effect_params(name).is_some(), "{name} has a table");
            assert!(effect(name).is_some(), "{name} is built");
            assert!(
                effect_tapped(name, None).is_some(),
                "{name} builds untapped"
            );
            assert!(
                effect_with_meter(name).is_some(),
                "{name} is built with its meter"
            );
            assert_eq!(watches_spectrum(name), watched, "{name}");
        }
        assert!(effect_params("nothing").is_none());
        assert!(effect("nothing").is_none());
    }

    #[test]
    fn a_device_with_a_meter_hands_out_its_handle() {
        for name in [dynamics::compressor::NAME, dynamics::limiter::NAME] {
            let (effect, published) = effect_built(name, None).expect("a device");
            let meter = published.meter.expect("a meter");
            assert_eq!(meter.take().reduction_db, 0.0);
            assert!(published.onset.is_none());
            drop(effect);
        }
        let (effect, published) = effect_built(delay::NAME, None).expect("a device");
        assert!(
            published.meter.is_none(),
            "a delay has no gain-reduction meter"
        );
        assert_eq!(
            published.onset.expect("a repeat clock").seconds(),
            None,
            "nothing has entered it"
        );
        drop(effect);
        for name in [utility::NAME, eq::NAME, reverb::NAME] {
            let (_, published) = effect_built(name, None).expect("a device");
            assert!(published.onset.is_none(), "{name} publishes no clock");
        }

        assert!(takes_sidechain(dynamics::compressor::NAME));
        assert!(!takes_sidechain(dynamics::limiter::NAME));
        assert!(!takes_sidechain(utility::NAME));
        for name in SIDECHAINED {
            assert!(EFFECTS.iter().any(|d| d.name == *name), "{name} exists");
        }
    }

    #[test]
    fn a_description_names_the_device_it_describes() {
        assert!(!EFFECTS.is_empty());
        for description in EFFECTS {
            let name = description.name;
            assert_eq!(effect_description(name).map(|d| d.name), Some(name));
            assert_eq!(description.table.len(), effect_params(name).unwrap().len());

            let defaults: Vec<(u32, f32)> = description
                .table
                .iter()
                .map(|p| (p.id, p.normalised(p.default)))
                .collect();

            let fresh = effect(name).unwrap();
            assert_eq!(effect_latency(name, &defaults), Some(fresh.latency()));
            assert_eq!(effect_tail(name, &defaults), Some(fresh.tail()));
        }
        assert_eq!(effect_latency("nothing", &[]), None);
        assert_eq!(effect_tail("nothing", &[]), None);
    }

    #[test]
    fn every_built_in_s_table_fits_one_block() {
        for description in EFFECTS {
            assert!(
                description.table.len() <= MOST_QUEUES,
                "{} names {} parameters of {MOST_QUEUES}",
                description.name,
                description.table.len()
            );
        }
    }
}
