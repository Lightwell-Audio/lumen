//! Lumen's icon as pixels (ADR-0043): the owner's Dawn in Sunrise on the
//! app icon's dark rounded square, drawn from the mark's geometry
//! (`widgets::mark`) with no image files or decoders. Used for the window
//! icon now; the installer's app and file icons follow in M9.

use eframe::egui;

use crate::widgets::mark::{GRID, HORIZON, SUN};

/// The window icon's size in pixels.
pub const WINDOW_ICON: u32 = 128;
/// The sun's rim, in both icons (the dark theme's `sun`).
const SUN_RIM: [f32; 3] = [255.0, 196.0, 107.0];
/// The water: the dark theme's accent.
const WATER: [f32; 3] = [79.0, 163.0, 255.0];
const WHITE: [f32; 3] = [255.0, 255.0, 255.0];
/// The rounded square's top and bottom colours.
const GROUND_TOP: [f32; 3] = [31.0, 34.0, 40.0];
const GROUND_BOTTOM: [f32; 3] = [14.0, 15.0, 18.0];
/// The rounded square's corner radius, as a share of its size.
const CORNER: f32 = 0.225;
/// The mark's size, as a share of the icon's.
const MARK: f32 = 0.72;
/// Samples per pixel along each axis.
const SAMPLES: u32 = 4;

/// The window icon.
pub fn window_icon() -> egui::IconData {
    egui::IconData {
        rgba: icon_rgba(WINDOW_ICON),
        width: WINDOW_ICON,
        height: WINDOW_ICON,
    }
}

/// The icon, `size` pixels square, as straight RGBA.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let n = (SAMPLES * SAMPLES) as f32;
    for y in 0..size {
        for x in 0..size {
            // Premultiplied sums over the pixel's samples.
            let mut sum = [0.0f32; 4];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let u = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let v = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    if let Some(c) = sample(u, v, size as f32) {
                        for i in 0..3 {
                            sum[i] += c[i];
                        }
                        sum[3] += 1.0;
                    }
                }
            }
            let alpha = sum[3] / n;
            for &channel in &sum[..3] {
                let straight = if sum[3] > 0.0 { channel / sum[3] } else { 0.0 };
                out.push(straight.round().clamp(0.0, 255.0) as u8);
            }
            out.push((alpha * 255.0).round() as u8);
        }
    }
    out
}

/// The colour at `(u, v)` in an icon `size` pixels square, or `None`
/// outside its rounded square.
fn sample(u: f32, v: f32, size: f32) -> Option<[f32; 3]> {
    if !in_rounded_square(u, v, size) {
        return None;
    }
    let mut c = mix(GROUND_TOP, GROUND_BOTTOM, v / size);
    let m = MARK * size;
    let offset = (size - m) / 2.0;
    let gx = (u - offset) / m * GRID;
    let gy = (v - offset) / m * GRID;
    let centre = GRID / 2.0;
    if gy < HORIZON {
        let d = ((gx - centre).powi(2) + (gy - HORIZON).powi(2)).sqrt();
        if d <= SUN {
            // The disc, white at the centre, amber at the rim.
            let t = ((1.0 - d / SUN) / 0.6).clamp(0.0, 1.0);
            c = mix(SUN_RIM, WHITE, t);
        } else {
            // Its light, falling off over the sky.
            let glow = 0.5 * (-(d - SUN).powi(2) / (2.0 * 4.0f32.powi(2))).exp();
            c = mix(c, SUN_RIM, glow);
        }
    }
    // The horizon: white at the middle, the water's colour at its ends.
    if capsule(gx, gy, (5.0, GRID - 5.0), HORIZON, 1.3) {
        let t = 1.0 - (gx - centre).abs() / (centre - 5.0);
        let colour = mix(WATER, WHITE, t.clamp(0.0, 1.0));
        c = mix(c, colour, 0.14 + 0.86 * t.clamp(0.0, 1.0));
    }
    // The water: three reflections.
    for (from, to, y, a) in [
        (21.0, 43.0, 38.0, 0.85),
        (25.0, 39.0, 43.5, 0.6),
        (28.5, 35.5, 49.0, 0.3),
    ] {
        if capsule(gx, gy, (from, to), y, 1.3) {
            c = mix(c, WATER, a);
        }
    }
    Some(c)
}

fn in_rounded_square(u: f32, v: f32, size: f32) -> bool {
    let r = CORNER * size;
    let dx = (r - u).max(u - (size - r)).max(0.0);
    let dy = (r - v).max(v - (size - r)).max(0.0);
    (0.0..size).contains(&u) && (0.0..size).contains(&v) && dx * dx + dy * dy <= r * r
}

/// Inside a horizontal line from `xs.0` to `xs.1` at `y` with round ends.
fn capsule(x: f32, y: f32, xs: (f32, f32), at: f32, half_width: f32) -> bool {
    let cx = x.clamp(xs.0, xs.1);
    (x - cx).powi(2) + (y - at).powi(2) <= half_width * half_width
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn the_icon_is_the_sun_over_water_on_a_rounded_square() {
        let size = 64;
        let rgba = icon_rgba(size);
        assert_eq!(rgba.len(), (size * size * 4) as usize);
        // Corners are clear; the middle of an edge is solid ground.
        assert_eq!(pixel(&rgba, size, 0, 0)[3], 0);
        assert_eq!(pixel(&rgba, size, size / 2, 1)[3], 255);
        // Just above the horizon at the centre: the sun's white core.
        let m = MARK * size as f32;
        let horizon = ((size as f32 - m) / 2.0 + HORIZON / GRID * m) as u32;
        let core = pixel(&rgba, size, size / 2, horizon - 3);
        assert!(core[0] > 240 && core[1] > 230, "{core:?}");
        // Near the sun's rim it is amber: red above blue.
        let rim = pixel(&rgba, size, size / 2, horizon - (SUN / GRID * m) as u32 + 1);
        assert!(rim[0] > rim[2] + 40, "{rim:?}");
        // The first reflection is the water's blue: blue above red.
        let y = ((size as f32 - m) / 2.0 + 38.0 / GRID * m) as u32;
        let water = pixel(&rgba, size, size / 2, y);
        assert!(water[2] > water[0] + 60, "{water:?}");
    }
}
