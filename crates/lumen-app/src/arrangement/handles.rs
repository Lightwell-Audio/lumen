//! Fade and gain handles (M3 slice 6d), in the owner's choices: a fade is a
//! glowing curve with a round handle at its top and a dot that bends it
//! (F3); the dot moves freely, the curve passing through it, and Alt adds
//! S-shape (C3, `lumen_core::curve`). Clip gain is a glowing level line
//! across the clip (G1). They show on audio clips.
//!
//! Where everything sits, and what the pointer is over, is pure and tested
//! here; the app turns drags on the handles into document edits
//! (`app/handles.rs`) and the arrangement draws them (`draw_handles`).

use eframe::egui::{self, Color32, Painter, Pos2, Rangef, Rect, Stroke, Vec2, pos2};
use lumen_core::curve::{self, Curve};
use lumen_core::fade::Fade;
use lumen_core::time::Ticks;
use lumen_project::model::{Clip, ClipContent};

use super::TITLE;
use super::clips::WAVE_GAP;
use super::select::Edge;
use crate::{light, theme};

/// The ends of the gain line: clip gain's range.
const MIN_DB: f32 = *crate::document::CLIP_GAIN.start();
const MAX_DB: f32 = *crate::document::CLIP_GAIN.end();
/// The radius of a fade handle, and of a bend dot.
const HANDLE_RADIUS: f32 = 3.5;
const BEND_RADIUS: f32 = 3.0;
/// A fade with no length keeps its handle this far in from the clip's
/// side, clear of the trim edge.
const CORNER: f32 = 8.0;
/// How close the pointer must be to a fade handle, a bend dot, or the gain
/// line.
const FADE_GRAB: f32 = 6.0;
const BEND_GRAB: f32 = 5.0;
const LINE_GRAB: f32 = 3.0;
/// Clips narrower than this, or with a shorter wave area, show no handles.
const MIN_WIDTH: f32 = 28.0;
const MIN_HEIGHT: f32 = 10.0;
/// A fade narrower than this shows no bend dot.
const BEND_MIN: f32 = 16.0;
/// The coarsest a gain drag gets (Shift: a tenth of it).
const MAX_DB_PER_POINT: f32 = 0.5;
/// How far up an Alt-drag on a bend dot goes from no S-shape to all of it.
pub const S_SPAN: f32 = 40.0;

/// A handle on a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    /// The top of a fade: drag it along to set the fade's length.
    Fade(Edge),
    /// The dot on a fade's curve: drag it anywhere to bend the fade, with
    /// Alt for S-shape.
    Bend(Edge),
    /// The gain line: drag it up or down.
    Gain,
}

/// A fade on screen: it runs from `from` to `to` (points), silent at the
/// clip's side and full at the other end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FadeLook {
    pub edge: Edge,
    pub from: f32,
    pub to: f32,
    pub curve: Curve,
    offset: f32,
    portion: f32,
}

impl FadeLook {
    fn new(edge: Edge, fade: Fade, side: f32, inner: f32) -> Self {
        let (from, to) = match edge {
            Edge::Start => (side, inner),
            Edge::End => (inner, side),
        };
        Self {
            edge,
            from,
            to,
            curve: fade.curve,
            offset: fade.offset.0 as f32 / fade.span.0.max(fade.length.0).max(1) as f32,
            portion: fade.length.0 as f32 / fade.span.0.max(fade.length.0).max(1) as f32,
        }
    }

    pub fn width(&self) -> f32 {
        self.to - self.from
    }

    /// The fade's level at `x`, 0 (silent) to 1, as the engine plays it.
    pub fn level_at(&self, x: f32) -> f32 {
        if self.width() <= 0.0 {
            return 1.0;
        }
        let t = ((x - self.from) / self.width()).clamp(0.0, 1.0);
        match self.edge {
            Edge::Start => self.curve.at(self.offset + t * self.portion),
            Edge::End => self.curve.at(self.offset + (1.0 - t) * self.portion),
        }
    }

    /// Where the point `[u, v]` of the curve (u along it from silence, v
    /// its level) is across: a fade out runs backwards.
    fn x_of(&self, u: f32) -> f32 {
        let u = (u - self.offset) / self.portion.max(f32::EPSILON);
        match self.edge {
            Edge::Start => self.from + u * self.width(),
            Edge::End => self.to - u * self.width(),
        }
    }

    /// Where the fade reaches full level: its handle's x.
    fn inner(&self) -> f32 {
        match self.edge {
            Edge::Start => self.to,
            Edge::End => self.from,
        }
    }
}

/// An audio clip's handles on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipHandles {
    /// Where the waveform is drawn.
    pub wave: Rect,
    pub fade_in: FadeLook,
    pub fade_out: FadeLook,
    /// The incoming fade is a crossfade; draw its outgoing partner too.
    pub crossfade: bool,
    pub gain_db: f32,
}

/// Where the waveform of a clip drawn in `rect` goes (as the GPU draws it).
pub fn wave_area(rect: Rect) -> Rect {
    Rect::from_min_max(
        pos2(rect.left() + 1.0, rect.top() + TITLE + WAVE_GAP),
        pos2(rect.right() - 1.0, rect.bottom() - WAVE_GAP),
    )
}

/// Where `db` sits up the wave area, 0 (bottom, silence) to 1 (+12 dB):
/// even in dB, so 0 dB is five sixths of the way up.
pub fn db_position(db: f32) -> f32 {
    ((db - MIN_DB) / (MAX_DB - MIN_DB)).clamp(0.0, 1.0)
}

/// How many dB one point of a drag moves the gain line, in a wave area
/// `height` points tall: in a tall clip the line follows the pointer; in a
/// short one it moves at most [`MAX_DB_PER_POINT`], or it would be too
/// coarse to set (about 4 dB a point at the default track height).
pub fn db_per_point(height: f32) -> f32 {
    ((MAX_DB - MIN_DB) / height.max(1.0)).min(MAX_DB_PER_POINT)
}

/// A bend handle moved by the pointer moving `by` points over a fade at
/// `edge` drawn `size` points wide and tall (C3): the dot follows the
/// pointer, within the curve's middle three quarters; with `alt`, up adds
/// S-shape and down takes it away, [`S_SPAN`] points for all of it.
pub fn bend(handle: curve::Handle, edge: Edge, size: Vec2, by: Vec2, alt: bool) -> curve::Handle {
    if alt {
        return curve::Handle {
            s: (handle.s - by.y / S_SPAN).clamp(0.0, 1.0),
            ..handle
        };
    }
    let across = match edge {
        Edge::Start => by.x,
        Edge::End => -by.x,
    };
    let [u, v] = handle.dot;
    curve::Handle {
        dot: [
            (u + across / size.x.max(1.0)).clamp(curve::DOT_MIN, curve::DOT_MAX),
            (v - by.y / size.y.max(1.0)).clamp(curve::DOT_MIN, curve::DOT_MAX),
        ],
        ..handle
    }
}

impl ClipHandles {
    /// The handles of `clip` drawn in `rect`, with `x_of` placing a time on
    /// screen. `None` for clips without audio, and clips too small for
    /// handles to be grabbed.
    pub fn of(clip: &Clip, rect: Rect, x_of: impl Fn(Ticks) -> f32) -> Option<Self> {
        let ClipContent::Audio { gain_db, .. } = clip.content else {
            return None;
        };
        let wave = wave_area(rect);
        if rect.width() < MIN_WIDTH || wave.height() < MIN_HEIGHT {
            return None;
        }
        let end = clip.start + clip.length;
        Some(Self {
            wave,
            fade_in: FadeLook::new(
                Edge::Start,
                clip.fade_in,
                rect.left(),
                x_of(clip.start + clip.fade_in.length),
            ),
            fade_out: FadeLook::new(
                Edge::End,
                clip.fade_out,
                rect.right(),
                x_of(end - clip.fade_out.length),
            ),
            crossfade: clip.crossfade,
            gain_db,
        })
    }

    pub fn fade(&self, edge: Edge) -> &FadeLook {
        match edge {
            Edge::Start => &self.fade_in,
            Edge::End => &self.fade_out,
        }
    }

    /// The y of `level` (0 to 1) in the wave area.
    pub fn y_of(&self, level: f32) -> f32 {
        self.wave.bottom() - level * self.wave.height()
    }

    /// Where a fade's handle is: at the top of the wave area where the
    /// fade reaches full level, kept clear of the clip's side.
    pub fn handle_pos(&self, edge: Edge) -> Pos2 {
        let (left, right) = (self.wave.left() + CORNER, self.wave.right() - CORNER);
        let x = self.fade(edge).inner();
        let x = match edge {
            Edge::Start => x.max(left).min(right),
            Edge::End => x.min(right).max(left),
        };
        pos2(x, self.wave.top())
    }

    /// Where a fade's bend dot is (its curve's handle, on the curve), if
    /// the fade is long enough to show one.
    pub fn bend_pos(&self, edge: Edge) -> Option<Pos2> {
        let fade = self.fade(edge);
        if fade.width() < BEND_MIN {
            return None;
        }
        let [u, v] = fade.curve.handle().dot;
        if u < fade.offset || u > fade.offset + fade.portion {
            return None;
        }
        let level = if self.crossfade && edge == Edge::Start {
            equal_power_gains(fade.level_at(fade.x_of(u))).0
        } else {
            v
        };
        Some(pos2(fade.x_of(u), self.y_of(level)))
    }

    /// How big a fade is drawn, for bending it: its width and the wave
    /// area's height.
    pub fn fade_size(&self, edge: Edge) -> Vec2 {
        egui::vec2(self.fade(edge).width(), self.wave.height())
    }

    /// The y of the gain line.
    pub fn gain_y(&self) -> f32 {
        self.y_of(db_position(self.gain_db))
    }

    /// The handle under `p`: fade handles first, then bend dots, then the
    /// gain line.
    pub fn handle_at(&self, p: Pos2) -> Option<Handle> {
        let near = |at: Pos2, reach: f32| at.distance(p) <= reach;
        for edge in [Edge::Start, Edge::End] {
            if near(self.handle_pos(edge), FADE_GRAB) {
                return Some(Handle::Fade(edge));
            }
        }
        for edge in [Edge::Start, Edge::End] {
            if self.bend_pos(edge).is_some_and(|at| near(at, BEND_GRAB)) {
                return Some(Handle::Bend(edge));
            }
        }
        let across = self.wave.x_range().contains(p.x);
        (across && (p.y - self.gain_y()).abs() <= LINE_GRAB).then_some(Handle::Gain)
    }

    /// Points along a fade's curve from `x0` to `x1` (the part in view),
    /// about every two points, for drawing.
    pub fn curve(&self, edge: Edge, x0: f32, x1: f32) -> Vec<Pos2> {
        let fade = self.fade(edge);
        let (a, b) = (fade.from.max(x0), fade.to.min(x1));
        if b <= a {
            return Vec::new();
        }
        let steps = ((b - a) / 2.0).ceil().max(1.0) as usize;
        (0..=steps)
            .map(|i| {
                let x = a + (b - a) * i as f32 / steps as f32;
                pos2(x, self.y_of(fade.level_at(x)))
            })
            .collect()
    }
}

/// Draws `h` on `painter`, within `view` (the lanes' x range, so nothing
/// sized by a long clip is drawn whole). A fade is a quiet curve until the
/// clip is `hovered`; then fades glow in the accent with their handles and
/// bend dots, and the gain line glows across the clip. `hot` is the handle
/// under the pointer or being dragged.
pub fn draw_handles(
    painter: &Painter,
    h: &ClipHandles,
    view: Rangef,
    hovered: bool,
    hot: Option<Handle>,
) {
    let palette = theme::p();
    let accent = palette.accent;
    for edge in [Edge::Start, Edge::End] {
        let mut curve = h.curve(edge, view.min, view.max);
        let other = if h.crossfade && edge == Edge::Start {
            let mut incoming = Vec::with_capacity(curve.len());
            let mut outgoing = Vec::with_capacity(curve.len());
            for point in curve {
                let progress = ((h.wave.bottom() - point.y) / h.wave.height()).clamp(0.0, 1.0);
                let (in_gain, out_gain) = equal_power_gains(progress);
                incoming.push(pos2(point.x, h.y_of(in_gain)));
                outgoing.push(pos2(point.x, h.y_of(out_gain)));
            }
            curve = incoming;
            Some(outgoing)
        } else {
            None
        };
        if curve.len() < 2 {
            continue;
        }
        if !hovered {
            if h.crossfade && edge == Edge::Start {
                let other = other.expect("crossfade outgoing curve");
                painter.line(
                    curve,
                    Stroke::new(theme::stroke::HAIRLINE, palette.text.gamma_multiply(0.45)),
                );
                painter.line(
                    other,
                    Stroke::new(theme::stroke::HAIRLINE, palette.text.gamma_multiply(0.45)),
                );
            } else {
                painter.line(
                    curve,
                    Stroke::new(theme::stroke::HAIRLINE, palette.text.gamma_multiply(0.45)),
                );
            }
            continue;
        }
        // What the fade takes away: a light wash above the curve.
        let mut wash = egui::Mesh::default();
        let tint = accent.gamma_multiply(0.10);
        for p in &curve {
            wash.colored_vertex(*p, tint);
            wash.colored_vertex(pos2(p.x, h.wave.top()), tint);
        }
        for i in 0..curve.len() as u32 - 1 {
            let k = 2 * i;
            wash.add_triangle(k, k + 1, k + 2);
            wash.add_triangle(k + 1, k + 3, k + 2);
        }
        painter.add(wash);
        glow_line(painter, curve, accent, hot == Some(Handle::Bend(edge)));
        if let Some(other) = other {
            glow_line(painter, other, accent, hot == Some(Handle::Bend(edge)));
        }
    }
    if !hovered {
        return;
    }
    // The gain line (G1), across the part of the clip in view: quiet until
    // the pointer is on it, so it does not crowd the fades.
    let (x0, x1) = (h.wave.left().max(view.min), h.wave.right().min(view.max));
    if x1 > x0 {
        let line = vec![pos2(x0, h.gain_y()), pos2(x1, h.gain_y())];
        if hot == Some(Handle::Gain) {
            glow_line(painter, line, accent, true);
        } else {
            painter.line(line, Stroke::new(1.0, accent.gamma_multiply(0.55)));
        }
    }
    for edge in [Edge::Start, Edge::End] {
        let at = h.handle_pos(edge);
        if !view.contains(at.x) {
            continue;
        }
        let big = if hot == Some(Handle::Fade(edge)) {
            1.0
        } else {
            0.0
        };
        if h.fade(edge).width() > 0.0 {
            painter.circle_filled(at, HANDLE_RADIUS + 2.5 + big, accent.gamma_multiply(0.2));
            painter.circle_filled(at, HANDLE_RADIUS + big, accent);
        } else {
            // No fade yet: an open ring waits in the corner.
            painter.circle_stroke(
                at,
                HANDLE_RADIUS - 0.5 + big,
                Stroke::new(1.5, accent.gamma_multiply(0.8)),
            );
        }
        if let Some(dot) = h.bend_pos(edge).filter(|d| view.contains(d.x)) {
            let big = if hot == Some(Handle::Bend(edge)) {
                1.0
            } else {
                0.0
            };
            painter.circle_filled(dot, BEND_RADIUS + 2.0 + big, accent.gamma_multiply(0.2));
            painter.circle(
                dot,
                BEND_RADIUS + big,
                palette.clip_body,
                Stroke::new(1.5, accent),
            );
        }
    }
}

/// Equal-power incoming and outgoing gains at normalized fade progress.
fn equal_power_gains(progress: f32) -> (f32, f32) {
    let angle = progress.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    (angle.sin(), angle.cos())
}

/// A glowing accent line through `points` (the [`light::LIT`] level),
/// brighter when `hot`.
fn glow_line(painter: &Painter, points: Vec<Pos2>, accent: Color32, hot: bool) {
    let lift = if hot { 1.6 } else { 1.0 };
    light::polyline(painter, light::LIT, points, accent, lift);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::model::Id;

    #[test]
    fn equal_power_crossfade_curves_have_constant_squared_gain() {
        for step in 0..=100 {
            let progress = step as f32 / 100.0;
            let (incoming, outgoing) = equal_power_gains(progress);
            assert!((incoming * incoming + outgoing * outgoing - 1.0).abs() < 1e-6);
        }
    }

    /// An audio clip of four beats drawn 100 points a beat from x = 0.
    fn setup(fade_in: Fade, fade_out: Fade, gain_db: f32) -> ClipHandles {
        let clip = Clip {
            id: Id(1),
            name: "kick".into(),
            start: Ticks::ZERO,
            length: Ticks::quarters(4),
            muted: false,
            color: None,
            locked: false,
            crossfade: false,
            fade_in,
            fade_out,
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            content: ClipContent::Audio {
                media: Id(2),
                offset: 0,
                gain_db,
            },
        };
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 80.0));
        ClipHandles::of(&clip, rect, |t| (t.as_quarters() * 100.0) as f32).unwrap()
    }

    fn fade(beats: i64, curve: Curve) -> Fade {
        Fade {
            length: Ticks::quarters(beats),
            curve,
            ..Default::default()
        }
    }

    #[test]
    fn fades_are_drawn_as_the_engine_plays_them() {
        let h = setup(fade(1, Curve::SLOW), fade(2, Curve::FAST), 0.0);
        assert_eq!((h.fade_in.from, h.fade_in.to), (0.0, 100.0));
        assert_eq!((h.fade_out.from, h.fade_out.to), (200.0, 400.0));
        // The same curve the engine plays, backwards for a fade-out.
        assert_eq!(h.fade_in.level_at(50.0), Curve::SLOW.at(0.5));
        assert_eq!(h.fade_out.level_at(250.0), Curve::FAST.at(0.75));
        assert_eq!(h.fade_in.level_at(300.0), 1.0);
        let curve = h.curve(Edge::Start, -50.0, 1_000.0);
        assert_eq!(
            curve.first().unwrap().y,
            h.wave.bottom(),
            "silent at the side"
        );
        assert_eq!(curve.last().unwrap().y, h.wave.top(), "full at the handle");
        assert!(h.curve(Edge::Start, 150.0, 300.0).is_empty(), "out of view");
    }

    #[test]
    fn handles_sit_at_the_fades_tops_and_clear_of_the_sides() {
        let h = setup(fade(1, Curve::LINEAR), Fade::default(), 0.0);
        assert_eq!(h.handle_pos(Edge::Start), pos2(100.0, h.wave.top()));
        // No fade: the handle waits in the corner, clear of the trim edge.
        assert_eq!(h.handle_pos(Edge::End).x, h.wave.right() - CORNER);
        assert!(h.bend_pos(Edge::End).is_none(), "no fade, no bend dot");
        let bend = h.bend_pos(Edge::Start).unwrap();
        assert_eq!(bend.x, 50.0);
        assert_eq!(bend.y, h.y_of(0.5), "linear: half way up");
    }

    #[test]
    fn the_pointer_finds_fade_handles_then_bends_then_the_gain_line() {
        let h = setup(fade(1, Curve::LINEAR), Fade::default(), 0.0);
        let at = |x: f32, y: f32| h.handle_at(pos2(x, y));
        assert_eq!(
            at(102.0, h.wave.top() + 2.0),
            Some(Handle::Fade(Edge::Start))
        );
        assert_eq!(
            at(h.wave.right() - CORNER, h.wave.top()),
            Some(Handle::Fade(Edge::End))
        );
        assert_eq!(at(50.0, h.y_of(0.5) + 1.0), Some(Handle::Bend(Edge::Start)));
        assert_eq!(at(250.0, h.gain_y() - 2.0), Some(Handle::Gain));
        assert_eq!(at(250.0, h.gain_y() + 10.0), None);
    }

    #[test]
    fn gain_runs_even_in_db_and_the_line_follows_the_pointer_in_tall_clips() {
        assert_eq!(db_position(MIN_DB), 0.0);
        assert_eq!(db_position(MAX_DB), 1.0);
        assert!((db_position(0.0) - 5.0 / 6.0).abs() < 1e-6);
        assert_eq!(db_position(-100.0), 0.0, "clamped");
        let loud = setup(Fade::default(), Fade::default(), 0.0);
        let soft = setup(Fade::default(), Fade::default(), -12.0);
        // In a tall clip one point of drag is the dB of one point of line.
        let tall = 72.0 / MAX_DB_PER_POINT;
        assert_eq!(db_per_point(tall), MAX_DB_PER_POINT);
        assert_eq!(db_per_point(2.0 * tall), MAX_DB_PER_POINT / 2.0);
        // Shorter, it never gets coarser than that.
        assert_eq!(db_per_point(loud.wave.height()), MAX_DB_PER_POINT);
        assert_eq!(db_per_point(19.0), MAX_DB_PER_POINT);
        let spread = soft.gain_y() - loud.gain_y();
        assert!((spread / loud.wave.height() - 12.0 / 72.0).abs() < 1e-5);
    }

    #[test]
    fn a_bend_dot_follows_the_pointer_and_alt_adds_s_shape() {
        let size = egui::vec2(100.0, 50.0);
        let start = Curve::LINEAR.handle();
        // Up and to the right on a fade in: further along, louder.
        let moved = bend(start, Edge::Start, size, egui::vec2(10.0, -5.0), false);
        assert!((moved.dot[0] - 0.6).abs() < 1e-6 && (moved.dot[1] - 0.6).abs() < 1e-6);
        // A fade out runs backwards: right is back towards its start.
        let back = bend(start, Edge::End, size, egui::vec2(10.0, 0.0), false);
        assert!((back.dot[0] - 0.4).abs() < 1e-6);
        // Never past the middle three quarters.
        let far = bend(start, Edge::Start, size, egui::vec2(1e4, 1e4), false);
        assert_eq!(far.dot, [curve::DOT_MAX, curve::DOT_MIN]);
        // Alt: up adds S-shape and leaves the dot where it is.
        let s = bend(
            start,
            Edge::Start,
            size,
            egui::vec2(30.0, -S_SPAN / 2.0),
            true,
        );
        assert_eq!((s.dot, s.s), (start.dot, 0.5));
        assert_eq!(
            bend(s, Edge::Start, size, egui::vec2(0.0, 1e3), true).s,
            0.0
        );
    }

    #[test]
    fn the_bend_dot_sits_on_the_curve_where_it_bends() {
        let bent = Curve::from_handle(curve::Handle {
            dot: [0.25, 0.75],
            s: 0.0,
        });
        let h = setup(fade(1, bent), fade(1, bent), 0.0);
        let dot = h.bend_pos(Edge::Start).unwrap();
        assert!((dot.x - 25.0).abs() < 1e-3, "a quarter along: {dot:?}");
        assert!(
            (h.fade_in.level_at(dot.x) - 0.75).abs() < 1e-3,
            "on the curve"
        );
        // The fade out (300 to 400) mirrors it.
        let dot = h.bend_pos(Edge::End).unwrap();
        assert!((dot.x - 375.0).abs() < 1e-3, "{dot:?}");
        assert!((h.fade_out.level_at(dot.x) - 0.75).abs() < 1e-3);
    }

    #[test]
    fn small_and_midi_clips_have_no_handles() {
        let mut clip = Clip {
            id: Id(1),
            name: String::new(),
            start: Ticks::ZERO,
            length: Ticks::quarters(1),
            muted: false,
            color: None,
            locked: false,
            crossfade: false,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            content: ClipContent::Audio {
                media: Id(2),
                offset: 0,
                gain_db: 0.0,
            },
        };
        let x = |t: Ticks| t.as_quarters() as f32 * 20.0;
        let narrow = Rect::from_min_max(pos2(0.0, 0.0), pos2(20.0, 80.0));
        assert!(ClipHandles::of(&clip, narrow, x).is_none());
        let short = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 24.0));
        assert!(ClipHandles::of(&clip, short, x).is_none());
        clip.content = ClipContent::midi();
        let roomy = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 80.0));
        assert!(ClipHandles::of(&clip, roomy, x).is_none());
    }
}
