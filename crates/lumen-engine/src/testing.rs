//! Routings for tests, benchmarks and the performance gate: the same
//! shapes everywhere, so a measured cost is the cost tested.

use lumen_core::pan::PanLaw;

use crate::mixer::{Destination, NodeSpec, Routing, SendSpec};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_compiles() {
        for variant in 0..4 {
            crate::mixer::compile(&routed(64, variant)).unwrap();
        }
    }
}
