//! Deterministic quality-bar scene shared by every renderer host, so hosts
//! are compared on identical content: 200 tracks × 100 clips, 100,000
//! notes, and a vector overlay (automation curves, knobs, meters).

use vello::kurbo::{Affine, Arc, BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};

use crate::{RectInstance, rgba};

pub const TRACKS: usize = 200;
pub const CLIPS_PER_TRACK: usize = 100;
pub const NOTES: usize = 100_000;
pub const NOTE_SPAN_BEATS: f32 = 4_000.0;
pub const ARRANGEMENT_BEATS: f32 = 404.0;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1_u64 << 24) as f32
    }
}

const PALETTE: [[u8; 3]; 8] = [
    [0xE0, 0x6C, 0x5F],
    [0xE8, 0xA0, 0x4C],
    [0xD6, 0xC8, 0x5A],
    [0x7F, 0xC0, 0x6A],
    [0x4F, 0xB8, 0xB0],
    [0x5B, 0x9B, 0xE6],
    [0x95, 0x7D, 0xE0],
    [0xD4, 0x75, 0xB8],
];

/// Arrangement clips; `row` is the track index.
pub fn clip_instances() -> Vec<RectInstance> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut out = Vec::with_capacity(TRACKS * CLIPS_PER_TRACK);
    for track in 0..TRACKS {
        let [r, g, b] = PALETTE[track % PALETTE.len()];
        for k in 0..CLIPS_PER_TRACK {
            let start = k as f32 * 4.0 + (rng.unit() * 4.0).floor() * 0.25;
            let len = 1.0 + (rng.unit() * 8.0).floor() * 0.25;
            let _ = rng.next();
            out.push(RectInstance::new(
                f64::from(start),
                f64::from(start + len),
                track as f32,
                rgba(r, g, b, 255),
            ));
        }
    }
    out
}

/// Piano-roll notes; `row` is `127 - pitch`.
pub fn note_instances() -> Vec<RectInstance> {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    (0..NOTES)
        .map(|_| {
            let start = (rng.unit() * NOTE_SPAN_BEATS * 4.0).floor() / 4.0;
            let len = 0.25 * (1.0 + (rng.unit() * 7.0).floor());
            let pitch = 24 + (rng.next() % 84) as u8;
            let velocity = 1 + (rng.next() % 127) as u8;
            let shade = 0.45 + f32::from(velocity) / 127.0 * 0.55;
            let c = |v: u8| (f32::from(v) * shade) as u8;
            RectInstance::new(
                f64::from(start),
                f64::from(start + len),
                f32::from(127 - pitch),
                rgba(c(0x5B), c(0x9B), c(0xE6), 255),
            )
        })
        .collect()
}

/// Builds the vector overlay for a `width × height` canvas at animation
/// time `t` (0..1): 8 automation lanes of 2,000 points, 64 knobs and 64
/// meters. Rebuilt every frame, as live meters and moving curves would be.
pub fn vector_overlay(scene: &mut vello::Scene, width: f64, height: f64, t: f64) {
    let accent = Color::from_rgba8(0xF2, 0xC1, 0x4E, 255);
    let lanes = 8;
    for lane in 0..lanes {
        let top = height * 0.05 + lane as f64 * height * 0.11;
        let lane_h = height * 0.08;
        let mut path = BezPath::new();
        for i in 0..2_000 {
            let x = width * f64::from(i) / 1_999.0;
            let phase = f64::from(i) * 0.01 + t * std::f64::consts::TAU + lane as f64;
            let y = top + lane_h * (0.5 + 0.45 * (phase.sin() * (phase * 0.37).cos()));
            if i == 0 {
                path.move_to(Point::new(x, y));
            } else {
                path.line_to(Point::new(x, y));
            }
        }
        scene.stroke(&Stroke::new(1.5), Affine::IDENTITY, accent, None, &path);
    }
    let track = Color::from_rgba8(0x3A, 0x3E, 0x48, 255);
    for i in 0..64 {
        let cx = 20.0 + (i % 32) as f64 * (width - 40.0) / 31.0;
        let cy = height - 70.0 + (i / 32) as f64 * 34.0;
        let center = Point::new(cx, cy);
        let value = 0.5 + 0.5 * ((t * 6.0 + i as f64 * 0.3).sin());
        let start = 0.75 * std::f64::consts::PI;
        let sweep = 1.5 * std::f64::consts::PI;
        let bg = Arc::new(center, (11.0, 11.0), start, sweep, 0.0);
        let fg = Arc::new(center, (11.0, 11.0), start, sweep * value, 0.0);
        scene.stroke(&Stroke::new(3.0), Affine::IDENTITY, track, None, &bg);
        scene.stroke(&Stroke::new(3.0), Affine::IDENTITY, accent, None, &fg);
        let meter_x = cx + 15.0;
        let level = 0.5 + 0.5 * ((t * 11.0 + i as f64).cos());
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            track,
            None,
            &Rect::new(meter_x, cy - 12.0, meter_x + 3.0, cy + 12.0),
        );
        let green = Color::from_rgba8(0x7F, 0xC0, 0x6A, 255);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            green,
            None,
            &Rect::new(meter_x, cy + 12.0 - 24.0 * level, meter_x + 3.0, cy + 12.0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_has_quality_bar_counts() {
        assert_eq!(clip_instances().len(), TRACKS * CLIPS_PER_TRACK);
        assert_eq!(note_instances().len(), NOTES);
        assert!(
            note_instances()
                .iter()
                .all(|n| n.end > n.start && (0.0..=127.0).contains(&n.row))
        );
    }

    #[test]
    fn overlay_builds_without_panicking() {
        let mut scene = vello::Scene::new();
        vector_overlay(&mut scene, 1920.0, 1080.0, 0.25);
    }
}
