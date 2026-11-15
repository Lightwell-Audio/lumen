use eframe::egui::{Pos2, Rect};
use lumen_media::detect::Analysis;
use lumen_project::model::Clip;

use super::source::WholeFile;
use crate::theme;
use crate::timeview::TimeView;

pub(crate) const STRIP: f32 = theme::size::WARP_STRIP;

pub(crate) fn rect_under(ruler: Rect, clip: &Clip) -> Option<Rect> {
    clip.warp.as_ref()?;
    Some(Rect::from_min_max(
        ruler.left_bottom(),
        eframe::egui::pos2(ruler.right(), ruler.bottom() + STRIP),
    ))
}

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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WarpStrip {
    pub rect: Rect,
    pub flags: Vec<f32>,
}

impl WarpStrip {
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

    pub(crate) fn notch_rect(&self, index: usize) -> Option<Rect> {
        let x = *self.flags.get(index)?;
        let (w, h) = (theme::size::WARP_NOTCH_W, theme::size::WARP_NOTCH_H);
        let top = self.rect.top() + theme::size::WARP_NOTCH_INSET;
        Some(Rect::from_x_y_ranges(
            x - w / 2.0..=x + w / 2.0,
            top..=top + h,
        ))
    }

    pub(crate) fn flag_rect(&self, index: usize) -> Option<Rect> {
        let x = *self.flags.get(index)?;
        let reach = theme::size::WARP_GRAB;
        Some(
            Rect::from_x_y_ranges(x - reach..=x + reach, self.rect.y_range())
                .intersect(self.rect.expand2(eframe::egui::vec2(reach, 0.0))),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ghost {
    pub x: f32,
    pub source: f64,
    pub on_transient: bool,
    pub strength: f32,
}

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
        assert_eq!(strip.flags, vec![0.0, 20.0, 40.0]);
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
        let near = ghost(&file(), (RATE, time()), 0.0, 27.0, Some((&transients, 100))).unwrap();
        assert!(near.on_transient);
        assert_eq!((near.x, near.source), (20.0, 1.0));
        let far = ghost(&file(), (RATE, time()), 0.0, 60.0, Some((&transients, 100))).unwrap();
        assert!(!far.on_transient);
        assert_eq!(far.x, 60.0);
        assert!((far.source - 3.0).abs() < 1e-9);
        let none = ghost(&file(), (RATE, time()), 0.0, 60.0, None).unwrap();
        assert!(!none.on_transient);
        assert!(ghost(&file(), (RATE, time()), 0.0, 400.0, None).is_none());
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
