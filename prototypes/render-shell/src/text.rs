//! Spike: clip-name labels laid out by Parley and drawn by Vello, to cost
//! text in a toolkit-free host. Layouts are cached per distinct label, as a
//! real arrangement would cache them.

use std::collections::HashMap;

use lumen_render::vello::kurbo::Affine;
use lumen_render::vello::peniko::{Color, Fill};
use lumen_render::vello::{self, Scene};
use lumen_render::{Area, RectInstance, View, demo};
use parley::{FontContext, Layout, LayoutContext, PositionedLayoutItem, StyleProperty};

pub struct Labels {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    cache: HashMap<usize, Layout<()>>,
    clips: Vec<RectInstance>,
    pub drawn: usize,
}

impl Labels {
    pub fn new() -> Self {
        Self {
            fonts: FontContext::new(),
            layouts: LayoutContext::new(),
            cache: HashMap::new(),
            clips: demo::clip_instances(),
            drawn: 0,
        }
    }

    fn layout(&mut self, index: usize) -> &Layout<()> {
        self.cache.entry(index).or_insert_with(|| {
            let text = format!("Clip {}", index + 1);
            let mut builder = self
                .layouts
                .ranged_builder(&mut self.fonts, &text, 1.0, true);
            builder.push_default(StyleProperty::FontSize(12.0));
            builder.push_default(StyleProperty::FontFamily("Segoe UI".into()));
            let mut layout = builder.build(&text);
            layout.break_all_lines(None);
            layout
        })
    }

    /// Adds a label to every visible clip wider than 40 px.
    pub fn draw(&mut self, scene: &mut Scene, area: Area, view: View) {
        self.drawn = 0;
        let first = (view.top_px / view.row_height).floor().max(0.0) as usize;
        let last =
            (((view.top_px + area.height) / view.row_height).ceil() as usize).min(demo::TRACKS);
        let white = Color::from_rgba8(0xE6, 0xE8, 0xEC, 255);
        for row in first..last {
            for k in 0..demo::CLIPS_PER_TRACK {
                let clip = self.clips[row * demo::CLIPS_PER_TRACK + k];
                let x = |b: [f32; 2]| {
                    let beat = f64::from(b[0]) + f64::from(b[1]);
                    area.x + ((beat - view.left) * f64::from(view.px_per_unit)) as f32
                };
                let (x0, x1) = (x(clip.start), x(clip.end));
                if x1 < area.x || x0 > area.x + area.width || x1 - x0 < 40.0 {
                    continue;
                }
                let y = area.y + row as f32 * view.row_height - view.top_px + 3.0;
                let origin = (f64::from(x0.max(area.x) + 5.0), f64::from(y));
                let layout = self.layout(k);
                draw_layout(scene, layout, origin, white);
                self.drawn += 1;
            }
        }
    }
}

fn draw_layout(scene: &mut Scene, layout: &Layout<()>, origin: (f64, f64), color: Color) {
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let mut x = glyph_run.offset();
            let y = glyph_run.baseline();
            let glyphs = glyph_run.glyphs().map(|g| {
                let glyph = vello::Glyph {
                    id: g.id,
                    x: x + g.x,
                    y: y - g.y,
                };
                x += g.advance;
                glyph
            });
            scene
                .draw_glyphs(run.font())
                .font_size(run.font_size())
                .transform(Affine::translate(origin))
                .brush(color)
                .draw(Fill::NonZero, glyphs);
        }
    }
}
