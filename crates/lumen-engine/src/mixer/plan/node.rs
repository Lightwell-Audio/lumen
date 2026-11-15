use super::super::auto::NodeStages;
use super::super::delay::LineId;
use super::super::smooth::Smoothed;
use super::super::spec::Tap;
use lumen_core::pan::PanLaw;

use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::mixer) enum Out {
    Node(u32),
    Hardware(u16),
    None,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::mixer) struct Send {
    pub(in crate::mixer) to: u32,
    pub(in crate::mixer) to_id: u64,
    pub(in crate::mixer) pre_fader: bool,
    pub(in crate::mixer) gain: Smoothed,
    pub(in crate::mixer) line: LineId,
}

#[derive(Debug, Clone)]
pub(in crate::mixer) struct Dev {
    pub(in crate::mixer) line: LineId,
    pub(in crate::mixer) listen: bool,
    pub(in crate::mixer) host: u64,
    pub(in crate::mixer) builtin: u64,
    pub(in crate::mixer) bypass: bool,
    pub(in crate::mixer) values: Arc<[(u32, f32)]>,
    pub(in crate::mixer) values_last: Vec<f32>,
    pub(in crate::mixer) wet: Smoothed,
    pub(in crate::mixer) began: bool,
    pub(in crate::mixer) refused: bool,
}

pub(in crate::mixer) const NO_DEV: u32 = u32::MAX;

#[derive(Debug, Clone, Copy)]
pub(in crate::mixer) struct TapOut {
    pub(in crate::mixer) to: u32,
    pub(in crate::mixer) tap: Tap,
    pub(in crate::mixer) line: LineId,
}

pub(in crate::mixer) const NO_SIDE: u32 = u32::MAX;

#[derive(Debug, Clone)]
pub(in crate::mixer) struct Node {
    pub(in crate::mixer) id: u64,
    pub(in crate::mixer) law: PanLaw,
    pub(in crate::mixer) input_line: LineId,
    pub(in crate::mixer) pre: Smoothed,
    pub(in crate::mixer) split: bool,
    pub(in crate::mixer) heard: Smoothed,
    pub(in crate::mixer) gate: Smoothed,
    pub(in crate::mixer) cue: Smoothed,
    pub(in crate::mixer) cue_line: LineId,
    pub(in crate::mixer) post: [Smoothed; 2],
    pub(in crate::mixer) out: Out,
    pub(in crate::mixer) out_line: LineId,
    pub(in crate::mixer) first: u32,
    pub(in crate::mixer) count: u32,
    pub(in crate::mixer) first_device: u32,
    pub(in crate::mixer) devices: u32,
    pub(in crate::mixer) early: u32,
    pub(in crate::mixer) first_tap: u32,
    pub(in crate::mixer) taps: u32,
    pub(in crate::mixer) side: u32,
    pub(in crate::mixer) lined: bool,
    pub(in crate::mixer) meter: u32,
    pub(in crate::mixer) side_meter: u32,
    pub(in crate::mixer) auto: Option<Box<NodeStages>>,
}

pub(in crate::mixer) const NO_METER: u32 = u32::MAX;

#[derive(Clone, Copy)]
pub(in crate::mixer) enum Ramp<'a> {
    Full,
    Fade(&'a [f32]),
}

#[derive(Clone, Copy)]
pub(in crate::mixer) struct Pass<'a> {
    pub(in crate::mixer) start: i64,
    pub(in crate::mixer) rate: u32,
    pub(in crate::mixer) bpm: f32,
    pub(in crate::mixer) ramp: Ramp<'a>,
    pub(in crate::mixer) advance: bool,
}
