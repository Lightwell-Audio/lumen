//! Lumen's icons as pixels (ADR-0043, ADR-0117): the owner's Dawn mark, a
//! sun rising over water, on the app icon's dark rounded square (App A) and
//! on the project file icon's dark page with an amber fold (File B). Drawn
//! from the mark's geometry with no image files or decoders, so the window
//! icon, the executable's icons and the transport mark cannot drift apart.
//!
//! The app uses this crate for the window icon and the mark's geometry;
//! `lumen-app`'s build script uses it to write the ICO files it embeds on
//! Windows.

mod ico;
mod raster;

pub use ico::ico;
pub use raster::{app_icon, project_icon};

/// The grid the mark is drawn on.
pub const GRID: f32 = 64.0;
/// Where the horizon sits on the grid.
pub const HORIZON: f32 = 32.0;
/// The sun's radius on the grid.
pub const SUN: f32 = 16.0;
/// A mark this many pixels wide or less takes its compact form: thicker
/// strokes and two reflections instead of three.
pub const COMPACT: f32 = 40.0;
/// Half the width of the horizon and the reflections, on the grid.
pub const HALF_WIDTH: f32 = 1.3;
/// The same in the compact form.
pub const COMPACT_HALF_WIDTH: f32 = 2.0;
/// The reflections: from, to, height on the grid, and opacity.
pub const REFLECTIONS: [(f32, f32, f32, f32); 3] = [
    (21.0, 43.0, 38.0, 0.85),
    (25.0, 39.0, 43.5, 0.6),
    (28.5, 35.5, 49.0, 0.3),
];
/// The compact form's reflections.
pub const COMPACT_REFLECTIONS: [(f32, f32, f32, f32); 2] =
    [(19.0, 45.0, 40.0, 0.85), (25.5, 38.5, 48.0, 0.6)];
/// Icons this many pixels square or less get a thin light rim, so their
/// dark shape stays visible on a dark background (owner, 2026-11-03).
pub const RIM_AT: u32 = 32;
/// The sizes an ICO file carries: what the Windows shell asks for at 100%
/// to 200% scale, and the 256 px tile.
pub const ICO_SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 96, 256];

/// The app icon as an ICO file, every size in [`ICO_SIZES`].
pub fn app_ico() -> Vec<u8> {
    ico_of(app_icon)
}

/// The project file icon as an ICO file, every size in [`ICO_SIZES`].
pub fn project_ico() -> Vec<u8> {
    ico_of(project_icon)
}

fn ico_of(draw: fn(u32) -> Vec<u8>) -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = ICO_SIZES.iter().map(|&s| (s, draw(s))).collect();
    let refs: Vec<(u32, &[u8])> = images.iter().map(|(s, p)| (*s, p.as_slice())).collect();
    ico(&refs)
}
