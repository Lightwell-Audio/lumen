//! Where the parts of a built-in's card sit at each step. Pure, so the layout
//! is tested without a window.

use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::chain::face::{Drawing, Kind, Parts};
use crate::chain::geometry::Step;
use crate::theme;
use crate::theme::personality::card as c;

/// The most dials the compact card keeps.
const COMPACT_DIALS: usize = 4;
/// The gap between pills on the short card.
const PILL_GAP: f32 = 6.0;
/// What a dial's name leaves of its column's pitch on the short and compact
/// cards, and the whole card's room for a name.
const LABEL_GAP: f32 = 2.0;
const WHOLE_LABEL_ROOM: f32 = 56.0;
/// The compact card's "All n" link, and the room between it and the pill.
const COMPACT_LINK: Vec2 = Vec2::new(44.0, 16.0);
const COMPACT_LINK_GAP: f32 = 6.0;
/// What is left of that link when the name needs its room: the chevron.
pub(super) const COMPACT_LINK_NARROW: Vec2 = Vec2::new(18.0, 16.0);
/// The whole and the short card's link.
const LINK: Vec2 = Vec2::new(60.0, 16.0);
/// The short card's row of part pills: the gap between them, how small and
/// how big one is.
const SHORT_PILL_GAP: f32 = 2.0;
const SHORT_PILL_LEAST: f32 = 10.0;
const SHORT_PILL_MOST: f32 = 18.0;

/// Where a card's parts are for one step.
#[derive(Debug, Clone, Copy)]
pub(super) struct Plan {
    pub step: Step,
    pub card: Rect,
    pub header: Rect,
    /// The deep screen, and what of it the drawing has.
    pub screen: Rect,
    pub picture: Rect,
    pub power: Pos2,
    pub name_size: f32,
    pub name_baseline: f32,
    pub preset: Rect,
    pub link: Rect,
    pub dial_radius: f32,
    pub labels: Labels,
}

/// How the names and values under the dials are set at one step: their sizes,
/// their baselines under a dial's foot and the room each may take.
#[derive(Debug, Clone, Copy)]
pub(super) struct Labels {
    pub name: f32,
    pub value: f32,
    pub name_under: f32,
    pub value_under: f32,
    pub room: f32,
}

impl Plan {
    pub(super) fn new(card: Rect, drawing: &Drawing, step: Step) -> Self {
        let compact = step == Step::Compact;
        let head = if compact {
            c::COMPACT_HEADER
        } else {
            c::HEADER_HEIGHT
        };
        let header = Rect::from_min_size(card.min, vec2(card.width(), head));
        let side = c::SCREEN_INSET;
        let screen = match step {
            Step::Whole => Rect::from_min_size(
                pos2(card.left() + side, card.top() + c::SCREEN_TOP),
                vec2(card.width() - 2.0 * side, drawing.screen),
            ),
            Step::Short => Rect::from_min_max(
                pos2(card.left() + side, card.top() + c::SCREEN_TOP),
                pos2(card.right() - side, card.bottom() - c::SHORT_BOTTOM),
            ),
            // A thin band of the picture under the header; the dials and their
            // names are under it, never over it.
            Step::Compact => Rect::from_min_size(
                pos2(card.left() + side, header.bottom() + c::COMPACT_BAND_GAP),
                vec2(card.width() - 2.0 * side, c::COMPACT_BAND_HEIGHT),
            ),
        };
        let picture = if step == Step::Short {
            Rect::from_min_max(
                screen.min,
                pos2(screen.right(), screen.bottom() - c::SHORT_DIALS_BAND),
            )
        } else {
            screen
        };
        let (preset_size, preset_top) = if compact {
            (c::COMPACT_PRESET, c::COMPACT_PRESET_TOP)
        } else if step == Step::Short {
            (c::PRESET_SIZE_PT, c::SHORT_PRESET_TOP)
        } else {
            (c::PRESET_SIZE_PT, c::PRESET_TOP)
        };
        let preset = Rect::from_min_size(
            pos2(
                card.right() - c::PRESET_INSET - preset_size.0,
                card.top() + preset_top,
            ),
            vec2(preset_size.0, preset_size.1),
        );
        let link = if compact {
            Rect::from_min_size(
                pos2(
                    preset.left() - COMPACT_LINK_GAP - COMPACT_LINK.x,
                    preset.center().y - COMPACT_LINK.y / 2.0,
                ),
                COMPACT_LINK,
            )
        } else {
            Rect::from_min_size(
                pos2(
                    card.right() - c::ALL_INSET - LINK.x,
                    card.bottom() - c::ALL_FROM_BOTTOM - LINK.y / 2.0,
                ),
                LINK,
            )
        };
        Self {
            step,
            card,
            header,
            screen,
            picture,
            power: pos2(
                card.left() + c::POWER_X,
                card.top()
                    + if compact {
                        c::COMPACT_POWER_Y
                    } else {
                        c::POWER_Y
                    },
            ),
            name_size: if compact {
                c::COMPACT_NAME_SIZE
            } else {
                c::NAME_SIZE
            },
            name_baseline: if compact {
                c::COMPACT_NAME_BASELINE
            } else {
                c::NAME_BASELINE
            },
            preset,
            link,
            dial_radius: if compact {
                c::COMPACT_DIAL_RADIUS
            } else {
                c::DIAL_RADIUS
            },
            labels: {
                let dials = drawing.dials().count().max(1);
                let dials = if compact {
                    dials.min(COMPACT_DIALS)
                } else {
                    dials
                };
                let pitch = screen.width() / dials as f32 - LABEL_GAP;
                match step {
                    Step::Whole => Labels {
                        name: c::DIAL_NAME_SIZE,
                        value: c::DIAL_VALUE_SIZE,
                        name_under: c::DIAL_NAME_BASELINE,
                        value_under: c::DIAL_VALUE_BASELINE,
                        room: WHOLE_LABEL_ROOM,
                    },
                    Step::Short => Labels {
                        name: c::SHORT_NAME_SIZE,
                        value: c::SHORT_VALUE_SIZE,
                        name_under: c::SHORT_NAME_BASELINE,
                        value_under: c::SHORT_VALUE_BASELINE,
                        room: pitch,
                    },
                    Step::Compact => Labels {
                        name: c::COMPACT_NAME_LABEL,
                        value: c::COMPACT_VALUE_LABEL,
                        name_under: c::COMPACT_NAME_BASELINE_UNDER,
                        value_under: c::COMPACT_VALUE_BASELINE_UNDER,
                        room: pitch,
                    },
                }
            },
        }
    }

    /// The compact header with a name `name_width` wide: a set piece's name is
    /// never cut at its normal sizes, so when it does not fit left of the full
    /// "All n" link the link gives way to its chevron (the count is in its tooltip
    /// and its accessible name).
    pub(super) fn for_name(mut self, name_width: f32) -> Self {
        if self.step != Step::Compact {
            return self;
        }
        let start = self.card.left() + c::NAME_X;
        if start + name_width + theme::space::S > self.link.left() {
            let right = self.link.right();
            self.link = Rect::from_min_size(
                pos2(right - COMPACT_LINK_NARROW.x, self.link.top()),
                COMPACT_LINK_NARROW,
            );
        }
        self
    }

    /// The subtitle is drawn on the whole and the short card.
    pub(super) fn has_subtitle(&self) -> bool {
        self.step != Step::Compact
    }

    /// The idea names and values are drawn under the dials at every step.
    pub(super) fn has_labels(&self) -> bool {
        true
    }

    /// The names sit on the deep screen (on the short card), not on the card.
    pub(super) fn on_screen(&self) -> bool {
        self.step == Step::Short
    }

    /// The pill of part `index` of a drawing with parts: the specimen's place on
    /// the whole card, a row of small ones in the strip under the short card's
    /// screen, none on the compact card (a point on its band picks).
    pub(super) fn pill_cell(&self, parts: &Parts, index: usize) -> Option<Rect> {
        match self.step {
            Step::Whole => Some((parts.pill)(self.card, index)),
            Step::Short => {
                let pitch = (self.link.left() - PILL_GAP - self.screen.left()) / parts.count as f32;
                let size = (pitch - SHORT_PILL_GAP).clamp(SHORT_PILL_LEAST, SHORT_PILL_MOST);
                Some(Rect::from_min_size(
                    pos2(
                        self.screen.left() + index as f32 * pitch,
                        self.card.bottom() - c::ALL_FROM_BOTTOM - size / 2.0,
                    ),
                    Vec2::splat(size),
                ))
            }
            Step::Compact => None,
        }
    }

    /// The cell of control `index` of the drawing at this step, or `None` when
    /// the step leaves it out (the compact card keeps four dials, no pills).
    pub(super) fn cell(&self, drawing: &Drawing, index: usize) -> Option<Rect> {
        let control = drawing.controls.get(index)?;
        if self.step == Step::Whole {
            return Some((control.place)(self.card));
        }
        let before = drawing.controls[..index]
            .iter()
            .filter(|other| other.kind == control.kind)
            .count();
        match control.kind {
            Kind::Knob => {
                let count = drawing.dials().count();
                let count = if self.step == Step::Compact {
                    count.min(COMPACT_DIALS)
                } else {
                    count
                };
                if before >= count {
                    return None;
                }
                let x =
                    self.screen.left() + self.screen.width() * (before as f32 + 0.5) / count as f32;
                let y = if self.step == Step::Short {
                    self.screen.bottom() - c::SHORT_DIAL_FROM_FOOT
                } else {
                    // Under the band, with room below for a name and a value.
                    self.screen.bottom() + c::COMPACT_DIAL_GAP + self.dial_radius
                };
                Some(Rect::from_center_size(
                    pos2(x, y),
                    Vec2::splat(2.0 * self.dial_radius + 4.0),
                ))
            }
            Kind::Pill => {
                if self.step != Step::Short {
                    return None;
                }
                // Pills sit in the strip under the screen, at the left, level
                // with the "All n" link at the right: nothing on the picture.
                let size = (control.place)(self.card).size();
                Some(Rect::from_min_size(
                    pos2(
                        self.screen.left() + before as f32 * (size.x + PILL_GAP),
                        self.card.bottom() - c::ALL_FROM_BOTTOM - size.y / 2.0,
                    ),
                    size,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::face::drawing_of;

    fn prism() -> &'static Drawing {
        drawing_of(lumen_devices::utility::NAME).expect("Prism has a drawing")
    }

    fn card(step: Step) -> Rect {
        Rect::from_min_size(pos2(10.0, 20.0), step.size(prism().card))
    }

    #[test]
    fn the_whole_card_is_the_specimens() {
        let plan = Plan::new(card(Step::Whole), prism(), Step::Whole);
        assert_eq!(plan.screen.size(), vec2(272.0, 136.0));
        assert_eq!(plan.screen.min - plan.card.min, vec2(14.0, 60.0));
        // The Width dial is the third, at x 156 and 40 under the screen's foot.
        let cell = plan.cell(prism(), 2).expect("a cell");
        assert_eq!(cell.center() - plan.card.min, vec2(156.0, 236.0));
        // The pill is on the card, right of the dials.
        let pill = plan.cell(prism(), 4).expect("a cell");
        assert!(pill.left() > cell.right());
    }

    #[test]
    fn the_short_card_puts_its_dials_on_the_screens_lower_edge() {
        let plan = Plan::new(card(Step::Short), prism(), Step::Short);
        assert!(plan.card.height() < 280.0);
        assert!(plan.picture.bottom() < plan.screen.bottom());
        let mut last_right = f32::NEG_INFINITY;
        for index in 0..4 {
            let cell = plan.cell(prism(), index).expect("a dial");
            assert!(
                plan.screen.contains_rect(cell),
                "dial {index} is on the screen"
            );
            assert!(
                cell.top() >= plan.picture.bottom() - 1.0,
                "under the picture"
            );
            // Even spacing: each column is one pitch wide, and a name has
            // that pitch to itself.
            assert!(cell.left() >= last_right - 1.0, "dial {index} is clear");
            last_right = cell.right();
        }
        assert!(plan.labels.room <= plan.screen.width() / 4.0);
        // The pill is on the card, in the strip under the screen: nothing of the
        // picture, the dials or their names is under it.
        let pill = plan.cell(prism(), 4).expect("the pill");
        assert!(plan.card.contains_rect(pill));
        assert!(pill.top() >= plan.screen.bottom(), "under the screen");
        assert!(pill.right() < plan.link.left(), "left of the link");
        // The Preset pill is on the name's line, above the subtitle.
        assert!(
            plan.preset.bottom() - plan.card.top() < c::SUBTITLE_BASELINE - c::SUBTITLE_SIZE * 0.7
        );
    }

    #[test]
    fn the_compact_card_puts_four_named_dials_under_a_thin_band() {
        let plan = Plan::new(card(Step::Compact), prism(), Step::Compact);
        assert_eq!(plan.card.height(), 116.0);
        assert_eq!(plan.picture, plan.screen);
        assert!(plan.screen.bottom() <= plan.card.top() + 64.0);
        let mut last_right = f32::NEG_INFINITY;
        for index in 0..4 {
            let cell = plan.cell(prism(), index).expect("a dial");
            // Under the band: no dial covers the glass or the beam's start.
            assert!(
                cell.top() >= plan.screen.bottom(),
                "dial {index} is under the band"
            );
            assert!(plan.card.contains_rect(cell));
            assert!(cell.left() >= last_right - 1.0, "dial {index} is clear");
            last_right = cell.right();
            // Its name and value fit between its foot and the card's edge.
            let foot = cell.center().y + plan.dial_radius;
            assert!(
                foot + plan.labels.value_under + 2.0 <= plan.card.bottom(),
                "dial {index}'s value is on the card"
            );
        }
        assert!(plan.has_labels());
        assert_eq!(plan.cell(prism(), 4), None);
        // The header's parts do not overlap.
        assert!(plan.link.right() < plan.preset.left());
        assert!(plan.link.left() > plan.power.x + c::POWER_RADIUS);
    }

    #[test]
    fn a_long_name_takes_the_compact_links_room_and_a_short_one_leaves_it() {
        let plan = Plan::new(card(Step::Compact), prism(), Step::Compact);
        let short = plan.for_name(40.0);
        assert_eq!(short.link, plan.link, "a short name leaves the full link");
        // A name too wide for the room left of the full link: only the chevron
        // stays, at the same right edge, clear of the preset pill.
        let long = plan.for_name(100.0);
        assert!(long.link.width() < plan.link.width());
        assert_eq!(long.link.right(), plan.link.right());
        assert!(long.link.right() < long.preset.left());
        assert!(long.link.left() > long.card.left() + c::NAME_X + 100.0);
        // The other steps keep their link whatever the name.
        for step in [Step::Whole, Step::Short] {
            let plan = Plan::new(card(step), prism(), step);
            assert_eq!(plan.for_name(500.0).link, plan.link);
        }
    }

    #[test]
    fn no_cell_leaves_its_card() {
        for step in [Step::Whole, Step::Short, Step::Compact] {
            let plan = Plan::new(card(step), prism(), step);
            for index in 0..prism().controls.len() {
                if let Some(cell) = plan.cell(prism(), index) {
                    assert!(plan.card.contains_rect(cell), "{step:?} control {index}");
                }
            }
            assert!(plan.card.contains_rect(plan.link), "{step:?} link");
            assert!(plan.card.contains_rect(plan.preset), "{step:?} preset");
        }
    }
}
