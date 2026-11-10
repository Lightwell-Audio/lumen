//! The warp strip (M9.5 S5b, the owner's P A): a thin strip between the audio
//! editor's ruler and its wave where a warped clip's markers are notches
//! (rounded pins pointing at their place) and its transients small triangles. The
//! strip is where markers are grabbed, so they never share the wave with the
//! trim, fade and gain handles. Hovering the strip offers a ghost marker at
//! the nearest transient, or at the pointer when none is near (G B); pressing
//! the ghost makes the marker and moves it in one gesture.
//!
//! Pure: where the flags are, what the pointer is on, where the ghost goes.
//! The edits are `lumen_project::edit::warp_markers`; the gestures are the
//! editor's (`app::audio_editor::warp_markers`).

use eframe::egui::{Pos2, Rect};
use lumen_media::detect::Analysis;
use lumen_project::model::Clip;

use super::source::WholeFile;
use crate::theme;
use crate::timeview::TimeView;

/// The strip's height.
pub(crate) const STRIP: f32 = theme::size::WARP_STRIP;

/// Where the strip is for `clip`: only a warped clip has one.
pub(crate) fn rect_under(ruler: Rect, clip: &Clip) -> Option<Rect> {
    clip.warp.as_ref()?;
    Some(Rect::from_min_max(
        ruler.left_bottom(),
        eframe::egui::pos2(ruler.right(), ruler.bottom() + STRIP),
    ))
}

/// The x of second `seconds` of the file, drawn as the wave is. A second just
/// outside the file (a marker on its last frame) lands on its nearer end.
pub(crate) fn x_of_second(
    file: &WholeFile,
    rate: u32,
    time: TimeView,
    left: f32,
    seconds: f64,
) -> Option<f32> {
    let frame = seconds * f64::from(rate);
    let beat = file.beat_of_frame(frame).or_else(|| {
        let first = file.spans.first()?;
        Some(if frame < first.first_frame {
            file.from
        } else {
            file.to
        })
    })?;
    Some(time.x(left, beat))
}

/// The markers of a clip on screen.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WarpStrip {
    pub rect: Rect,
    /// Each marker's x, in time order (the marker's index is its place here).
    pub flags: Vec<f32>,
}

impl WarpStrip {
    /// The strip of `clip`, whose file is laid as `file`, in `rect`.
    pub(crate) fn of(
        clip: &Clip,
        file: &WholeFile,
        (rate, time): (u32, TimeView),
        rect: Rect,
    ) -> Self {
        let flags = clip.warp.as_ref().map_or_else(Vec::new, |warp| {
            warp.markers
                .iter()
                .map(|m| {
                    x_of_second(file, rate, time, rect.left(), m.source).unwrap_or(rect.left())
                })
                .collect()
        });
        Self { rect, flags }
    }

    /// The marker under `p`: the nearest within reach, when `p` is in the
    /// strip.
    pub(crate) fn hit(&self, p: Pos2) -> Option<usize> {
        if !self.rect.contains(p) {
            return None;
        }
        self.flags
            .iter()
            .enumerate()
            .map(|(i, x)| (i, (x - p.x).abs()))
            .filter(|(_, d)| *d <= theme::size::WARP_GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Where marker `index`'s focus ring goes.
    pub(crate) fn flag_rect(&self, index: usize) -> Option<Rect> {
        let x = *self.flags.get(index)?;
        let reach = theme::size::WARP_GRAB;
        Some(
            Rect::from_x_y_ranges(x - reach..=x + reach, self.rect.y_range())
                .intersect(self.rect.expand2(eframe::egui::vec2(reach, 0.0))),
        )
    }
}

/// The marker the pointer would make: a transient near it, or the pointer's
/// own place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ghost {
    pub x: f32,
    /// The second of the file it would pin.
    pub source: f64,
    pub on_transient: bool,
    /// The transient's strength, 0 to 1 (0 off a transient).
    pub strength: f32,
}

/// The ghost for a pointer at `x`, who is over the file; `found` is what
/// detection offers and the sensitivity it is read at.
pub(crate) fn ghost(
    file: &WholeFile,
    (rate, time): (u32, TimeView),
    left: f32,
    x: f32,
    found: Option<(&Analysis, u8)>,
) -> Option<Ghost> {
    let beat = time.beat_at(left, x);
    let span = file.span_at(beat)?;
    let seconds = file.frame_at(beat)? / f64::from(rate.max(1));
    if let Some((analysis, sensitivity)) = found {
        // The reach in points, as seconds of the file under the pointer.
        let per_beat = span.frames_per_beat / f64::from(rate.max(1));
        let within = f64::from(theme::size::WARP_NEAR) / time.zoom.max(1e-9) * per_beat;
        if let Some(t) = analysis.nearest(seconds, sensitivity, within)
            && let Some(at) = x_of_second(file, rate, time, left, t.seconds)
        {
            return Some(Ghost {
                x: at,
                source: t.seconds,
                on_transient: true,
                strength: t.strength,
            });
        }
    }
    Some(Ghost {
        x,
        source: seconds,
        on_transient: false,
        strength: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrangement::waves::Span;
    use crate::audio_editor::source::Played;
    use eframe::egui::pos2;
    use lumen_core::time::Ticks;
    use lumen_media::detect::Transient;
    use lumen_project::model::Warp;

    const RATE: u32 = 48_000;

    /// A file of 32 beats at 10 points a beat from beat 0, 24,000 frames a
    /// beat (120 BPM).
    fn file() -> WholeFile {
        WholeFile {
            spans: vec![Span {
                media: 1,
                start: 0.0,
                end: 32.0,
                first_frame: 0.0,
                frames_per_beat: 24_000.0,
                gain: 1.0,
            }],
            from: 0.0,
            to: 32.0,
            loop_window: (0.0, 32.0),
            played: Played {
                first: (0.0, 32.0),
                second: None,
            },
            heard: (0.0, 32.0),
        }
    }

    fn time() -> TimeView {
        TimeView {
            left: 0.0,
            zoom: 10.0,
        }
    }

    fn strip_rect() -> Rect {
        Rect::from_min_max(pos2(0.0, 37.0), pos2(1_000.0, 37.0 + STRIP))
    }

    fn clip(markers: &[(f64, i64)]) -> Clip {
        use lumen_project::model::{ClipContent, Id};
        let mut clip = Clip {
            id: Id(1),
            name: "Loop".into(),
            start: Ticks::ZERO,
            length: Ticks::quarters(32),
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
        };
        let mut warp = Warp::new(120.0, 0.0);
        warp.markers = markers
            .iter()
            .map(|(s, q)| lumen_core::warp::Marker::new(*s, Ticks::quarters(*q)))
            .collect();
        clip.warp = Some(warp);
        clip
    }

    fn found(seconds: &[f64]) -> Analysis {
        Analysis {
            seconds: 16.0,
            transients: seconds
                .iter()
                .map(|s| Transient {
                    seconds: *s,
                    strength: 0.8,
                })
                .collect(),
            tempo: None,
        }
    }

    #[test]
    fn only_a_warped_clip_has_a_strip_under_the_ruler() {
        let ruler = Rect::from_min_max(pos2(0.0, 0.0), pos2(1_000.0, 37.0));
        let mut plain = clip(&[]);
        plain.warp = None;
        assert_eq!(rect_under(ruler, &plain), None);
        assert_eq!(rect_under(ruler, &clip(&[(0.0, 0)])), Some(strip_rect()));
    }

    #[test]
    fn flags_stand_where_the_wave_draws_their_second() {
        let strip = WarpStrip::of(
            &clip(&[(0.0, 0), (1.0, 2), (2.0, 4)]),
            &file(),
            (RATE, time()),
            strip_rect(),
        );
        // Two beats to the second at 120 BPM, ten points a beat.
        assert_eq!(strip.flags, vec![0.0, 20.0, 40.0]);
        // The marker nearest wins within reach; none outside it.
        assert_eq!(strip.hit(pos2(22.0, 44.0)), Some(1));
        assert_eq!(strip.hit(pos2(30.0, 44.0)), None);
        assert_eq!(strip.hit(pos2(20.0, 30.0)), None, "the ruler above");
    }

    #[test]
    fn a_marker_on_the_files_last_frame_lands_at_its_end() {
        let end = 16.0;
        let x = x_of_second(&file(), RATE, time(), 0.0, end).unwrap();
        assert_eq!(x, 320.0);
        assert_eq!(x_of_second(&file(), RATE, time(), 0.0, 40.0), Some(320.0));
        assert_eq!(x_of_second(&file(), RATE, time(), 0.0, -3.0), Some(0.0));
    }

    #[test]
    fn the_ghost_lands_on_a_near_transient_or_follows_the_pointer() {
        let transients = found(&[1.0, 5.0]);
        // A transient at second 1.0 is x 20; the pointer at 27 is 7 points
        // away, within reach.
        let near = ghost(&file(), (RATE, time()), 0.0, 27.0, Some((&transients, 100))).unwrap();
        assert!(near.on_transient);
        assert_eq!((near.x, near.source), (20.0, 1.0));
        // Too far: the ghost is at the pointer, on the second it is over.
        let far = ghost(&file(), (RATE, time()), 0.0, 60.0, Some((&transients, 100))).unwrap();
        assert!(!far.on_transient);
        assert_eq!(far.x, 60.0);
        assert!((far.source - 3.0).abs() < 1e-9);
        // Nothing detected yet: the pointer.
        let none = ghost(&file(), (RATE, time()), 0.0, 60.0, None).unwrap();
        assert!(!none.on_transient);
        // Past the file there is nothing to pin.
        assert!(ghost(&file(), (RATE, time()), 0.0, 400.0, None).is_none());
        // A low sensitivity does not offer a weak transient.
        let weak = Analysis {
            transients: vec![Transient {
                seconds: 1.0,
                strength: 0.2,
            }],
            ..found(&[])
        };
        let off = ghost(&file(), (RATE, time()), 0.0, 22.0, Some((&weak, 0))).unwrap();
        assert!(!off.on_transient);
    }
}
