use std::time::Instant;

use lumen_core::midi::Event;

pub trait Instrument: Send {
    fn render(&mut self, rate: u32, events: &[Event], left: &mut [f32], right: &mut [f32]);

    fn render_by(
        &mut self,
        rate: u32,
        events: &[Event],
        left: &mut [f32],
        right: &mut [f32],
        _deadline: Option<Instant>,
    ) {
        self.render(rate, events, left, right);
    }

    fn active(&self) -> bool;

    fn preview(&mut self, _rate: u32, _left: &mut [f32], _right: &mut [f32]) {}

    fn configure(&mut self, _preset: u32) {}

    fn configure_values(&mut self, _values: &[(u32, f32)]) {}

    fn set_tempo(&mut self, _bpm: f32) {}

    fn silence(&mut self);

    fn hosted(&self) -> bool {
        false
    }

    fn automate(&mut self, _params: &crate::ParamBlock) {}

    fn begin(&mut self, _rate: u32, _frames: usize, _events: &[Event]) -> bool {
        false
    }

    fn output(&mut self, _pair: usize, _left: &mut [f32], _right: &mut [f32]) -> bool {
        false
    }

    fn finish(&mut self, _deadline: Instant, _left: &mut [f32], _right: &mut [f32]) {}
}
