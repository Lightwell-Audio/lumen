//! How a clip looks: its colours, title bar and name, from its track,
//! its media's load state and what a recording gives it. `clips` draws
//! the lanes with these.

use eframe::egui::{self, Align2, Color32, FontId, Painter, Rect, Stroke, pos2, vec2};
use lumen_project::model::{Clip, ClipContent};
use lumen_render::{BoxInstance, beats, rgba};

use super::clip_marks::{self, DOT, GAP, MARK, Marks, Placed};
use super::recording_look::Paint;
use super::{TITLE, clip_color};
use crate::lanes::Lanes;
use crate::media::{Item, State};
use crate::{icons, theme};

/// How opaque a clip's body is: the grid shows faintly through it
/// (the owner's G1, MT-APP-14 run 2). The title bar and waveform
/// stay solid.
const BODY_OPACITY: f32 = 0.55;
/// A locked clip's padlock, and the narrowest title that shows it.
const LOCK_ICON: f32 = 9.0;
const LOCK_MIN_WIDTH: f32 = 30.0;
/// A clip's title bar shows which take plays from this width.
const TAKES_MIN_WIDTH: f32 = 72.0;
/// The "REC" tag's dot: its radius, and the room it takes before the name.
const TAG_DOT: f32 = 3.0;
const TAG_ROOM: f32 = 11.0;

/// What a clip's audio is doing, for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ClipState {
    Ready,
    Loading,
    Failed,
    Missing,
}

pub(super) fn item_state(item: &Item) -> ClipState {
    match item.state {
        State::Ready(_) => ClipState::Ready,
        State::Loading => ClipState::Loading,
        State::Failed(_) => ClipState::Failed,
    }
}

/// How a clip looks: its name, track colour and load state, and `alpha`,
/// which fades it in and dims it when the track cannot be heard.
pub(super) struct ClipLook<'a> {
    pub(super) name: &'a str,
    /// The take playing and how many there are (counted from 1), for a clip
    /// that keeps the passes of a loop recording.
    pub(super) takes: Option<(u32, u32)>,
    pub(super) color: Color32,
    pub(super) state: ClipState,
    pub(super) progress: Option<f32>,
    pub(super) alpha: f32,
    /// Locked (the owner's K3, slice 6e): a padlock ends the title bar.
    pub(super) locked: bool,
    /// What a recording gives the clip (3-A): none for a clip not recording.
    pub(super) paint: Paint,
    /// What was done to the clip that its name does not say (M9.5 S3).
    pub(super) marks: Marks,
}

/// A project clip's look on `track`, before dimming for its track.
pub(super) fn look_of<'a>(
    clip: &'a Clip,
    track: &lumen_project::model::Track,
    paint: Paint,
    lanes: &mut Lanes,
    ctx: &egui::Context,
) -> ClipLook<'a> {
    let (state, progress, key) = match &clip.content {
        ClipContent::Audio { media, .. } => (
            lanes
                .library
                .get(media.0)
                .map_or(ClipState::Missing, item_state),
            lanes.library.progress(media.0),
            media.0,
        ),
        ClipContent::Midi { .. } => (ClipState::Ready, None, clip.id.0),
    };
    ClipLook {
        name: &clip.name,
        takes: clip.takes.as_ref().map(|t| (t.active + 1, t.count)),
        color: clip_color(track, clip),
        state,
        progress,
        alpha: lanes.shown(key, ctx),
        // A frozen track's clips cannot change, so they show the padlock too.
        locked: clip.locked || track.frozen.is_some(),
        paint,
        marks: Marks::of(clip),
    }
}

/// A clip body's colour at `alpha`: the clip body at the grid's opacity,
/// unless a recording gives it a colour (the record red take).
fn body_color(paint: &Paint, palette: &theme::Palette, alpha: f32) -> Color32 {
    match paint.body {
        Some(color) => color.gamma_multiply(alpha),
        None => palette.clip_body.gamma_multiply(BODY_OPACITY * alpha),
    }
}

/// A clip's outline at `alpha`: the track colour at 60%, unless a recording
/// gives it the record red.
fn outline_color(paint: &Paint, track: Color32, alpha: f32) -> Color32 {
    match paint.outline {
        Some(color) => color.gamma_multiply(alpha),
        None => track.gamma_multiply(theme::CLIP_EDGE * alpha),
    }
}

/// The title bar's colour and text for a clip in `state`.
fn bar_and_label(look: &ClipLook<'_>, palette: &theme::Palette) -> (Color32, String) {
    let name = look.name;
    match look.state {
        ClipState::Ready => (look.color, name.to_owned()),
        ClipState::Loading => (
            look.color.gamma_multiply(0.6),
            format!(
                "{name} \u{00B7} loading {:.0}%",
                look.progress.unwrap_or(0.0) * 100.0
            ),
        ),
        ClipState::Failed => (palette.record, format!("{name} \u{00B7} could not load")),
        ClipState::Missing => (palette.record, format!("{name} \u{00B7} file missing")),
    }
}

/// A clip's body for the GPU: the same colours as [`draw_body`].
pub(super) fn clip_box(
    clip: &Clip,
    row: u32,
    look: &ClipLook<'_>,
    palette: &theme::Palette,
) -> BoxInstance {
    let straight = |c: Color32| {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        rgba(r, g, b, a)
    };
    let a = look.alpha;
    let (bar, _) = bar_and_label(look, palette);
    BoxInstance {
        start: beats(clip.start.as_quarters()),
        end: beats((clip.start + clip.length).as_quarters()),
        row,
        body: straight(body_color(&look.paint, palette, a)),
        bar: straight(bar.gamma_multiply(a)),
        border: straight(outline_color(&look.paint, look.color, a)),
        fill: straight(look.color.gamma_multiply(0.25 * a)),
        progress: match look.state {
            ClipState::Loading => look.progress.unwrap_or(0.0),
            _ => 0.0,
        },
    }
}

/// A clip body drawn by egui: dark body, track-coloured border and title
/// bar. Only clips still loading for import use it; the rest are on the
/// GPU ([`clip_box`]).
pub(super) fn draw_body(painter: &Painter, rect: Rect, look: &ClipLook<'_>) {
    let palette = theme::p();
    let radius = theme::radius::SMALL;
    let alpha = look.alpha;
    let (bar, _) = bar_and_label(look, palette);
    painter.rect_filled(rect, radius, body_color(&look.paint, palette, alpha));
    if let (ClipState::Loading, Some(p)) = (look.state, look.progress) {
        let done = Rect::from_min_size(rect.min, vec2(rect.width() * p, rect.height()));
        painter.rect_filled(done, radius, look.color.gamma_multiply(0.25 * alpha));
    }
    let title = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE.min(rect.height())));
    painter.rect_filled(
        title,
        egui::CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        },
        bar.gamma_multiply(alpha),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, outline_color(&look.paint, look.color, alpha)),
        egui::StrokeKind::Inside,
    );
}

/// A clip's name in its title bar, kept within the visible part of the
/// lane.
pub(super) fn draw_name(painter: &Painter, rect: Rect, lane: Rect, look: &ClipLook<'_>) {
    let palette = theme::p();
    let title = Rect::from_min_size(rect.min, vec2(rect.width(), TITLE.min(rect.height())));
    let visible = title.intersect(lane);
    if visible.width() <= 8.0 {
        return;
    }
    let (_, label) = bar_and_label(look, palette);
    let alpha = look.alpha;
    let mut room = visible.shrink2(vec2(4.0, 0.0));
    // A recording's tag: a drawn dot (the UI font has no "●") and "REC" in
    // front of the name, so the state never rests on colour alone.
    let (label, text_left) = if look.paint.tag {
        painter.circle_filled(
            pos2(visible.left() + 5.0 + TAG_DOT, title.center().y),
            TAG_DOT,
            theme::CLIP_TEXT.gamma_multiply(alpha),
        );
        (
            format!("REC \u{00B7} {label}"),
            visible.left() + 5.0 + TAG_ROOM,
        )
    } else {
        (label, visible.left() + 5.0)
    };
    if look.locked && visible.width() >= LOCK_MIN_WIDTH {
        let side = LOCK_ICON;
        let at = Rect::from_center_size(
            pos2(visible.right() - 4.0 - side / 2.0, title.center().y),
            vec2(side, side),
        );
        painter.extend(icons::shapes(
            icons::Icon::Lock,
            at,
            theme::CLIP_TEXT.gamma_multiply(0.9 * alpha),
        ));
        room.max.x = at.left() - 3.0;
    }
    let ink = if alpha < 0.9 {
        palette.text_secondary.gamma_multiply(alpha.max(0.6))
    } else {
        theme::CLIP_TEXT.gamma_multiply(alpha)
    };
    // Which take plays, as "2/5" at the right end of the bar.
    if let Some((now, all)) = look.takes
        && visible.width() >= TAKES_MIN_WIDTH
    {
        let galley = painter.layout_no_wrap(
            format!("{now}/{all}"),
            FontId::proportional(theme::size::CAPTION),
            ink,
        );
        let width = galley.size().x;
        let right = room.max.x;
        painter.with_clip_rect(room).galley(
            pos2(right - width, title.center().y - galley.size().y / 2.0),
            galley,
            ink,
        );
        room.max.x = right - width - 4.0;
    }
    // The marks sit left of the padlock and the counter, which keep their
    // places; the name has what is left (`clip_marks::place`).
    let y = title.center().y;
    let placed = clip_marks::place(visible.width(), room.max.x - text_left, look.marks.count());
    match placed {
        Placed::Alone => {
            painter.circle_filled(pos2(visible.center().x, y), DOT / 2.0, ink);
            return;
        }
        Placed::Marks(width) => {
            let mut x = room.max.x - width;
            for mark in look.marks.list() {
                let at = Rect::from_center_size(pos2(x + MARK / 2.0, y), vec2(MARK, MARK));
                painter.extend(icons::shapes(mark.icon(), at, ink));
                x += MARK + GAP;
            }
        }
        Placed::Dot => {
            painter.circle_filled(pos2(room.max.x - DOT / 2.0, y), DOT / 2.0, ink);
        }
        Placed::Nothing => {}
    }
    room.max.x -= placed.takes();
    painter.with_clip_rect(room).text(
        pos2(text_left, title.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme::size::CAPTION),
        // Dimmed clips keep a readable name in both themes.
        ink,
    );
}
