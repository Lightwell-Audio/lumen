use super::*;

pub(super) fn sidechain_bus(inputs: &[BusInfo]) -> Option<usize> {
    inputs
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, bus)| (1..=2).contains(&bus.channels))
        .map(|(index, _)| index)
}

pub(super) fn output_budget(outputs: &[BusInfo]) -> usize {
    MAX_CHANNELS.saturating_sub(outputs.first().map_or(2, |main| usize::from(main.channels)))
}

pub(super) fn playable(outputs: &[BusInfo], bus: u32) -> bool {
    bus >= 1
        && outputs
            .get(bus as usize)
            .is_some_and(|b| (1..=2).contains(&b.channels))
}

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

pub(super) fn sidechain_mask(wanted: bool, inputs: &[BusInfo]) -> u32 {
    match sidechain_bus(inputs) {
        Some(bus) if wanted => 1 << bus,
        _ => 0,
    }
}

impl Hosting {
    pub fn outputs_on(&self, device: Id) -> u32 {
        self.tracked.get(&device.0).map_or(0, |t| t.aux.outputs)
    }

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

    pub fn output_names(&self, device: Id) -> Vec<String> {
        self.tracked.get(&device.0).map_or_else(Vec::new, |t| {
            t.outputs.iter().map(|b| b.name.clone()).collect()
        })
    }

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

    pub fn takes_sidechain(&self, device: Id) -> bool {
        self.tracked
            .get(&device.0)
            .is_some_and(|t| sidechain_bus(&t.inputs).is_some())
    }
}
