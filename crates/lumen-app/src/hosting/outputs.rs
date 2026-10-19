//! A plugin's buses: which outputs are on and what they are called, and
//! whether it takes a sidechain (M6 slice 6, ADR-0078).

use super::*;

/// The audio input a sidechain feeds: the first bus after the plugin's own
/// audio (bus 0) that is mono or stereo. The bus's own "main" flag is not
/// read: plugins mark every bus main (a real orchestra does, for all sixteen
/// outputs), so the flag cannot tell a spare bus from the first.
pub(super) fn sidechain_bus(inputs: &[BusInfo]) -> Option<usize> {
    inputs
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, bus)| (1..=2).contains(&bus.channels))
        .map(|(index, _)| index)
}

/// The channels the auxiliary outputs may use in all: a block carries at most
/// `MAX_CHANNELS`, the main output's first (so a stereo main leaves seven
/// stereo outputs).
pub(super) fn output_budget(outputs: &[BusInfo]) -> usize {
    MAX_CHANNELS.saturating_sub(outputs.first().map_or(2, |main| usize::from(main.channels)))
}

/// Whether `bus` is an output besides the first that Lumen can play: mono or
/// stereo. The bus's "main" flag is not read (a real orchestra declares all
/// sixteen outputs as main buses, and none was ever turned on while it was).
pub(super) fn playable(outputs: &[BusInfo], bus: u32) -> bool {
    bus >= 1
        && outputs
            .get(bus as usize)
            .is_some_and(|b| (1..=2).contains(&b.channels))
}

/// The auxiliary outputs to turn on for an instrument: every one it has as a
/// mono or stereo output, lowest first while they fit a block (M6 slice 6d).
/// Which have a track is the project's; an output nobody plays is heard on
/// the instrument's own track. Turning on only the outputs that have a track
/// left the plugin to move the rest by its own rule (one plugin sent a sound
/// meant for an output that was off to the next output that was on).
pub(super) fn output_mask(outputs: &[BusInfo]) -> u32 {
    let wanted: Vec<u32> = (1..outputs.len().min(32) as u32)
        .filter(|&bus| playable(outputs, bus))
        .collect();
    let mut room = output_budget(outputs);
    let mut mask = 0;
    for bus in wanted {
        let width = usize::from(outputs[bus as usize].channels);
        if width <= room {
            room -= width;
            mask |= 1 << bus;
        }
    }
    mask
}

/// The auxiliary inputs to turn on for a plugin with these inputs: its
/// sidechain input when a sidechain is wanted.
pub(super) fn sidechain_mask(wanted: bool, inputs: &[BusInfo]) -> u32 {
    match sidechain_bus(inputs) {
        Some(bus) if wanted => 1 << bus,
        _ => 0,
    }
}

impl Hosting {
    /// The auxiliary outputs of the running instrument `device` that its
    /// link has on, one bit for each bus (M6 slice 6d).
    pub fn outputs_on(&self, device: Id) -> u32 {
        self.tracked.get(&device.0).map_or(0, |t| t.aux.outputs)
    }

    /// Whether the plugin's output `bus` can be turned on besides the outputs
    /// `on` already are: a block carries at most `MAX_CHANNELS` in all.
    pub fn output_fits(&self, device: Id, on: &[u32], bus: u32) -> bool {
        let Some(t) = self.tracked.get(&device.0) else {
            return false;
        };
        let width = |b: u32| {
            t.outputs
                .get(b as usize)
                .map_or(0, |i| usize::from(i.channels))
        };
        let used: usize = on.iter().filter(|&&b| b != bus).map(|&b| width(b)).sum();
        used + width(bus) <= output_budget(&t.outputs)
    }

    /// The names of every audio output the plugin declares, by bus number (the
    /// main one is 0); empty until it has loaded.
    pub fn output_names(&self, device: Id) -> Vec<String> {
        self.tracked.get(&device.0).map_or_else(Vec::new, |t| {
            t.outputs.iter().map(|b| b.name.clone()).collect()
        })
    }

    /// The outputs a running instrument can play on tracks of their own, with
    /// their bus numbers: every output past the main one that is mono or
    /// stereo.
    pub fn extra_outputs(&self, device: Id) -> Vec<(u32, String)> {
        self.tracked
            .get(&device.0)
            .filter(|t| t.state == State::Ready)
            .map_or_else(Vec::new, |t| {
                t.outputs
                    .iter()
                    .enumerate()
                    .skip(1)
                    .filter(|(_, bus)| (1..=2).contains(&bus.channels))
                    .map(|(index, bus)| (index as u32, bus.name.clone()))
                    .collect()
            })
    }

    /// Whether the running plugin has a sidechain input to key (M6 slice 6c).
    pub fn takes_sidechain(&self, device: Id) -> bool {
        self.tracked
            .get(&device.0)
            .is_some_and(|t| sidechain_bus(&t.inputs).is_some())
    }
}
