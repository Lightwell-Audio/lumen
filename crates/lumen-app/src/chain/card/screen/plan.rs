//! Where the parts of a built-in's card sit at each step. Pure, so the layout
//! is tested without a window.

use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::chain::face::{Drawing, Kind};
use crate::chain::geometry::Step;
use crate::theme::personality::card as c;

/// The most dials the compact card keeps.
const COMPACT_DIALS: usize = 4;
/// A pill on the short card: how far in from the screen's corner, and the gap
/// between pills.
const PILL_INSET: f32 = 8.0;
const PILL_GAP: f32 = 6.0;
/// The compact card's "All n" link, and the room between it and the pill.
const COMPACT_LINK: Vec2 = Vec2::new(44.0, 16.0);
const COMPACT_LINK_GAP: f32 = 6.0;
/// The whole and the short card's link.
const LINK: Vec2 = Vec2::new(60.0, 16.0);

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
            Step::Compact => Rect::from_min_max(
                pos2(card.left() + side, header.bottom() + c::COMPACT_BAND_GAP),
                pos2(card.right() - side, card.bottom() - c::COMPACT_BAND_MARGIN),
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
        }
    }

    /// The subtitle is drawn on the whole and the short card.
    pub(super) fn has_subtitle(&self) -> bool {
        self.step != Step::Compact
    }

    /// The idea names and values are drawn under the dials (not on the compact
    /// card, where the dials sit over the band).
    pub(super) fn has_labels(&self) -> bool {
        self.step != Step::Compact
    }

    /// The names sit on the deep screen (on the short card), not on the card.
    pub(super) fn on_screen(&self) -> bool {
        self.step == Step::Short
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
                    self.screen.center().y
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
                let size = (control.place)(self.card).size();
                Some(Rect::from_min_size(
                    pos2(
                        self.screen.left() + PILL_INSET + before as f32 * (size.x + PILL_GAP),
                        self.screen.top() + PILL_INSET,
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
        }
        // The pill goes on the screen's corner.
        let pill = plan.cell(prism(), 4).expect("the pill");
        assert!(plan.picture.contains_rect(pill));
    }

    #[test]
    fn the_compact_card_keeps_four_dials_over_the_band_and_no_pill() {
        let plan = Plan::new(card(Step::Compact), prism(), Step::Compact);
        assert_eq!(plan.card.height(), 116.0);
        assert_eq!(plan.picture, plan.screen);
        for index in 0..4 {
            let cell = plan.cell(prism(), index).expect("a dial");
            assert!(plan.screen.contains_rect(cell));
        }
        assert_eq!(plan.cell(prism(), 4), None);
        // The header's parts do not overlap.
        assert!(plan.link.right() < plan.preset.left());
        assert!(plan.link.left() > plan.power.x + c::POWER_RADIUS);
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
