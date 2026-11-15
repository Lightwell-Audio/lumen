use super::{BUS, CHUNK};

pub struct Buses<'a> {
    pub(super) by_id: &'a [(u64, u32)],
    pub(super) data: &'a mut [f32],
    pub(super) frames: usize,
}

impl<'a> Buses<'a> {
    pub(crate) fn over(by_id: &'a [(u64, u32)], data: &'a mut [f32], frames: usize) -> Self {
        Self {
            by_id,
            data,
            frames,
        }
    }
}

impl Buses<'_> {
    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn track(&mut self, id: u64) -> Option<Bus<'_>> {
        let index = self
            .by_id
            .binary_search_by_key(&id, |&(id, _)| id)
            .ok()
            .map(|i| self.by_id[i].1 as usize)?;
        Some(Bus {
            data: &mut self.data[index * BUS..(index + 1) * BUS],
            frames: self.frames,
        })
    }
}

pub struct Bus<'a> {
    data: &'a mut [f32],
    frames: usize,
}

impl Bus<'_> {
    pub fn channel(&mut self, side: usize) -> &mut [f32] {
        let start = side.min(1) * CHUNK;
        &mut self.data[start..start + self.frames]
    }
}
