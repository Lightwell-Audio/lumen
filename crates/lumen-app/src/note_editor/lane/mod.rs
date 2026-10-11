//! The velocity and controller lane under the piano roll (M5 slice 4b).
//!
//! - `geometry`: where values and points are on the screen;
//! - `gesture`: the pointer gestures as pure arithmetic.

#![allow(dead_code, reason = "the lane UI is wired in step by step (M5 slice 4b)")]

pub(crate) mod geometry;
pub(crate) mod gesture;
pub(crate) mod paint;
