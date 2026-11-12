//! Running a plan: the node loop, the cue bus and the small in-place mixes.

use super::super::auto::NodeStages;
use super::super::builtins::{self, NO_BUILTIN};
use super::super::delay::{Lines, NO_LINE};
use super::super::hosts::{self, Hosting, NO_HOST};
use super::super::meter::{LoudnessTap, keep};
use super::super::spec::Tap;
use crate::source::Output;

use super::{BUS, CHUNK, NO_DEV, NO_SIDE, Out, Pass, Plan, Ramp, TapOut};

impl Plan {
    /// Runs the nodes in order over the first `frames` of their buses,
    /// adding what reaches the device into `out`. With `advance` false (a
    /// tail rendered ahead of a jump), levels are read but do not move;
    /// delay lines do move, which only matters to a latent plan's stop
    /// (and playback clears them when it starts, unless an instrument still
    /// rings, ADR-0124). With `tap` (the plan the
    /// listener hears, not one fading out), every node's output is metered
    /// and the measured node's loudness taken.
    pub(in crate::mixer) fn process(
        &mut self,
        out: &mut Output<'_>,
        pass: Pass<'_>,
        tap: Option<&mut LoudnessTap>,
        hosting: &mut Hosting<'_>,
    ) {
        let Pass {
            start,
            rate,
            ramp,
            advance,
        } = pass;
        let frames = out.frames();
        let mut gain = [0.0f32; CHUNK];
        let gain = &mut gain[..frames];
        let mut tap = tap.filter(|_| advance);
        let meters = if tap.is_some() {
            self.meters.as_deref()
        } else {
            None
        };
        let keep = keep(frames, rate);
        let silent = [0.0f32; CHUNK];
        let measured = self.measured.map_or(usize::MAX, |m| m as usize);
        // Before any node adds to another's bus, the buses hold only what
        // the tracks played.
        for (node, bus) in self.nodes.iter().zip(self.buses.as_chunks_mut::<BUS>().0) {
            if node.input_line != NO_LINE {
                let (left, right) = bus.split_at_mut(CHUNK);
                self.lines
                    .run(node.input_line, &mut left[..frames], &mut right[..frames]);
            }
        }
        // Hosts work while the rest of the mixer does: a node that nothing
        // feeds hands its first hosted device its block now, scaled as the
        // node loop would, and the loop collects it where it reaches it.
        if self.hosted && advance && hosting.pool.is_some() {
            for index in 0..self.nodes.len() {
                let early = self.nodes[index].early;
                if early == NO_DEV {
                    continue;
                }
                self.nodes[index].pre.fill(gain, rate, false);
                let split = self.nodes[index].split;
                if let Some(auto) = self.nodes[index].auto.as_deref_mut() {
                    auto.input(start, rate, false, split, gain);
                }
                let bus = &self.buses[index * BUS..(index + 1) * BUS];
                let (input_left, input_right) = self.host_scratch.input.split_at_mut(CHUNK);
                for k in 0..frames {
                    input_left[k] = bus[k] * gain[k];
                    input_right[k] = bus[CHUNK + k] * gain[k];
                }
                let host = self.devices[early as usize].host;
                match self.nodes[index].auto.as_deref() {
                    Some(auto) => {
                        self.host_scratch.params.start(frames);
                        auto.device_block(host, start, frames, &mut self.host_scratch.params);
                    }
                    None => self.host_scratch.params.start(frames),
                }
                hosts::begin_early(
                    &mut self.devices[early as usize],
                    hosting,
                    &self.host_scratch,
                    frames,
                );
            }
        }
        for index in 0..self.nodes.len() {
            let node = &mut self.nodes[index];
            let law = node.law;
            // Downstream nodes always come later, so the buses split here.
            let (head, tail) = self.buses.split_at_mut((index + 1) * BUS);
            let bus = &mut head[index * BUS..];
            let (left, right) = bus.split_at_mut(CHUNK);
            let (left, right) = (&mut left[..frames], &mut right[..frames]);
            // Where node `to`'s bus starts in `tail`.
            let target = |to: u32| (to as usize - index - 1) * BUS;

            // Cued: heard on the cue bus before mute and the fader.
            let lined_cue = node.cue_line != NO_LINE;
            if (lined_cue || !(node.cue.settled() && node.cue.value() == 0.0))
                && let Some(cue) = self.cue_bus.as_chunks_mut::<BUS>().0.first_mut()
            {
                node.cue.fill(gain, rate, advance);
                if let Some(auto) = node.auto.as_deref_mut() {
                    auto.cue(start, rate, advance, gain);
                }
                let (cue_left, cue_right) = cue.split_at_mut(CHUNK);
                if lined_cue {
                    let (a, b) = self.scratch.split_at_mut(CHUNK);
                    let (a, b) = (&mut a[..frames], &mut b[..frames]);
                    a.copy_from_slice(left);
                    b.copy_from_slice(right);
                    scale(a, gain);
                    scale(b, gain);
                    self.lines.run(node.cue_line, a, b);
                    add(&mut cue_left[..frames], a);
                    add(&mut cue_right[..frames], b);
                } else {
                    add_scaled(&mut cue_left[..frames], left, gain);
                    add_scaled(&mut cue_right[..frames], right, gain);
                }
            }
            if !node.lined && node.pre.settled() && node.pre.value() == 0.0 {
                // Muted and settled: nothing leaves this node.
                if let Some(meters) = meters {
                    meters.silence(node.meter, keep);
                }
                if index == measured
                    && let Some(tap) = tap.as_deref_mut()
                {
                    tap.record(&silent[..frames], &silent[..frames], rate);
                }
                continue;
            }
            node.pre.fill(gain, rate, advance);
            if let Some(auto) = node.auto.as_deref_mut() {
                auto.input(start, rate, advance, node.split, gain);
            }
            scale(left, gain);
            scale(right, gain);

            let taps = &self.taps[node.first_tap as usize..(node.first_tap + node.taps) as usize];
            if node.side != NO_SIDE
                && let Some(meters) = meters
            {
                // What arrives to key the node's devices: every source is
                // earlier in the order, so the bus is whole by now.
                let side = &self.sides[node.side as usize * BUS..][..BUS];
                meters.record(
                    node.side_meter,
                    [&side[..frames], &side[CHUNK..CHUNK + frames]],
                    keep,
                );
            }
            if node.split {
                let (lines, sides) = (&mut self.lines, &mut self.sides);
                send_taps(
                    taps,
                    Tap::PreFx,
                    lines,
                    sides,
                    left,
                    right,
                    &mut self.scratch,
                );
                let devices =
                    node.first_device as usize..(node.first_device + node.devices) as usize;
                for device in &mut self.devices[devices] {
                    if device.host != NO_HOST {
                        // A plugin that takes a sidechain hears what arrives
                        // for the node.
                        let key = (node.side != NO_SIDE).then(|| {
                            let side = &self.sides[node.side as usize * BUS..][..BUS];
                            (&side[..frames], &side[CHUNK..CHUNK + frames])
                        });
                        // The parameters lanes change in this block, for
                        // a device not already handed it.
                        self.host_scratch.params.start(frames);
                        if !device.began
                            && let Some(auto) = node.auto.as_deref()
                        {
                            auto.device_block(
                                device.host,
                                start,
                                frames,
                                &mut self.host_scratch.params,
                            );
                        }
                        hosts::run(
                            device,
                            &mut self.lines,
                            &mut self.host_scratch,
                            hosting,
                            (&mut *left, &mut *right),
                            key,
                            (rate, advance),
                        );
                        continue;
                    }
                    if device.builtin != NO_BUILTIN {
                        // A built-in runs where it is, with the values the
                        // project holds and the lanes' points.
                        let key = (node.side != NO_SIDE).then(|| {
                            let side = &self.sides[node.side as usize * BUS..][..BUS];
                            (&side[..frames], &side[CHUNK..CHUNK + frames])
                        });
                        self.host_scratch.params.start(frames);
                        for &(id, value) in device.values.iter() {
                            if self.host_scratch.params.open(id) {
                                self.host_scratch.params.point(0, f64::from(value));
                            }
                        }
                        if let Some(auto) = node.auto.as_deref() {
                            auto.device_block(
                                device.builtin,
                                start,
                                frames,
                                &mut self.host_scratch.params,
                            );
                        }
                        builtins::run(
                            device,
                            &mut self.lines,
                            &mut self.host_scratch,
                            hosting,
                            (&mut *left, &mut *right),
                            key,
                            (rate, start, advance),
                            law,
                        );
                        continue;
                    }
                    if device.listen && node.side != NO_SIDE {
                        let side = &self.sides[node.side as usize * BUS..][..BUS];
                        add(left, &side[..frames]);
                        add(right, &side[CHUNK..CHUNK + frames]);
                    }
                    self.lines.run(device.line, left, right);
                }
                let (lines, sides) = (&mut self.lines, &mut self.sides);
                send_taps(
                    taps,
                    Tap::PostFx,
                    lines,
                    sides,
                    left,
                    right,
                    &mut self.scratch,
                );
                // Mute, after the devices.
                let lane = node.auto.as_deref().is_some_and(NodeStages::has_mute);
                if lane || !(node.heard.settled() && node.heard.value() == 1.0) {
                    node.heard.fill(gain, rate, advance);
                    if let Some(auto) = node.auto.as_deref_mut() {
                        auto.heard(start, rate, advance, gain);
                    }
                    scale(left, gain);
                    scale(right, gain);
                }
            }

            let sends = node.first as usize..(node.first + node.count) as usize;
            for (k, send) in self.sends[sends.clone()]
                .iter_mut()
                .enumerate()
                .filter(|(_, s)| s.pre_fader)
            {
                let at = target(send.to);
                send.gain.fill(gain, rate, advance);
                if let Some(auto) = node.auto.as_deref_mut() {
                    auto.send(k, start, rate, advance, gain);
                }
                let (l, r) = self
                    .lines
                    .through(send.line, left, right, &mut self.scratch);
                add_scaled(&mut tail[at..at + frames], l, gain);
                add_scaled(&mut tail[at + CHUNK..at + CHUNK + frames], r, gain);
            }

            node.post[0].fill(gain, rate, advance);
            scale(left, gain);
            node.post[1].fill(gain, rate, advance);
            scale(right, gain);
            if let Some(auto) = node.auto.as_deref_mut() {
                auto.mix(start, rate, advance, left, right);
            }

            for (k, send) in self.sends[sends]
                .iter_mut()
                .enumerate()
                .filter(|(_, s)| !s.pre_fader)
            {
                let at = target(send.to);
                send.gain.fill(gain, rate, advance);
                if let Some(auto) = node.auto.as_deref_mut() {
                    auto.send(k, start, rate, advance, gain);
                }
                let (l, r) = self
                    .lines
                    .through(send.line, left, right, &mut self.scratch);
                add_scaled(&mut tail[at..at + frames], l, gain);
                add_scaled(&mut tail[at + CHUNK..at + CHUNK + frames], r, gain);
            }

            if !(node.gate.settled() && node.gate.value() == 1.0) {
                if node.gate.settled() && !node.lined {
                    // Only the sends are heard.
                    if let Some(meters) = meters {
                        meters.silence(node.meter, keep);
                    }
                    if index == measured
                        && let Some(tap) = tap.as_deref_mut()
                    {
                        tap.record(&silent[..frames], &silent[..frames], rate);
                    }
                    continue;
                }
                node.gate.fill(gain, rate, advance);
                scale(left, gain);
                scale(right, gain);
            }
            if node.split {
                let (lines, sides) = (&mut self.lines, &mut self.sides);
                send_taps(
                    taps,
                    Tap::PostMixer,
                    lines,
                    sides,
                    left,
                    right,
                    &mut self.scratch,
                );
            }

            // What the node plays on: after fader, pan and the gate.
            if let Some(meters) = meters {
                meters.record(node.meter, [left, right], keep);
            }
            if index == measured
                && let Some(tap) = tap.as_deref_mut()
            {
                tap.record(left, right, rate);
            }

            match node.out {
                Out::Node(to) => {
                    let at = target(to);
                    let (l, r) = self
                        .lines
                        .through(node.out_line, left, right, &mut self.scratch);
                    add(&mut tail[at..at + frames], l);
                    add(&mut tail[at + CHUNK..at + CHUNK + frames], r);
                }
                Out::Hardware(pair) => {
                    let (l, r) = self
                        .lines
                        .through(node.out_line, left, right, &mut self.scratch);
                    let first = usize::from(pair) * 2;
                    for (side, samples) in [(0, l), (1, r)] {
                        if first + side >= out.channels() {
                            break;
                        }
                        let device = out.channel(first + side);
                        match ramp {
                            Ramp::Full => add(device, samples),
                            Ramp::Fade(fade) => add_scaled(device, samples, fade),
                        }
                    }
                }
                Out::None => {}
            }
        }
        self.cue_out(out, rate, ramp, advance);
    }

    /// Writes the cue bus to its pair of device channels at the cue level.
    fn cue_out(&mut self, out: &mut Output<'_>, rate: u32, ramp: Ramp<'_>, advance: bool) {
        let Some(first) = self.cue_channel.map(usize::from) else {
            return;
        };
        let Some(cue) = self.cue_bus.as_chunks_mut::<BUS>().0.first_mut() else {
            return;
        };
        let frames = out.frames();
        let mut gain = [0.0f32; CHUNK];
        let gain = &mut gain[..frames];
        self.cue_volume.fill(gain, rate, advance);
        let (left, right) = cue.split_at_mut(CHUNK);
        for (side, samples) in [(0, &mut left[..frames]), (1, &mut right[..frames])] {
            if first + side >= out.channels() {
                break;
            }
            scale(samples, gain);
            let device = out.channel(first + side);
            match ramp {
                Ramp::Full => add(device, samples),
                Ramp::Fade(fade) => add_scaled(device, samples, fade),
            }
        }
    }
}

/// Adds `left` and `right`, through their lines, to the sidechain buses
/// of the taps taken at `which`.
fn send_taps(
    taps: &[TapOut],
    which: Tap,
    lines: &mut Lines,
    sides: &mut [f32],
    left: &[f32],
    right: &[f32],
    scratch: &mut [f32; 2 * CHUNK],
) {
    for tap in taps.iter().filter(|t| t.tap == which) {
        let (l, r) = lines.through(tap.line, left, right, scratch);
        let side = &mut sides[tap.to as usize * BUS..][..BUS];
        add(&mut side[..l.len()], l);
        add(&mut side[CHUNK..CHUNK + r.len()], r);
    }
}

#[inline]
fn scale(samples: &mut [f32], gain: &[f32]) {
    for (s, g) in samples.iter_mut().zip(gain) {
        *s *= g;
    }
}

#[inline]
fn add(target: &mut [f32], samples: &[f32]) {
    for (t, s) in target.iter_mut().zip(samples) {
        *t += s;
    }
}

#[inline]
fn add_scaled(target: &mut [f32], samples: &[f32], gain: &[f32]) {
    for ((t, s), g) in target.iter_mut().zip(samples).zip(gain) {
        *t += s * g;
    }
}
