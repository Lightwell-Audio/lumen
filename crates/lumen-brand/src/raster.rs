//! The icons drawn into straight RGBA pixels, sampled 4 × 4 per pixel.

use crate::{
    COMPACT, COMPACT_HALF_WIDTH, COMPACT_REFLECTIONS, GRID, HALF_WIDTH, HORIZON, REFLECTIONS,
    RIM_AT, SUN,
};

type Rgb = [f32; 3];

/// The sun's rim (the dark theme's `sun`).
const SUN_RIM: Rgb = [255.0, 196.0, 107.0];
/// The water: the dark theme's accent.
const WATER: Rgb = [79.0, 163.0, 255.0];
const WHITE: Rgb = [255.0, 255.0, 255.0];
/// The rounded square's and the page's top and bottom colours.
const GROUND_TOP: Rgb = [31.0, 34.0, 40.0];
const GROUND_BOTTOM: Rgb = [14.0, 15.0, 18.0];
const PAGE_TOP: Rgb = [35.0, 38.0, 45.0];
/// The page's outline at large sizes.
const PAGE_EDGE: Rgb = [58.0, 63.0, 72.0];
/// The light rim of small icons.
const RIM: Rgb = [112.0, 120.0, 134.0];
/// The rounded square's corner radius, as a share of its size.
const CORNER: f32 = 0.225;
/// The mark's size on the app icon, as a share of the icon's.
const MARK: f32 = 0.72;
/// Samples per pixel along each axis.
const SAMPLES: u32 = 4;

/// The app icon (App A), `size` pixels square, as straight RGBA.
pub fn app_icon(size: u32) -> Vec<u8> {
    let s = size as f32;
    let compact = MARK * s <= COMPACT;
    let rim = size <= RIM_AT;
    supersample(size, |u, v| {
        if !in_rounded_square(u, v, s) {
            return None;
        }
        let ground = mix(GROUND_TOP, GROUND_BOTTOM, v / s);
        let m = MARK * s;
        let offset = (s - m) / 2.0;
        let mut c = dawn(
            ground,
            (u - offset) / m * GRID,
            (v - offset) / m * GRID,
            compact,
        );
        if rim && rounded_square_distance(u, v, s) > -1.0 {
            c = RIM;
        }
        Some(c)
    })
}

/// The project file icon (File B), `size` pixels square, as straight RGBA:
/// a dark page with an amber fold holding the mark. Drawn on a 256 box.
pub fn project_icon(size: u32) -> Vec<u8> {
    let k = size as f32 / 256.0;
    // The mark on the page, larger when it is small.
    let compact = 168.0 * k <= COMPACT;
    let m = if compact { 200.0 } else { 168.0 };
    let (mx, my) = (128.0 - m / 2.0, 140.0 - m / 2.0);
    // Strokes are 5 units, or a pixel when that is wider.
    let stroke = 5.0f32.max(1.0 / k);
    let edge = if size <= RIM_AT { RIM } else { PAGE_EDGE };
    supersample(size, |u, v| {
        let (x, y) = (u / k, v / k);
        let d = page_distance(x, y);
        if d > stroke / 2.0 {
            return None;
        }
        if d > -stroke / 2.0 {
            return Some(edge);
        }
        if let Some(fold) = fold_distance(x, y) {
            if fold.abs() <= stroke / 2.0 {
                return Some(edge);
            }
            if fold < 0.0 {
                return Some(SUN_RIM);
            }
        }
        let ground = mix(
            PAGE_TOP,
            GROUND_BOTTOM,
            ((y - 12.0) / 234.0).clamp(0.0, 1.0),
        );
        Some(dawn(
            ground,
            (x - mx) / m * GRID,
            (y - my) / m * GRID,
            compact,
        ))
    })
}

/// The Dawn mark over `base` at `(gx, gy)` on its grid.
fn dawn(base: Rgb, gx: f32, gy: f32, compact: bool) -> Rgb {
    let mut c = base;
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
    let half = if compact {
        COMPACT_HALF_WIDTH
    } else {
        HALF_WIDTH
    };
    // The horizon: white at the middle, the water's colour at its ends.
    if capsule(gx, gy, (5.0, GRID - 5.0), HORIZON, half) {
        let t = (1.0 - (gx - centre).abs() / (centre - 5.0)).clamp(0.0, 1.0);
        let colour = mix(WATER, WHITE, t);
        c = mix(c, colour, 0.14 + 0.86 * t);
    }
    let lines: &[(f32, f32, f32, f32)] = if compact {
        &COMPACT_REFLECTIONS
    } else {
        &REFLECTIONS
    };
    for &(from, to, y, a) in lines {
        if capsule(gx, gy, (from, to), y, half) {
            c = mix(c, WATER, a);
        }
    }
    c
}

/// Samples `f` (pixel coordinates in, a colour or nothing out) 4 × 4 times
/// per pixel into straight RGBA.
fn supersample(size: u32, f: impl Fn(f32, f32) -> Option<Rgb>) -> Vec<u8> {
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
                    if let Some(c) = f(u, v) {
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

fn in_rounded_square(u: f32, v: f32, size: f32) -> bool {
    let r = CORNER * size;
    let dx = (r - u).max(u - (size - r)).max(0.0);
    let dy = (r - v).max(v - (size - r)).max(0.0);
    (0.0..size).contains(&u) && (0.0..size).contains(&v) && dx * dx + dy * dy <= r * r
}

/// The signed distance from the rounded square's edge, negative inside.
fn rounded_square_distance(u: f32, v: f32, size: f32) -> f32 {
    let half = size / 2.0;
    let r = CORNER * size;
    let qx = (u - half).abs() - (half - r);
    let qy = (v - half).abs() - (half - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// The page on the 256 box: x 44 to 212, y 12 to 246, its bottom corners
/// rounded by 10 and its top right corner cut from (164, 12) to (212, 60).
/// The signed distance from its outline, negative inside.
fn page_distance(x: f32, y: f32) -> f32 {
    let rect = if y > 236.0 && !(54.0..=202.0).contains(&x) {
        let cx = if x < 54.0 { 54.0 } else { 202.0 };
        ((x - cx).powi(2) + (y - 236.0).powi(2)).sqrt() - 10.0
    } else {
        (44.0 - x).max(x - 212.0).max(12.0 - y).max(y - 246.0)
    };
    let cut = ((x - 164.0) - (y - 12.0)) / std::f32::consts::SQRT_2;
    rect.max(cut)
}

/// The fold, the triangle (164, 12), (164, 60), (212, 60) with its lower
/// left corner rounded by 10: the signed distance from its two inner edges,
/// negative inside, or `None` far from it.
fn fold_distance(x: f32, y: f32) -> Option<f32> {
    if x < 158.0 || y > 66.0 {
        return None;
    }
    let edges = if x < 174.0 && y > 50.0 {
        ((x - 174.0).powi(2) + (y - 50.0).powi(2)).sqrt() - 10.0
    } else {
        (164.0 - x).max(y - 60.0)
    };
    Some(edges)
}

/// Inside a horizontal line from `xs.0` to `xs.1` at `at` with round ends.
fn capsule(x: f32, y: f32, xs: (f32, f32), at: f32, half_width: f32) -> bool {
    let cx = x.clamp(xs.0, xs.1);
    (x - cx).powi(2) + (y - at).powi(2) <= half_width * half_width
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
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
    fn the_app_icon_is_the_sun_over_water_on_a_rounded_square() {
        let size = 64;
        let rgba = app_icon(size);
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

    #[test]
    fn small_icons_get_a_light_rim_and_large_ones_do_not() {
        // The middle of the top edge, one pixel in.
        for (size, rimmed) in [(16, true), (32, true), (48, false), (256, false)] {
            let edge = pixel(&app_icon(size), size, size / 2, 0);
            let ground = (GROUND_TOP[0] as u8).abs_diff(edge[0]) < 8;
            assert_eq!(!ground, rimmed, "{size}: {edge:?}");
        }
    }

    #[test]
    fn the_compact_form_starts_where_the_mark_is_forty_pixels() {
        // At 48 px the mark is 34.6 px: compact, its first reflection sits
        // at grid row 40, below the full form's 38.
        let size = 48;
        let rgba = app_icon(size);
        let m = MARK * size as f32;
        let at = |row: f32| ((size as f32 - m) / 2.0 + row / GRID * m) as u32;
        let compact_row = pixel(&rgba, size, size / 2, at(40.5));
        assert!(compact_row[2] > compact_row[0] + 60, "{compact_row:?}");
        // At 64 px (46 px of mark) the full form: row 38 is water.
        let rgba = app_icon(64);
        let m = MARK * 64.0;
        let y = ((64.0 - m) / 2.0 + 38.0 / GRID * m) as u32;
        let full = pixel(&rgba, 64, 32, y);
        assert!(full[2] > full[0] + 60, "{full:?}");
    }

    #[test]
    fn the_project_icon_is_a_dark_page_with_an_amber_fold() {
        let size = 256;
        let rgba = project_icon(size);
        // Outside the page: clear, left of it and in the cut corner.
        assert_eq!(pixel(&rgba, size, 20, 128)[3], 0);
        assert_eq!(pixel(&rgba, size, 240, 20)[3], 0);
        // Inside the fold: amber, red well above blue.
        let fold = pixel(&rgba, size, 180, 44);
        assert!(fold[3] == 255 && fold[0] > 230 && fold[2] < 140, "{fold:?}");
        // The page below the water: dark.
        let page = pixel(&rgba, size, 128, 230);
        assert!(page[3] == 255 && page[0] < 40, "{page:?}");
        // The sun's core just above the horizon (y 140 on the box).
        let core = pixel(&rgba, size, 128, 136);
        assert!(core[0] > 240 && core[1] > 230, "{core:?}");
    }

    #[test]
    fn the_small_project_icon_keeps_its_outline() {
        // At 16 px the page's left edge is a full pixel of the light rim.
        let size = 16;
        let rgba = project_icon(size);
        let edge = pixel(&rgba, size, 2, 10);
        assert!(edge[3] > 100 && edge[0] > 60, "{edge:?}");
    }

    #[test]
    fn the_window_icon_size_is_unchanged_by_the_small_size_rules() {
        // 128 px: no rim, full form.
        let size = 128;
        let edge = pixel(&app_icon(size), size, size / 2, 0);
        assert!((GROUND_TOP[0] as u8).abs_diff(edge[0]) < 8, "{edge:?}");
    }
}
