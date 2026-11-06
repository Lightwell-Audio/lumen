//! The pointer and the click mark, drawn into clips: the render has no
//! system cursor, so the recorder draws the one Lumen asks for that frame
//! (USER_DOCS.md §6.3).

use eframe::egui::{CursorIcon, Pos2};

/// The outline of the pointer for `icon`, 1 unit = 1 point, with the hot
/// spot at the origin. Shapes Lumen asks for that have no outline here
/// (the hands, the zoom glass) are drawn as the arrow.
fn outline(icon: CursorIcon) -> Vec<(f32, f32)> {
    match icon {
        CursorIcon::Crosshair => vec![
            (-1.2, -9.0),
            (1.2, -9.0),
            (1.2, -1.2),
            (9.0, -1.2),
            (9.0, 1.2),
            (1.2, 1.2),
            (1.2, 9.0),
            (-1.2, 9.0),
            (-1.2, 1.2),
            (-9.0, 1.2),
            (-9.0, -1.2),
            (-1.2, -1.2),
        ],
        CursorIcon::ResizeHorizontal
        | CursorIcon::ResizeColumn
        | CursorIcon::ResizeEast
        | CursorIcon::ResizeWest => RESIZE.to_vec(),
        CursorIcon::ResizeVertical
        | CursorIcon::ResizeRow
        | CursorIcon::ResizeNorth
        | CursorIcon::ResizeSouth => RESIZE.iter().map(|&(x, y)| (y, x)).collect(),
        _ => ARROW.to_vec(),
    }
}

/// The classic arrow, its tip at the origin.
const ARROW: [(f32, f32); 7] = [
    (0.0, 0.0),
    (0.0, 17.0),
    (4.2, 13.2),
    (7.0, 19.5),
    (9.6, 18.4),
    (6.9, 12.2),
    (12.2, 12.2),
];

/// A double arrow across, centred on the origin.
const RESIZE: [(f32, f32); 10] = [
    (-10.0, 0.0),
    (-5.0, -5.0),
    (-5.0, -1.7),
    (5.0, -1.7),
    (5.0, -5.0),
    (10.0, 0.0),
    (5.0, 5.0),
    (5.0, 1.7),
    (-5.0, 1.7),
    (-5.0, 5.0),
];

/// The pointer for `icon` at `at` (points). `press` from 0 to 1 shrinks it
/// slightly while the button is down.
pub fn draw(image: &mut image::RgbaImage, icon: CursorIcon, at: Pos2, scale: f32, press: f32) {
    let dip = 0.08 * (1.0 - (press * 2.0 - 1.0).abs()).min(1.0) * f32::from(u8::from(press > 0.0));
    let size = scale * (1.0 - dip);
    let points: Vec<(f32, f32)> = outline(icon)
        .iter()
        .map(|(x, y)| (at.x * scale + x * size, at.y * scale + y * size))
        .collect();
    let stroke = 1.6 * scale;
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in &points {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    let (w, h) = (image.width() as i32, image.height() as i32);
    for y in ((y0 - stroke) as i32).max(0)..((y1 + stroke) as i32 + 1).min(h) {
        for x in ((x0 - stroke) as i32).max(0)..((x1 + stroke) as i32 + 1).min(w) {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let inside = contains(&points, p);
            let edge = distance_to_outline(&points, p);
            let colour = if inside && edge > stroke * 0.5 {
                [255, 255, 255]
            } else if inside || edge <= stroke * 0.5 {
                [16, 18, 22]
            } else {
                continue;
            };
            let px = image.get_pixel_mut(x as u32, y as u32);
            px.0 = [colour[0], colour[1], colour[2], 255];
        }
    }
}

/// A click's ring around `at`, growing and fading as `t` goes from 0 to 1.
pub fn ring(image: &mut image::RgbaImage, at: Pos2, scale: f32, t: f32) {
    let radius = (6.0 + 16.0 * t) * scale;
    let width = 2.0 * scale;
    let alpha = (1.0 - t) * 0.85;
    let (cx, cy) = (at.x * scale, at.y * scale);
    let reach = (radius + width) as i32 + 1;
    for y in (cy as i32 - reach).max(0)..(cy as i32 + reach).min(image.height() as i32) {
        for x in (cx as i32 - reach).max(0)..(cx as i32 + reach).min(image.width() as i32) {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let cover = (1.0 - ((d - radius).abs() - width * 0.5).max(0.0)).clamp(0.0, 1.0) * alpha;
            if cover > 0.0 {
                let px = image.get_pixel_mut(x as u32, y as u32);
                // The accent, #4FA3FF.
                for (channel, accent) in px.0.iter_mut().zip([79.0, 163.0, 255.0]) {
                    *channel = (f32::from(*channel) * (1.0 - cover) + accent * cover) as u8;
                }
            }
        }
    }
}

fn contains(points: &[(f32, f32)], p: (f32, f32)) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn distance_to_outline(points: &[(f32, f32)], p: (f32, f32)) -> f32 {
    let mut best = f32::MAX;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        let (x, y) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
        best = best.min((x * x + y * y).sqrt());
    }
    best
}
