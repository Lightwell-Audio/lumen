mod ico;
mod raster;

pub use ico::ico;
pub use raster::{app_icon, project_icon};

pub const GRID: f32 = 64.0;
pub const HORIZON: f32 = 32.0;
pub const SUN: f32 = 16.0;
pub const COMPACT: f32 = 40.0;
pub const HALF_WIDTH: f32 = 1.3;
pub const COMPACT_HALF_WIDTH: f32 = 2.0;
pub const REFLECTIONS: [(f32, f32, f32, f32); 3] = [
    (21.0, 43.0, 38.0, 0.85),
    (25.0, 39.0, 43.5, 0.6),
    (28.5, 35.5, 49.0, 0.3),
];
pub const COMPACT_REFLECTIONS: [(f32, f32, f32, f32); 2] =
    [(19.0, 45.0, 40.0, 0.85), (25.5, 38.5, 48.0, 0.6)];
pub const RIM_AT: u32 = 32;
pub const ICO_SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 96, 256];

pub fn app_ico() -> Vec<u8> {
    ico_of(app_icon)
}

pub fn project_ico() -> Vec<u8> {
    ico_of(project_icon)
}

fn ico_of(draw: fn(u32) -> Vec<u8>) -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = ICO_SIZES.iter().map(|&s| (s, draw(s))).collect();
    let refs: Vec<(u32, &[u8])> = images.iter().map(|(s, p)| (*s, p.as_slice())).collect();
    ico(&refs)
}
