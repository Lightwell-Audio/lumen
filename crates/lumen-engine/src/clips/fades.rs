use std::sync::Arc;

use lumen_core::curve::Curve;

pub(super) const TABLE_STEPS: usize = 1_024;
pub(super) const MAX_TABLES: usize = 256;

pub(super) type Table = Arc<[f32]>;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClipShape {
    pub fade_in: FadeFrames,
    pub fade_out: FadeFrames,
    pub period: Option<i64>,
    pub phase: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FadeFrames {
    pub frames: i64,
    pub offset: i64,
    pub span: i64,
    pub curve: Curve,
    pub blend: bool,
}

#[derive(Clone, Default)]
pub(super) struct Fades(pub(super) [Option<Table>; 2]);

#[inline]
pub(super) fn read(table: &[f32], x: f32) -> f32 {
    let p = x.clamp(0.0, 1.0) * TABLE_STEPS as f32;
    let i = (p as usize).min(TABLE_STEPS - 1);
    match (table.get(i), table.get(i + 1)) {
        (Some(&a), Some(&b)) => a + (b - a) * (p - i as f32),
        _ => 1.0,
    }
}

impl ClipShape {
    pub(super) fn plain(&self) -> bool {
        self.fade_in.frames <= 0 && self.fade_out.frames <= 0 && self.period.is_none()
    }

    #[inline]
    pub(super) fn fade_in_gain(&self, fades: &Fades, pos: i64) -> f32 {
        let fade_in = self.fade_in.frames;
        if pos < fade_in
            && let Some(table) = &fades.0[0]
        {
            let span = self.fade_in.span.max(fade_in);
            let level = read(
                table,
                (pos as f32 + self.fade_in.offset as f32 + 0.5) / span as f32,
            );
            return if self.fade_in.blend {
                (level * std::f32::consts::FRAC_PI_2).sin()
            } else {
                level
            };
        }
        1.0
    }

    #[inline]
    pub(super) fn fade_out_gain(&self, fades: &Fades, pos: i64, length: i64) -> f32 {
        let fade_out = self.fade_out.frames;
        let left = length - pos;
        if left <= fade_out
            && let Some(table) = &fades.0[1]
        {
            let span = self.fade_out.span.max(fade_out);
            let x = (left as f32 + self.fade_out.offset as f32 - 0.5) / span as f32;
            return if self.fade_out.blend {
                (read(table, 1.0 - x) * std::f32::consts::FRAC_PI_2).cos()
            } else {
                read(table, x)
            };
        }
        1.0
    }
}
