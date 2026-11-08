//! The audio editor's handles (M9.5 S2a): where each one is for a clip drawn
//! in the wave area, what the pointer is on, how they are drawn at rest, and
//! what a typed value means. The edits themselves are the arrangement's
//! (`app::handles`); what the editor does with them is
//! `app::audio_editor::{gestures, keys}`.

use eframe::egui::{Painter, Pos2, Rangef, Rect, Stroke, StrokeKind, pos2, vec2};
use lumen_core::time::Ticks;
use lumen_project::model::Clip;

use super::source::WholeFile;
use crate::arrangement::handles::{self, ClipHandles, Handle, HandleLook};
use crate::arrangement::select::{EDGE_ZONE, Edge};
use crate::timeview::TimeView;
use crate::widgets::raised::FOCUS_OUTSET;
use crate::{motion, theme};

/// Under this width a clip's fades are too close to its grips to be told
/// apart: only the grips take the pointer (the arrangement's own least).
const FADES_FROM: f32 = handles::MIN_WIDTH;
/// A trim grip's room when it has the keyboard.
const TRIM_STOP: f32 = 12.0;
/// A fade handle's room when it has the keyboard.
const FADE_STOP: f32 = 16.0;
/// The gain line's room when it has the keyboard.
const GAIN_STOP: f32 = 8.0;

/// A part of the clip that can be edited in the wave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Trim(Edge),
    Handle(Handle),
}

/// The keyboard's stops, in Tab order. A bend has none: Up and Down on a fade
/// bend it (M9.5 D17).
pub(crate) const STOPS: [Part; 5] = [
    Part::Trim(Edge::Start),
    Part::Handle(Handle::Fade(Edge::Start)),
    Part::Handle(Handle::Gain),
    Part::Handle(Handle::Fade(Edge::End)),
    Part::Trim(Edge::End),
];

impl Part {
    /// What tests and logs call it.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Part::Trim(Edge::Start) => "trim start",
            Part::Trim(Edge::End) => "trim end",
            Part::Handle(Handle::Fade(Edge::Start)) => "fade in",
            Part::Handle(Handle::Fade(Edge::End)) => "fade out",
            Part::Handle(Handle::Bend(Edge::Start)) => "fade in bend",
            Part::Handle(Handle::Bend(Edge::End)) => "fade out bend",
            Part::Handle(Handle::Gain) => "gain",
        }
    }

    /// The fade this part belongs to.
    pub(crate) fn fade_edge(self) -> Option<Edge> {
        match self {
            Part::Handle(Handle::Fade(edge) | Handle::Bend(edge)) => Some(edge),
            _ => None,
        }
    }
}

/// What the pointer is on in the wave area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaveHit {
    Part(Part),
    /// The bright part: what the clip plays.
    Played,
    /// The dimmed part of the file.
    File,
    /// Past the file's ends.
    Outside,
}

/// A clip's handles in the wave area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EditorHandles {
    pub fades: ClipHandles,
    /// The x of the clip's start and end, which may be far out of view.
    pub start: f32,
    pub end: f32,
    pub body: Rect,
    /// The gain line is hidden: the Gain envelope is shown over the wave
    /// (M9.5 D20).
    pub gain_hidden: bool,
}

impl EditorHandles {
    /// The handles of `clip` (as shown: `App::shown_clip`) drawn in `body` at
    /// `time`. Its real start and end, wherever the part played lies: a
    /// looping clip's end is past its first repeat. `None` for a clip that
    /// is not audio.
    pub(crate) fn of(clip: &Clip, body: Rect, time: TimeView) -> Option<Self> {
        let x_of = |t: Ticks| time.x(body.left(), t.as_quarters());
        let (start, end) = (x_of(clip.start), x_of(clip.start + clip.length));
        let wave = Rect::from_x_y_ranges(
            start..=end,
            body.top() + theme::space::XS..=body.bottom() - theme::space::XS,
        );
        Some(Self {
            fades: ClipHandles::in_wave(clip, wave, x_of)?,
            start,
            end,
            body,
            gain_hidden: false,
        })
    }

    /// The same handles, with the gain line hidden while the Gain envelope
    /// is shown (`pill`).
    pub(crate) fn hiding_gain(mut self, pill: Option<lumen_project::envelope::Kind>) -> Self {
        self.gain_hidden = pill == Some(lumen_project::envelope::Kind::Gain);
        self
    }

    fn roomy(&self) -> bool {
        self.end - self.start >= FADES_FROM
    }

    /// The part under `p`: a fade's handle first, as the corners are where
    /// fades start; then the grips, the nearer one when a short clip's
    /// overlap; then a bend dot, then the gain line.
    pub(crate) fn part_at(&self, p: Pos2) -> Option<Part> {
        let on = if self.roomy() {
            self.fades.handle_at(p)
        } else {
            None
        };
        if let Some(handle @ Handle::Fade(_)) = on {
            return Some(Part::Handle(handle));
        }
        let (to_start, to_end) = ((p.x - self.start).abs(), (p.x - self.end).abs());
        if to_start.min(to_end) <= EDGE_ZONE {
            let edge = if to_start <= to_end {
                Edge::Start
            } else {
                Edge::End
            };
            return Some(Part::Trim(edge));
        }
        on.filter(|h| !(self.gain_hidden && *h == Handle::Gain))
            .map(Part::Handle)
    }

    /// What `p` is on, in a file drawn at `time`.
    pub(crate) fn hit(&self, p: Pos2, file: &WholeFile, time: TimeView) -> WaveHit {
        if let Some(part) = self.part_at(p) {
            return WaveHit::Part(part);
        }
        let q = time.beat_at(self.body.left(), p.x);
        if file.played.contains(q) {
            WaveHit::Played
        } else if q >= file.from && q < file.to {
            WaveHit::File
        } else {
            WaveHit::Outside
        }
    }

    /// The rectangle a stop has for the keyboard and assistive technology:
    /// kept inside the wave area, as a handle can be far out of view, with
    /// room for its whole focus ring (the wave area clips what it draws).
    pub(crate) fn stop_rect(&self, part: Part) -> Rect {
        let mid = self.body.center().y;
        let rect = match part {
            Part::Trim(edge) => {
                let x = match edge {
                    Edge::Start => self.start,
                    Edge::End => self.end,
                };
                Rect::from_center_size(pos2(x, mid), vec2(TRIM_STOP, theme::size::GRIP_HEIGHT))
            }
            Part::Handle(Handle::Fade(edge) | Handle::Bend(edge)) => {
                Rect::from_center_size(self.fades.handle_pos(edge), vec2(FADE_STOP, FADE_STOP))
            }
            Part::Handle(Handle::Gain) => {
                let left = self.start.max(self.body.left());
                let right = self.end.min(self.body.right());
                let y = self.fades.gain_y();
                if right - left >= FADE_STOP {
                    Rect::from_x_y_ranges(left..=right, y - GAIN_STOP / 2.0..=y + GAIN_STOP / 2.0)
                } else {
                    Rect::from_center_size(
                        pos2((left + right) / 2.0, y),
                        vec2(FADE_STOP, GAIN_STOP),
                    )
                }
            }
        };
        inside(rect, self.body.shrink(FOCUS_OUTSET))
    }
}

/// The beat to bring into view for `part` of `clip`, in a view of `body`:
/// `None` for the gain line while some of the clip is showing.
pub(crate) fn beat_of(clip: &Clip, part: Part, time: TimeView, body: Rect) -> Option<f64> {
    let end = clip.start + clip.length;
    Some(match part {
        Part::Trim(Edge::Start) => clip.start.as_quarters(),
        Part::Trim(Edge::End) => end.as_quarters(),
        Part::Handle(Handle::Fade(Edge::Start) | Handle::Bend(Edge::Start)) => {
            (clip.start + clip.fade_in.length).as_quarters()
        }
        Part::Handle(Handle::Fade(Edge::End) | Handle::Bend(Edge::End)) => {
            (end - clip.fade_out.length).as_quarters()
        }
        Part::Handle(Handle::Gain) => {
            let left = time.x(body.left(), clip.start.as_quarters());
            let right = time.x(body.left(), end.as_quarters());
            if right > body.left() && left < body.right() {
                return None;
            }
            clip.start.as_quarters()
        }
    })
}

/// `rect` moved the least to lie inside `area`.
fn inside(rect: Rect, area: Rect) -> Rect {
    let dx = (area.left() - rect.left()).max(0.0) + (area.right() - rect.right()).min(0.0);
    let dy = (area.top() - rect.top()).max(0.0) + (area.bottom() - rect.bottom()).min(0.0);
    rect.translate(vec2(dx, dy))
}

/// How the handles are shown this frame.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Look {
    /// Under the pointer, dragged, or with the keyboard.
    pub hot: Option<Part>,
    /// The clip takes no edit: its grips are hollow.
    pub fixed: bool,
    /// The colours of the two clips a crossfade blends.
    pub crossing: Option<(eframe::egui::Color32, eframe::egui::Color32)>,
}

/// Draws the handles on `painter`, which the caller keeps to the wave area:
/// only what is in view, whatever the clip's width.
pub(crate) fn draw(painter: &Painter, h: &EditorHandles, look: Look) {
    let view = h.body.x_range();
    let hot_handle = match look.hot {
        Some(Part::Handle(handle)) => Some(handle),
        _ => None,
    };
    handles::draw_handles(
        painter,
        &h.fades,
        view,
        HandleLook {
            hovered: false,
            hot: None,
            crossing: look.crossing,
        },
    );
    if h.roomy() {
        draw_resting(painter, &h.fades, view, hot_handle, h.gain_hidden);
    }
    for (edge, x) in [(Edge::Start, h.start), (Edge::End, h.end)] {
        if view.contains(x) {
            let hot = look.hot == Some(Part::Trim(edge));
            draw_grip(painter, x, h.body.y_range(), hot, look.fixed);
        }
    }
}

/// A clip's side: a line down the wave with a tab at its middle, hollow when
/// the clip takes no edit, in the accent when hot.
fn draw_grip(painter: &Painter, x: f32, y: Rangef, hot: bool, fixed: bool) {
    let palette = theme::p();
    let (ink, width) = if hot {
        (palette.accent, theme::stroke::LIT)
    } else {
        (
            palette.text_secondary.gamma_multiply(theme::GRIP_QUIET),
            theme::stroke::HAIRLINE,
        )
    };
    painter.vline(x, y, Stroke::new(width, ink));
    let tab = Rect::from_center_size(
        pos2(x, (y.min + y.max) / 2.0),
        vec2(theme::size::GRIP_WIDTH, theme::size::GRIP_HEIGHT),
    );
    let radius = theme::radius::for_height(theme::size::GRIP_WIDTH);
    if fixed {
        painter.rect_filled(tab, radius, palette.canvas);
        painter.rect_stroke(
            tab,
            radius,
            Stroke::new(theme::stroke::HAIRLINE, ink),
            StrokeKind::Inside,
        );
    } else {
        painter.rect_filled(tab, radius, ink);
    }
}

/// The handles as the audio editor keeps them at rest (the owner's HS A),
/// over [`handles::draw_handles`]' quiet curves: the gain line, the fades'
/// handles and their bend dots in hairlines and small marks, so what can be
/// grabbed is plain before the pointer gets there. `hot` lights up as the
/// arrangement's does.
fn draw_resting(
    painter: &Painter,
    h: &ClipHandles,
    view: Rangef,
    hot: Option<Handle>,
    gain_hidden: bool,
) {
    let palette = theme::p();
    let accent = palette.accent;
    let quiet = palette.text.gamma_multiply(theme::HANDLE_QUIET);
    for edge in [Edge::Start, Edge::End] {
        if matches!(hot, Some(Handle::Fade(e) | Handle::Bend(e)) if e == edge) {
            handles::draw_hot_fade(painter, h, edge, view);
        }
    }
    let (x0, x1) = (h.wave.left().max(view.min), h.wave.right().min(view.max));
    if x1 > x0 && !gain_hidden {
        let line = vec![pos2(x0, h.gain_y()), pos2(x1, h.gain_y())];
        if hot == Some(Handle::Gain) {
            handles::glow_line(painter, line, accent, true);
        } else {
            painter.line(line, Stroke::new(theme::stroke::HAIRLINE, quiet));
        }
    }
    for edge in [Edge::Start, Edge::End] {
        let at = h.handle_pos(edge);
        if view.contains(at.x) {
            let lit = hot == Some(Handle::Fade(edge));
            let ink = if lit { accent } else { quiet };
            if lit {
                painter.circle_filled(at, handles::HANDLE_RADIUS + 2.5, accent.gamma_multiply(0.2));
            }
            if h.fade(edge).width() > 0.0 {
                painter.circle_filled(at, handles::HANDLE_RADIUS - 0.5, ink);
            } else {
                painter.circle_stroke(
                    at,
                    handles::HANDLE_RADIUS - 0.5,
                    Stroke::new(theme::stroke::HAIRLINE, ink),
                );
            }
        }
        if let Some(dot) = h.bend_pos(edge).filter(|d| view.contains(d.x)) {
            let lit = hot == Some(Handle::Bend(edge));
            let ink = if lit { accent } else { quiet };
            painter.circle(
                dot,
                handles::BEND_RADIUS - 0.5,
                palette.clip_body,
                Stroke::new(theme::stroke::HAIRLINE, ink),
            );
        }
    }
}

/// How far a drag at `x`, pressed at `from`, scrolls the view in `dt`
/// seconds: within `theme::size::EDGE_SCROLL` of `lane`'s sides, faster the
/// closer, up to `motion::EDGE_SCROLL_SPEED` at the side and past it.
/// Positive moves the content right (the pointer is at the left side). Only
/// a hand moving outwards scrolls: a grip pressed inside the zone (a clip's
/// end framed by Zoom to clip) stays put until the hand goes on past where
/// it pressed, and then speeds up from nothing.
pub(crate) fn edge_scroll(x: f32, from: f32, lane: Rangef, dt: f32) -> f32 {
    let zone = theme::size::EDGE_SCROLL.min(lane.span() / 4.0).max(1.0);
    let ramp = |depth: f32, room: f32| (depth / room.max(1.0)).clamp(0.0, 1.0);
    let left_starts = (lane.min + zone).min(from);
    let right_starts = (lane.max - zone).max(from);
    let left = ramp(left_starts - x, left_starts - lane.min);
    let right = ramp(x - right_starts, lane.max - right_starts);
    (left - right) * motion::EDGE_SCROLL_SPEED * dt
}

/// A gain as typed: "-3", "−3.0 dB", "+2.5db".
pub(crate) fn parse_db(input: &str) -> Result<f32, String> {
    let text = input.trim().to_lowercase().replace('\u{2212}', "-");
    let number = text.strip_suffix("db").unwrap_or(&text).trim();
    number
        .parse::<f32>()
        .ok()
        .filter(|db| db.is_finite())
        .ok_or_else(|| format!("{:?} is not a gain. Type it in dB, like -3.0", input.trim()))
}

/// A fade's length as typed, in seconds: "125" and "125 ms" are milliseconds,
/// "1.5 s" seconds.
pub(crate) fn parse_length(input: &str) -> Result<f64, String> {
    let text = input.trim().to_lowercase();
    let bad = || {
        format!(
            "{:?} is not a length. Type it in ms, like 125, or in seconds, like 1.5 s",
            input.trim()
        )
    };
    let (number, unit) = if let Some(ms) = text.strip_suffix("ms") {
        (ms, 0.001)
    } else if let Some(s) = text.strip_suffix('s') {
        (s, 1.0)
    } else {
        (text.as_str(), 0.001)
    };
    let value = number.trim().parse::<f64>().map_err(|_| bad())?;
    if value.is_finite() && value >= 0.0 {
        Ok(value * unit)
    } else {
        Err(bad())
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Pos2, pos2};
    use lumen_core::time::{PPQ, Ticks};
    use lumen_project::model::{Clip, ClipContent, Id};

    use super::*;
    use crate::audio_editor::source::{Played, WholeFile};

    const BODY: Rect = Rect {
        min: Pos2::new(100.0, 50.0),
        max: Pos2::new(900.0, 350.0),
    };

    fn clip(start: i64, length: i64) -> Clip {
        Clip {
            id: Id(1),
            name: "Loop".into(),
            start: Ticks::quarters(start),
            length: Ticks::quarters(length),
            muted: false,
            color: None,
            locked: false,
            crossfade: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            takes: None,
            warp: None,
            content: ClipContent::Audio {
                media: Id(9),
                offset: 0,
                gain_db: 0.0,
            },
        }
    }

    fn time() -> TimeView {
        TimeView {
            left: 0.0,
            zoom: 50.0,
        }
    }

    fn at(clip: &Clip) -> EditorHandles {
        EditorHandles::of(clip, BODY, time()).expect("an audio clip")
    }

    #[test]
    fn the_grips_are_at_the_clips_real_ends() {
        let h = at(&clip(2, 4));
        assert_eq!(h.start, 200.0);
        assert_eq!(h.end, 400.0);
    }

    #[test]
    fn a_midi_clip_has_no_handles() {
        let midi = Clip {
            content: ClipContent::Midi {
                notes: Default::default(),
                events: Default::default(),
            },
            ..clip(2, 4)
        };
        assert!(EditorHandles::of(&midi, BODY, time()).is_none());
    }

    #[test]
    fn a_grip_is_found_near_its_side_and_a_fade_handle_first_at_the_corner() {
        let h = at(&clip(2, 4));
        let mid = BODY.center().y;
        assert_eq!(h.part_at(pos2(203.0, mid)), Some(Part::Trim(Edge::Start)));
        assert_eq!(h.part_at(pos2(397.0, mid)), Some(Part::Trim(Edge::End)));
        let corner = h.fades.handle_pos(Edge::Start);
        assert_eq!(
            h.part_at(corner),
            Some(Part::Handle(Handle::Fade(Edge::Start)))
        );
    }

    #[test]
    fn the_gain_line_is_found_along_the_clip_and_not_past_it() {
        let h = at(&clip(2, 4));
        let y = h.fades.gain_y();
        assert_eq!(h.part_at(pos2(300.0, y)), Some(Part::Handle(Handle::Gain)));
        assert_eq!(h.part_at(pos2(500.0, y)), None);
    }

    #[test]
    fn a_short_clip_gives_the_nearer_grip_and_no_fades() {
        // 20 points wide: the zones overlap.
        let h = at(&Clip {
            length: Ticks(PPQ * 2 / 5),
            ..clip(2, 1)
        });
        let corner = pos2(h.start + 2.0, h.fades.wave.top());
        assert_eq!(h.part_at(corner), Some(Part::Trim(Edge::Start)));
        let corner = pos2(h.end - 2.0, h.fades.wave.top());
        assert_eq!(h.part_at(corner), Some(Part::Trim(Edge::End)));
    }

    fn file(played: (f64, f64), from: f64, to: f64) -> WholeFile {
        WholeFile {
            spans: Vec::new(),
            from,
            to,
            loop_window: played,
            played: Played {
                first: played,
                second: None,
            },
            heard: played,
        }
    }

    #[test]
    fn a_hit_tells_the_played_part_from_the_rest_of_the_file_and_past_it() {
        let h = at(&clip(4, 4));
        let f = file((4.0, 8.0), 2.0, 12.0);
        let y = BODY.center().y + 40.0;
        let x = |beat: f64| time().x(BODY.left(), beat);
        assert_eq!(h.hit(pos2(x(5.0), y), &f, time()), WaveHit::Played);
        assert_eq!(h.hit(pos2(x(3.0), y), &f, time()), WaveHit::File);
        assert_eq!(h.hit(pos2(x(10.0), y), &f, time()), WaveHit::File);
        assert_eq!(h.hit(pos2(x(13.0), y), &f, time()), WaveHit::Outside);
    }

    #[test]
    fn a_stop_stays_in_view_when_its_handle_is_not() {
        // Starts before the view and ends far past it.
        let view = TimeView {
            left: 8.0,
            zoom: 50.0,
        };
        let h = EditorHandles::of(&clip(2, 400), BODY, view).expect("an audio clip");
        for part in STOPS {
            let rect = h.stop_rect(part);
            assert!(BODY.contains_rect(rect), "{}: {rect:?}", part.key());
            assert!(rect.width() > 0.0 && rect.height() > 0.0);
        }
    }

    #[test]
    fn a_focus_ring_is_whole_with_the_fade_handle_at_the_top_of_the_wave() {
        for clip in [clip(0, 4), clip(2, 400)] {
            let h = at(&clip);
            for part in STOPS {
                let ring = h.stop_rect(part).expand(FOCUS_OUTSET);
                assert!(BODY.contains_rect(ring), "{}: {ring:?}", part.key());
            }
        }
    }

    #[test]
    fn the_stops_run_from_start_to_end() {
        let names: Vec<_> = STOPS.iter().map(|p| p.key()).collect();
        assert_eq!(
            names,
            ["trim start", "fade in", "gain", "fade out", "trim end"]
        );
    }

    #[test]
    fn edge_scroll_is_faster_nearer_the_side_and_still_away_from_it() {
        let lane = Rangef::new(100.0, 900.0);
        let dt = 1.0 / 60.0;
        let mid = 500.0;
        assert_eq!(edge_scroll(500.0, mid, lane, dt), 0.0);
        let slow = edge_scroll(880.0, mid, lane, dt);
        let fast = edge_scroll(899.0, mid, lane, dt);
        assert!(slow < 0.0 && fast < slow, "{slow} {fast}");
        assert!(edge_scroll(105.0, mid, lane, dt) > 0.0, "the left side");
        // Past the side it goes no faster.
        assert_eq!(
            edge_scroll(2_000.0, mid, lane, dt),
            -motion::EDGE_SCROLL_SPEED * dt
        );
    }

    #[test]
    fn a_press_inside_the_zone_scrolls_only_once_the_hand_goes_outwards() {
        let lane = Rangef::new(100.0, 900.0);
        let dt = 1.0 / 60.0;
        // Pressed 10 pt from the right side: holding still, or moving back
        // in, does not scroll.
        assert_eq!(edge_scroll(890.0, 890.0, lane, dt), 0.0);
        assert_eq!(edge_scroll(870.0, 890.0, lane, dt), 0.0);
        // Moving on outwards does, from slowly to full speed at the side.
        let a = edge_scroll(893.0, 890.0, lane, dt);
        let b = edge_scroll(899.0, 890.0, lane, dt);
        assert!(a < 0.0 && b < a, "{a} {b}");
        assert_eq!(
            edge_scroll(950.0, 890.0, lane, dt),
            -motion::EDGE_SCROLL_SPEED * dt
        );
        // The same on the left.
        assert_eq!(edge_scroll(110.0, 110.0, lane, dt), 0.0);
        assert!(edge_scroll(104.0, 110.0, lane, dt) > 0.0);
    }

    #[test]
    fn gains_and_lengths_are_read_as_people_type_them() {
        assert_eq!(parse_db("-3"), Ok(-3.0));
        assert_eq!(parse_db("\u{2212}3.0 dB"), Ok(-3.0));
        assert_eq!(parse_db(" +2.5db "), Ok(2.5));
        assert!(parse_db("loud").is_err());
        assert_eq!(parse_length("125"), Ok(0.125));
        assert_eq!(parse_length("125 ms"), Ok(0.125));
        assert_eq!(parse_length("1.5 s"), Ok(1.5));
        assert!(parse_length("-2").is_err());
        assert!(parse_length("soon").is_err());
    }
}
