//! Compiling a [`Routing`] into a [`Plan`]: checking it, ordering the
//! nodes so that every node comes after everything that feeds it, working
//! out delay compensation (M4 slice 6), and allocating every buffer and
//! delay line the plan needs. Runs on the UI side.
//!
//! **Delay compensation.** Each node's input arrives some frames after the
//! tracks played it: the latest of everything feeding it (an output, a
//! send or a sidechain), each of which left its source after the source's
//! devices (or, for a pre-FX sidechain, before them). Paths that arrive
//! early go through delay lines as long as the difference, and so does
//! what the tracks play into a node that waits for a later input. All device
//! outputs and the cue bus are lined up to the latest of them, the
//! routing's [`Compensation`].

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::delay::{LineKey, Lines};
use super::meter::MeterBank;
use super::plan::{BUS, Dev, NO_METER, NO_SIDE, Node, Out, Plan, Send, TapOut};
use super::smooth::Smoothed;
use super::spec::{Compensation, Destination, Routing, RoutingError, Tap};

/// Checks `routing` and builds its plan, unmetered. Allocates; call it off
/// the audio thread.
pub fn compile(routing: &Routing) -> Result<Plan, RoutingError> {
    compile_metered(routing, None, |_| NO_METER)
}

/// What feeds a node: an output or send (after the source's devices) or a
/// sidechain taken at `Tap`.
#[derive(Clone, Copy)]
enum Edge {
    Main,
    Sidechain(Tap),
}

/// [`compile`], with every node's level going to `meters` at the slot
/// `slot` gives its id.
pub(super) fn compile_metered(
    routing: &Routing,
    meters: Option<Arc<MeterBank>>,
    slot: impl Fn(u64) -> u32,
) -> Result<Plan, RoutingError> {
    let specs = &routing.nodes;
    let mut by_id: Vec<(u64, u32)> = specs
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i as u32))
        .collect();
    by_id.sort_unstable();
    if let Some(w) = by_id.windows(2).find(|w| w[0].0 == w[1].0) {
        return Err(RoutingError::Duplicate(w[0].0));
    }
    let index_of = |from: u64, to: u64| -> Result<usize, RoutingError> {
        by_id
            .binary_search_by_key(&to, |&(id, _)| id)
            .map(|i| by_id[i].1 as usize)
            .map_err(|_| RoutingError::Unknown { from, to })
    };

    // Edges from each node to the nodes it feeds.
    let mut feeds: Vec<Vec<(usize, Edge)>> = vec![Vec::new(); specs.len()];
    let mut incoming = vec![0usize; specs.len()];
    for (i, spec) in specs.iter().enumerate() {
        let outputs = match spec.output {
            Destination::Node(to) => Some(to),
            _ => None,
        };
        for to in outputs.into_iter().chain(spec.sends.iter().map(|s| s.to)) {
            let j = index_of(spec.id, to)?;
            if j == i {
                return Err(RoutingError::Cycle);
            }
            feeds[i].push((j, Edge::Main));
            incoming[j] += 1;
        }
        for sidechain in &spec.sidechains {
            let from = index_of(spec.id, sidechain.from)?;
            if from == i {
                return Err(RoutingError::Cycle);
            }
            feeds[from].push((i, Edge::Sidechain(sidechain.tap)));
            incoming[i] += 1;
        }
    }

    // Kahn's order; among ready nodes, the earliest listed goes first, so
    // the same routing always compiles to the same plan.
    let mut ready: BTreeSet<usize> = (0..specs.len()).filter(|&i| incoming[i] == 0).collect();
    let mut order = Vec::with_capacity(specs.len());
    while let Some(i) = ready.pop_first() {
        order.push(i);
        for &(j, _) in &feeds[i] {
            incoming[j] -= 1;
            if incoming[j] == 0 {
                ready.insert(j);
            }
        }
    }
    if order.len() < specs.len() {
        return Err(RoutingError::Cycle);
    }

    let arrival = Arrival::work_out(routing, &order, &feeds);

    let mut position = vec![0u32; specs.len()];
    for (at, &i) in order.iter().enumerate() {
        position[i] = at as u32;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut nodes = Vec::with_capacity(specs.len());
    let mut sends = Vec::new();
    let mut devices = Vec::new();
    let mut taps = Vec::new();
    let mut lines = Lines::default();
    let mut sides = 0u32;
    let mut side_of = vec![NO_SIDE; specs.len()];
    for &i in &order {
        if !specs[i].sidechains.is_empty() {
            side_of[i] = sides;
            sides += 1;
        }
    }
    for &i in &order {
        let spec = &specs[i];
        let after = arrival.after_devices(i);
        let out = match spec.output {
            Destination::Node(to) => Out::Node(position[index_of(spec.id, to)?]),
            Destination::Hardware(pair) => Out::Hardware(pair),
            Destination::None => Out::None,
        };
        let out_delay = match spec.output {
            Destination::Node(to) => arrival.at[index_of(spec.id, to)?] - after,
            Destination::Hardware(_) => arrival.total - after,
            Destination::None => 0,
        };
        (spec.id, spec.output).hash(&mut hasher);
        let out_line = lines.add(LineKey::Output { from: spec.id }, out_delay);
        // What the tracks play into the node waits for its other inputs.
        let input_line = lines.add(LineKey::Input { node: spec.id }, arrival.at[i]);

        let first_device = devices.len() as u32;
        for (index, device) in spec.devices.iter().enumerate() {
            device.hash(&mut hasher);
            let super::spec::Device::Delay { listen, .. } = *device;
            devices.push(Dev {
                line: lines.add(
                    LineKey::Device {
                        node: spec.id,
                        index: index as u32,
                    },
                    device.latency(),
                ),
                listen,
            });
        }

        let first_tap = taps.len() as u32;
        for &(j, edge) in &feeds[i] {
            let Edge::Sidechain(tap) = edge else {
                continue;
            };
            (specs[j].id, tap).hash(&mut hasher);
            let leaves = if tap == Tap::PreFx {
                arrival.at[i]
            } else {
                after
            };
            let key = LineKey::Sidechain {
                from: spec.id,
                to: specs[j].id,
                tap,
            };
            taps.push(TapOut {
                to: side_of[j],
                tap,
                line: lines.add(key, arrival.at[j] - leaves),
            });
        }

        let first = sends.len() as u32;
        for send in &spec.sends {
            (send.to, send.pre_fader).hash(&mut hasher);
            let to = index_of(spec.id, send.to)?;
            let key = LineKey::Send {
                from: spec.id,
                to: send.to,
                pre_fader: send.pre_fader,
            };
            sends.push(Send {
                to: position[to],
                to_id: send.to,
                pre_fader: send.pre_fader,
                gain: Smoothed::new(finite(send.gain)),
                line: lines.add(key, arrival.at[to] - after),
            });
        }

        let sign = if spec.invert { -1.0 } else { 1.0 };
        let heard = spec.audible || spec.sends_audible;
        // With devices or sidechains taken from it, mute comes after them;
        // otherwise it is folded into the input gain.
        let split = !spec.devices.is_empty() || first_tap < taps.len() as u32;
        let pre = if split || heard {
            finite(spec.trim) * sign
        } else {
            0.0
        };
        let cued = spec.cue && routing.cue.is_some();
        let cue = if cued { finite(spec.trim) * sign } else { 0.0 };
        let cue_line = if cued {
            lines.add(
                LineKey::Cue { from: spec.id },
                arrival.total - arrival.at[i],
            )
        } else {
            super::delay::NO_LINE
        };
        let volume = finite(spec.volume);
        let [left, right] = spec.law.gains(spec.pan);
        nodes.push(Node {
            id: spec.id,
            input_line,
            pre: Smoothed::new(pre),
            split,
            heard: Smoothed::new(if heard { 1.0 } else { 0.0 }),
            cue: Smoothed::new(cue),
            cue_line,
            gate: Smoothed::new(if spec.audible { 1.0 } else { 0.0 }),
            post: [Smoothed::new(volume * left), Smoothed::new(volume * right)],
            out,
            out_line,
            first,
            count: spec.sends.len() as u32,
            first_device,
            devices: spec.devices.len() as u32,
            first_tap,
            taps: taps.len() as u32 - first_tap,
            side: side_of[i],
            lined: false,
            meter: if meters.is_some() {
                slot(spec.id)
            } else {
                NO_METER
            },
        });
    }
    lines.finish();
    lines.shape().for_each(|line| line.hash(&mut hasher));
    for node in &mut nodes {
        node.lined = node.split
            || node.out_line != super::delay::NO_LINE
            || node.cue_line != super::delay::NO_LINE
            || sends[node.first as usize..(node.first + node.count) as usize]
                .iter()
                .any(|s| s.line != super::delay::NO_LINE);
    }
    let mut by_id: Vec<(u64, u32)> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i as u32))
        .collect();
    by_id.sort_unstable();
    let cue_channel = routing.cue.map(|c| c.channel);
    cue_channel.hash(&mut hasher);
    let measured = routing.measure.and_then(|id| {
        by_id
            .binary_search_by_key(&id, |&(id, _)| id)
            .ok()
            .map(|i| by_id[i].1)
    });
    let compensation = arrival.compensation(routing);
    Ok(Plan {
        buses: vec![0.0; nodes.len() * BUS],
        // Room to blend the tracks' input across a jump (`Mixer::locate`),
        // only needed while the routing is latent.
        input_tail: if compensation.frames > 0 {
            vec![0.0; nodes.len() * BUS]
        } else {
            Vec::new()
        },
        sides: vec![0.0; sides as usize * BUS],
        nodes,
        sends,
        devices,
        taps,
        lines,
        by_id,
        topology: hasher.finish(),
        cue_bus: if cue_channel.is_some() {
            vec![0.0; BUS]
        } else {
            Vec::new()
        },
        cue_channel,
        cue_volume: Smoothed::new(routing.cue.map_or(0.0, |c| finite(c.volume))),
        meters,
        measured,
        compensation,
    })
}

/// When each node's input arrives, in frames after the tracks played it.
struct Arrival {
    /// Per node, by its index in the routing.
    at: Vec<u32>,
    /// Frames each node's devices add.
    latency: Vec<u32>,
    /// What each node waits for: the node whose path arrives last, if any.
    latest: Vec<Option<usize>>,
    /// When the device outputs and the cue bus play.
    total: u32,
    /// The node that sets `total`, if any.
    last: Option<usize>,
}

impl Arrival {
    fn work_out(routing: &Routing, order: &[usize], feeds: &[Vec<(usize, Edge)>]) -> Self {
        let specs = &routing.nodes;
        let latency: Vec<u32> = specs.iter().map(|s| s.latency()).collect();
        let mut at = vec![0u32; specs.len()];
        let mut latest = vec![None; specs.len()];
        for &i in order {
            let after = at[i].saturating_add(latency[i]);
            for &(j, edge) in &feeds[i] {
                let leaves = match edge {
                    Edge::Sidechain(Tap::PreFx) => at[i],
                    _ => after,
                };
                if leaves > at[j] {
                    at[j] = leaves;
                    latest[j] = Some(i);
                }
            }
        }
        let mut total = 0;
        let mut last = None;
        for &i in order {
            let spec = &specs[i];
            let plays = match spec.output {
                Destination::Hardware(_) => Some(at[i].saturating_add(latency[i])),
                _ => None,
            };
            let cued = (spec.cue && routing.cue.is_some()).then_some(at[i]);
            for arrives in plays.into_iter().chain(cued) {
                if arrives > total {
                    total = arrives;
                    last = Some(i);
                }
            }
        }
        Self {
            at,
            latency,
            latest,
            total,
            last,
        }
    }

    fn after_devices(&self, i: usize) -> u32 {
        self.at[i].saturating_add(self.latency[i])
    }

    fn compensation(&self, routing: &Routing) -> Compensation {
        let mut path = Vec::new();
        let mut at = self.last;
        // Each step goes to a node earlier in the order, so this ends.
        while let Some(i) = at {
            if self.latency[i] > 0 {
                path.push((routing.nodes[i].id, self.latency[i]));
            }
            at = self.latest[i];
        }
        path.reverse();
        Compensation {
            frames: self.total,
            path,
        }
    }
}

/// A gain the audio thread can use: NaN and infinities become silence.
pub(super) fn finite(gain: f32) -> f32 {
    if gain.is_finite() { gain } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::super::spec::{Device, NodeSpec, SendSpec, SidechainSpec};
    use super::*;

    fn node(id: u64, output: Destination) -> NodeSpec {
        NodeSpec::unity(id, output)
    }

    fn routing(nodes: Vec<NodeSpec>) -> Routing {
        Routing {
            nodes,
            cue: None,
            measure: None,
        }
    }

    fn order(plan: &Plan) -> Vec<u64> {
        plan.nodes.iter().map(|n| n.id).collect()
    }

    fn latent(frames: u32) -> Vec<Device> {
        vec![Device::Delay {
            frames,
            listen: false,
        }]
    }

    #[test]
    fn nodes_come_after_everything_that_feeds_them() {
        // The master listed first, a group before its children, a return
        // fed by a send from a track listed after it.
        let mut track = node(4, Destination::Node(2));
        track.sends.push(SendSpec {
            to: 3,
            gain: 1.0,
            pre_fader: false,
        });
        let plan = compile(&routing(vec![
            node(1, Destination::Hardware(0)),
            node(2, Destination::Node(1)),
            node(3, Destination::Node(1)),
            track,
            node(5, Destination::Node(2)),
        ]))
        .unwrap();
        assert_eq!(order(&plan), [4, 3, 5, 2, 1]);
    }

    #[test]
    fn a_sidechain_source_comes_before_the_node_it_feeds() {
        // Track 2 listens to track 3, listed after it.
        let mut ducked = node(2, Destination::Node(1));
        ducked.sidechains.push(SidechainSpec {
            from: 3,
            tap: Tap::PostFx,
        });
        let plan = compile(&routing(vec![
            node(1, Destination::Hardware(0)),
            ducked,
            node(3, Destination::Node(1)),
        ]))
        .unwrap();
        assert_eq!(order(&plan), [3, 2, 1]);
    }

    #[test]
    fn the_same_routing_compiles_the_same_way() {
        let nodes = (0..20)
            .map(|i| node(i, Destination::Node(100)))
            .chain([node(100, Destination::Hardware(0))])
            .collect();
        let routing = routing(nodes);
        let a = compile(&routing).unwrap();
        let b = compile(&routing).unwrap();
        assert_eq!(order(&a), order(&b));
        assert_eq!(a.topology, b.topology);
        // Levels do not change the shape; outputs, devices and sidechains
        // do.
        let mut louder = routing.clone();
        louder.nodes[3].volume = 0.5;
        assert_eq!(compile(&louder).unwrap().topology, a.topology);
        let mut elsewhere = routing.clone();
        elsewhere.nodes[3].output = Destination::Hardware(1);
        assert_ne!(compile(&elsewhere).unwrap().topology, a.topology);
        let mut slower = routing.clone();
        slower.nodes[3].devices = latent(10);
        assert_ne!(compile(&slower).unwrap().topology, a.topology);
        let mut ducked = routing.clone();
        ducked.nodes[3].sidechains.push(SidechainSpec {
            from: 4,
            tap: Tap::PreFx,
        });
        assert_ne!(compile(&ducked).unwrap().topology, a.topology);
    }

    #[test]
    fn loops_unknown_targets_and_duplicates_are_refused() {
        let cycle = routing(vec![
            node(1, Destination::Node(2)),
            node(2, Destination::Node(1)),
        ]);
        assert_eq!(compile(&cycle).unwrap_err(), RoutingError::Cycle);
        let mut feedback = node(1, Destination::Hardware(0));
        feedback.sends.push(SendSpec {
            to: 1,
            gain: 1.0,
            pre_fader: true,
        });
        assert_eq!(
            compile(&routing(vec![feedback])).unwrap_err(),
            RoutingError::Cycle
        );
        // Through a send: a return that feeds the track sending to it.
        let mut track = node(1, Destination::Hardware(0));
        track.sends.push(SendSpec {
            to: 2,
            gain: 1.0,
            pre_fader: false,
        });
        assert_eq!(
            compile(&routing(vec![track, node(2, Destination::Node(1))])).unwrap_err(),
            RoutingError::Cycle
        );
        let unknown = routing(vec![node(1, Destination::Node(9))]);
        assert_eq!(
            compile(&unknown).unwrap_err(),
            RoutingError::Unknown { from: 1, to: 9 }
        );
        let twice = routing(vec![
            node(1, Destination::Hardware(0)),
            node(1, Destination::Hardware(0)),
        ]);
        assert_eq!(compile(&twice).unwrap_err(), RoutingError::Duplicate(1));
    }

    #[test]
    fn sidechains_that_loop_are_refused() {
        // A group's sidechain from one of its own tracks is fine...
        let mut group = node(2, Destination::Node(1));
        group.sidechains.push(SidechainSpec {
            from: 3,
            tap: Tap::PostFx,
        });
        let fine = routing(vec![
            node(1, Destination::Hardware(0)),
            group,
            node(3, Destination::Node(2)),
        ]);
        compile(&fine).unwrap();
        // ...but a track's sidechain from its own group loops.
        let mut track = node(3, Destination::Node(2));
        track.sidechains.push(SidechainSpec {
            from: 2,
            tap: Tap::PostMixer,
        });
        let looped = routing(vec![
            node(1, Destination::Hardware(0)),
            node(2, Destination::Node(1)),
            track,
        ]);
        assert_eq!(compile(&looped).unwrap_err(), RoutingError::Cycle);
        let mut own = node(1, Destination::Hardware(0));
        own.sidechains.push(SidechainSpec {
            from: 1,
            tap: Tap::PreFx,
        });
        assert_eq!(
            compile(&routing(vec![own])).unwrap_err(),
            RoutingError::Cycle
        );
        let mut missing = node(1, Destination::Hardware(0));
        missing.sidechains.push(SidechainSpec {
            from: 9,
            tap: Tap::PreFx,
        });
        assert_eq!(
            compile(&routing(vec![missing])).unwrap_err(),
            RoutingError::Unknown { from: 1, to: 9 }
        );
    }

    #[test]
    fn nothing_latent_needs_no_compensation_and_no_lines() {
        let plan = compile(&crate::testing::routed(64, 0)).unwrap();
        assert_eq!(plan.compensation, Compensation::default());
        assert_eq!(plan.lines.shape().count(), 0);
        assert!(plan.input_tail.is_empty());
    }

    #[test]
    fn compensation_follows_the_longest_path_to_the_outputs() {
        // Track 3 (10 frames) → group 2 (5 frames) → master 1 (7 frames);
        // track 4 straight to the master; a return 5 fed by track 4.
        let mut track = node(3, Destination::Node(2));
        track.devices = latent(10);
        let mut group = node(2, Destination::Node(1));
        group.devices = latent(5);
        let mut master = node(1, Destination::Hardware(0));
        master.devices = latent(7);
        let mut other = node(4, Destination::Node(1));
        other.sends.push(SendSpec {
            to: 5,
            gain: 1.0,
            pre_fader: false,
        });
        let plan = compile(&routing(vec![
            master,
            group,
            track,
            other,
            node(5, Destination::Node(1)),
        ]))
        .unwrap();
        assert_eq!(
            plan.compensation,
            Compensation {
                frames: 22,
                path: vec![(3, 10), (2, 5), (1, 7)],
            }
        );
        // Track 4's output and the return's each wait the 15 frames of the
        // track and group.
        let lengths: Vec<(LineKey, usize)> = plan.lines.shape().collect();
        assert!(lengths.contains(&(LineKey::Output { from: 4 }, 15)));
        assert!(lengths.contains(&(LineKey::Output { from: 5 }, 15)));
        // The send meets nothing later at the return, so it needs no line;
        // nor do the nodes on the longest path.
        assert!(
            !lengths
                .iter()
                .any(|(key, _)| matches!(key, LineKey::Send { .. }))
        );
        assert!(
            !lengths
                .iter()
                .any(|(key, _)| matches!(key, LineKey::Output { from: 1..=3 }))
        );
    }

    #[test]
    fn a_pre_fx_sidechain_leaves_before_the_devices() {
        // Track 2 (12 frames) feeds the master and, pre-FX, the sidechain
        // of track 3, which waits for it.
        let mut source = node(2, Destination::Node(1));
        source.devices = latent(12);
        let mut listener = node(3, Destination::Node(1));
        listener.sidechains.push(SidechainSpec {
            from: 2,
            tap: Tap::PreFx,
        });
        let plan = compile(&routing(vec![
            node(1, Destination::Hardware(0)),
            source,
            listener,
        ]))
        .unwrap();
        // The sidechain arrives at once, so track 3 waits for nothing; its
        // output then waits 12 frames for track 2's at the master.
        assert_eq!(plan.compensation.frames, 12);
        let lengths: Vec<(LineKey, usize)> = plan.lines.shape().collect();
        assert!(lengths.contains(&(LineKey::Output { from: 3 }, 12)));
        assert!(
            !lengths
                .iter()
                .any(|(key, _)| matches!(key, LineKey::Sidechain { .. }))
        );
    }

    #[test]
    fn bad_gains_become_silence() {
        let mut spec = node(1, Destination::Hardware(0));
        spec.volume = f32::NAN;
        spec.trim = f32::INFINITY;
        let plan = compile(&routing(vec![spec])).unwrap();
        assert_eq!(plan.nodes[0].pre.target(), 0.0);
        assert_eq!(plan.nodes[0].post[0].target(), 0.0);
    }
}
