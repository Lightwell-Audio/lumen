use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, Id as EguiId, LayerId, epaint::ClippedShape, vec2};

use crate::motion;

const LIFT: f32 = 4.0;

#[derive(Clone)]
struct Picture {
    layer: LayerId,
    shapes: Arc<Vec<ClippedShape>>,
    pass: u64,
    gone_at: Option<f64>,
}

fn store_id() -> EguiId {
    EguiId::new("lumen fading windows")
}

pub(crate) fn remember(ctx: &egui::Context, key: EguiId, layer: LayerId) {
    if motion::time(motion::BASE) <= 0.0 {
        return;
    }
    let shapes: Vec<ClippedShape> = ctx.graphics(|g| {
        g.get(layer)
            .map(|list| list.all_entries().cloned().collect())
            .unwrap_or_default()
    });
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let all = d.get_temp_mut_or_default::<HashMap<EguiId, Picture>>(store_id());
        all.insert(
            key,
            Picture {
                layer,
                shapes: Arc::new(shapes),
                pass,
                gone_at: None,
            },
        );
    });
}

pub fn paint_gone(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    let now = ctx.input(|i| i.time);
    let fade = f64::from(motion::time(motion::BASE));
    let mut all: HashMap<EguiId, Picture> =
        ctx.data(|d| d.get_temp(store_id())).unwrap_or_default();
    if all.is_empty() {
        return;
    }
    all.retain(|_, picture| {
        if picture.pass == pass {
            return true;
        }
        let since = *picture.gone_at.get_or_insert(now);
        let left = 1.0 - (now - since) / fade.max(1e-6);
        if left <= 0.0 {
            return false;
        }
        let shown = motion::ease_out(left as f32);
        let lift = vec2(0.0, -LIFT * (1.0 - shown));
        let mut painter = ctx.layer_painter(picture.layer);
        painter.multiply_opacity(shown);
        for ClippedShape { clip_rect, shape } in picture.shapes.iter() {
            let mut shape = shape.clone();
            shape.translate(lift);
            painter.with_clip_rect(clip_rect.translate(lift)).add(shape);
        }
        ctx.request_repaint();
        true
    });
    ctx.data_mut(|d| d.insert_temp(store_id(), all));
}
