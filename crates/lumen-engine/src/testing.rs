//! Routings for tests, benchmarks and the performance gate: the same
//! shapes everywhere, so a measured cost is the cost tested.

use lumen_core::pan::PanLaw;

use crate::mixer::{Destination, Device, NodeSpec, Routing, SendSpec, SidechainSpec, Tap};

/// The master's id in [`routed`].
pub const MASTER: u64 = 1_000_000;

/// A production-sized routing: `tracks` tracks (ids `0..tracks`) in eight
/// groups, each track with a post-fader send to one return and a
/// pre-fader send to another, everything into the master, at the default
/// pan law, the master's loudness measured (as the app does). Each
/// `variant` changes every level; an odd one also moves
/// track 0 out of its group to the master (a new shape, which
/// crossfades).
pub fn routed(tracks: u64, variant: u32) -> Routing {
    const GROUPS: u64 = 8;
    let group = |g: u64| MASTER + 10 + g;
    let returns = [MASTER + 1, MASTER + 2];
    let level = |i: u64| 0.5 + ((i + u64::from(variant)) % 5) as f32 * 0.1;
    let law = PanLaw::default();
    let mut nodes = vec![NodeSpec::unity(MASTER, Destination::Hardware(0))];
    for id in returns {
        nodes.push(NodeSpec {
            law,
            ..NodeSpec::unity(id, Destination::Node(MASTER))
        });
    }
    for g in 0..GROUPS {
        nodes.push(NodeSpec {
            law,
            volume: level(g),
            ..NodeSpec::unity(group(g), Destination::Node(MASTER))
        });
    }
    for i in 0..tracks {
        let output = if i == 0 && variant % 2 == 1 {
            Destination::Node(MASTER)
        } else {
            Destination::Node(group(i % GROUPS))
        };
        nodes.push(NodeSpec {
            volume: level(i),
            pan: ((i % 9) as f32 - 4.0) / 4.0,
            law,
            invert: i % 7 == 3,
            sends: vec![
                SendSpec {
                    to: returns[0],
                    gain: 0.5,
                    pre_fader: false,
                },
                SendSpec {
                    to: returns[1],
                    gain: 0.25,
                    pre_fader: true,
                },
            ],
            ..NodeSpec::unity(i, output)
        });
    }
    Routing {
        nodes,
        cue: None,
        measure: Some(MASTER),
    }
}

/// [`routed`] with delay compensation at work (M4 slice 6): every eighth
/// track and the master have a latent device (the synthetic delay), whose
/// latency each `variant` changes, and track 1 listens to track 0 through
/// a sidechain.
pub fn latent(tracks: u64, variant: u32) -> Routing {
    let mut routing = routed(tracks, variant);
    let delay = |frames: u32| Device::Delay {
        frames,
        listen: false,
    };
    for node in &mut routing.nodes {
        if node.id == MASTER {
            node.devices.push(delay(128));
        } else if node.id < tracks && node.id % 8 == 4 {
            node.devices.push(delay(64 + 32 * (variant % 3)));
        } else if node.id == 1 {
            node.devices.push(Device::Delay {
                frames: 0,
                listen: true,
            });
            node.sidechains.push(SidechainSpec {
                from: 0,
                tap: Tap::PostFx,
            });
        }
    }
    routing
}

/// [`routed`] with every track automated (M7 slice A0): a fader and a pan
/// lane on all of them, an input trim on every fourth, a mute lane that
/// steps on every fifth, and a lane on both sends. The lanes run the length
/// of eight bars at 120 BPM and 48 kHz and are curves, straight lines and
/// steps; each `variant` changes every value, with the same shape.
pub fn automated(tracks: u64, variant: u32) -> Routing {
    use lumen_core::automation::Shape;
    use lumen_core::curve::Curve;

    use crate::mixer::{AutoLane, AutoPoint, NodeAutomation};

    const SECONDS: i64 = 16;
    let lane = |seed: u64, count: i64, low: f32, high: f32, shape: &dyn Fn(i64) -> Shape| {
        let points = (0..count)
            .map(|k| {
                // A small deterministic scatter of values.
                let x = ((seed * 7919 + k as u64 * 104_729 + u64::from(variant) * 31) % 1000)
                    as f32
                    / 1000.0;
                AutoPoint {
                    at: k * SECONDS * 48_000 / (count - 1),
                    value: low + (high - low) * x,
                    shape: shape(k),
                }
            })
            .collect();
        AutoLane::new(points)
    };
    let mixed = |k: i64| match k % 3 {
        0 => Shape::Linear,
        1 => Shape::Curve(Curve::S_CURVE),
        _ => Shape::Step,
    };
    let mut routing = routed(tracks, variant);
    for node in routing.nodes.iter_mut().filter(|n| n.id < tracks) {
        let id = node.id;
        node.automation = Some(NodeAutomation {
            volume: lane(id, 33, -24.0, 0.0, &mixed),
            pan: lane(id + 1, 25, -1.0, 1.0, &|_| Shape::Linear),
            trim: (id % 4 == 0)
                .then(|| lane(id + 2, 9, -6.0, 6.0, &|_| Shape::Linear))
                .flatten(),
            mute: (id % 5 == 0)
                .then(|| {
                    AutoLane::new(
                        (0..16)
                            .map(|k| AutoPoint {
                                at: k * SECONDS * 48_000 / 15,
                                value: (k % 2) as f32 * f32::from(u8::from(id % 2 == 0)),
                                shape: Shape::Step,
                            })
                            .collect(),
                    )
                })
                .flatten(),
            sends: (0..node.sends.len() as u64)
                .map(|k| lane(id + 3 + k, 17, -30.0, 0.0, &|_| Shape::Linear))
                .collect(),
            plugins: Vec::new(),
        });
    }
    routing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_compiles() {
        for variant in 0..4 {
            crate::mixer::compile(&routed(64, variant)).unwrap();
            let plan = crate::mixer::compile(&latent(64, variant)).unwrap();
            assert!(plan.compensation().frames > 128);
            crate::mixer::compile(&automated(64, variant)).unwrap();
        }
    }
}
