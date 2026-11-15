use eframe::egui::{Rect, Vec2, pos2, vec2};

pub(super) const CARD_W: f32 = 232.0;
pub(super) const CARD_H: f32 = 344.0;
pub(super) const BAND: f32 = 72.0;
pub(super) const HEADER: f32 = 48.0;
pub(super) const FOOTER: f32 = 32.0;
pub(super) const PRESET: f32 = 34.0;
pub(super) const BODY: f32 = 158.0 + PRESET;
pub(super) const KEY_ROOM: f32 = 28.0;
pub(super) const CABLE: f32 = 40.0;
pub(super) const CHIP_W: f32 = 112.0;
pub(super) const CHIP_H: f32 = 28.0;
pub(super) const PAD: f32 = 16.0;
pub(super) const PORT: f32 = 4.0;

pub(crate) fn panel_least(card: f32) -> f32 {
    card + 2.0 * PAD + 2.0 * crate::theme::space::S + crate::DETAIL_CHROME
}

#[cfg(test)]
fn panel_min_height() -> f32 {
    panel_least(COMPACT_H)
}

pub(super) const COMPACT_H: f32 = 116.0;
pub(super) const COMPACT_HEADER: f32 = 36.0;
pub(super) const COMPACT_FOOTER: f32 = 24.0;
const HYSTERESIS: f32 = 8.0;

pub(super) fn compact(was: bool, tray_height: f32, extra: f32) -> bool {
    let room = tray_height - 2.0 * PAD;
    let switch = HEADER + BODY + FOOTER + extra;
    if was {
        room < switch + HYSTERESIS
    } else {
        room < switch - HYSTERESIS
    }
}

#[cfg(test)]
pub(super) fn card_height(room: f32) -> f32 {
    card_height_with(room, 0.0)
}

pub(super) fn card_height_with(room: f32, extra: f32) -> f32 {
    room.clamp(HEADER + BODY + FOOTER + extra, CARD_H)
}

const BAND_LEAST: f32 = 24.0;

pub(super) fn band(height: f32) -> f32 {
    let room = (height - HEADER - BODY - FOOTER).clamp(0.0, BAND);
    if room < BAND_LEAST { 0.0 } else { room }
}

pub(super) const RAIL_W: f32 = 48.0;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Layout {
    pub cards: usize,
    pub card_h: f32,
    pub cable_y: f32,
    top: f32,
    spans: Vec<(f32, f32, f32)>,
    end: f32,
}

impl Layout {
    #[cfg(test)]
    pub fn new(folded: &[bool], tray_height: f32) -> Self {
        Self::keyed(folded, tray_height, 0.0)
    }

    pub fn keyed(folded: &[bool], tray_height: f32, extra: f32) -> Self {
        let card_h = card_height_with(tray_height - 2.0 * PAD, extra);
        Self::build(
            folded,
            tray_height,
            &vec![card_h; folded.len()],
            &[],
            card_h,
        )
    }

    pub fn sized(folded: &[bool], tray_height: f32, heights: &[f32]) -> Self {
        Self::build(folded, tray_height, heights, &[], COMPACT_H)
    }

    pub fn mixed(folded: &[bool], tray_height: f32, sizes: &[Vec2]) -> Self {
        let heights: Vec<f32> = sizes.iter().map(|s| s.y).collect();
        let widths: Vec<f32> = sizes.iter().map(|s| s.x).collect();
        Self::build(folded, tray_height, &heights, &widths, COMPACT_H)
    }

    fn build(
        folded: &[bool],
        tray_height: f32,
        heights: &[f32],
        widths: &[f32],
        empty: f32,
    ) -> Self {
        let card_h = heights.iter().copied().fold(0.0, f32::max);
        let card_h = if heights.is_empty() { empty } else { card_h };
        let least = heights.iter().copied().fold(card_h, f32::min);
        let top = ((tray_height - card_h) / 2.0).max(PAD.min(tray_height / 2.0));
        let mut x = PAD + CHIP_W + CABLE;
        let mut spans = Vec::with_capacity(folded.len());
        for (i, (&fold, &h)) in folded.iter().zip(heights).enumerate() {
            let wide = widths.get(i).copied().unwrap_or(CARD_W);
            let w = if fold { RAIL_W } else { wide };
            spans.push((x, w, h));
            x += w + CABLE;
        }
        Self {
            cards: folded.len(),
            card_h,
            cable_y: top + least * 0.54,
            top,
            spans,
            end: x,
        }
    }

    pub fn card(&self, i: usize) -> Rect {
        let (x, w, h) = self.spans[i];
        Rect::from_min_size(pos2(x, self.top), vec2(w, h))
    }

    pub fn input(&self) -> Rect {
        Rect::from_min_size(pos2(PAD, self.cable_y - CHIP_H / 2.0), vec2(CHIP_W, CHIP_H))
    }

    pub fn output(&self) -> Rect {
        Rect::from_min_size(
            pos2(self.end, self.cable_y - CHIP_H / 2.0),
            vec2(CHIP_W, CHIP_H),
        )
    }

    pub fn width(&self) -> f32 {
        self.output().right() + PAD
    }

    pub fn cables(&self) -> Vec<(f32, f32)> {
        let mut ends = vec![(self.input().right(), 0.0)];
        for i in 0..self.cards {
            let card = self.card(i);
            ends.last_mut().expect("the input is first").1 = card.left();
            ends.push((card.right(), 0.0));
        }
        ends.last_mut().expect("the input is first").1 = self.output().left();
        ends
    }

    pub fn gap_at(&self, x: f32) -> usize {
        (0..self.cards)
            .find(|&i| x < self.card(i).center().x)
            .unwrap_or(self.cards)
    }

    pub fn gap_x(&self, i: usize) -> f32 {
        if i == 0 {
            self.input().right() + CABLE / 2.0
        } else {
            self.card(i - 1).right() + CABLE / 2.0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Whole,
    Short,
    Compact,
}

pub(super) const SHORT_H: f32 = 272.0;

impl Step {
    pub(super) fn of(whole: Vec2, tray_height: f32, was: Option<Step>) -> Self {
        let room = tray_height - 2.0 * PAD;
        let fits = |height: f32, here: bool| {
            room >= if here {
                height - HYSTERESIS
            } else {
                height + HYSTERESIS
            }
        };
        if whole.y > SHORT_H && fits(whole.y, was == Some(Step::Whole)) {
            Step::Whole
        } else if fits(SHORT_H, was == Some(Step::Short)) {
            Step::Short
        } else {
            Step::Compact
        }
    }

    pub(super) fn size(self, whole: Vec2) -> Vec2 {
        match self {
            Step::Whole => whole,
            Step::Short => vec2(CARD_W, SHORT_H),
            Step::Compact => vec2(CARD_W, COMPACT_H),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_pieces_step_follows_the_tray_with_give_either_way() {
        let whole = vec2(300.0, 290.0);
        let tray = |room: f32| room + 2.0 * PAD;
        assert_eq!(Step::of(whole, tray(290.0 + HYSTERESIS), None), Step::Whole);
        assert_eq!(
            Step::of(whole, tray(289.0 + HYSTERESIS - 1.0), None),
            Step::Short
        );
        assert_eq!(
            Step::of(whole, tray(SHORT_H + HYSTERESIS), None),
            Step::Short
        );
        assert_eq!(Step::of(whole, tray(SHORT_H), None), Step::Compact);
        assert_eq!(
            Step::of(whole, tray(290.0 - HYSTERESIS), Some(Step::Whole)),
            Step::Whole
        );
        assert_eq!(
            Step::of(whole, tray(290.0 + 1.0), Some(Step::Short)),
            Step::Short
        );
        assert_eq!(
            Step::of(whole, tray(SHORT_H - HYSTERESIS), Some(Step::Short)),
            Step::Short
        );
        assert_eq!(Step::of(vec2(300.0, 250.0), tray(500.0), None), Step::Short);
    }

    #[test]
    fn a_steps_size_is_the_devices_own_then_the_chains() {
        let whole = vec2(300.0, 290.0);
        assert_eq!(Step::Whole.size(whole), whole);
        assert_eq!(Step::Short.size(whole), vec2(CARD_W, SHORT_H));
        assert_eq!(Step::Compact.size(whole), vec2(CARD_W, COMPACT_H));
    }

    #[test]
    fn a_sliver_of_picture_is_not_drawn() {
        let least = HEADER + BODY + FOOTER;
        assert_eq!(band(least), 0.0);
        assert_eq!(band(least + BAND_LEAST - 1.0), 0.0);
        assert_eq!(band(least + BAND_LEAST), BAND_LEAST);
        assert_eq!(band(CARD_H), BAND);
    }

    #[test]
    fn at_the_shortest_panel_a_compact_card_sits_whole_inside_the_tray() {
        let space = crate::theme::space::S;
        assert_eq!(panel_min_height(), 212.0);
        let tray = panel_min_height() - crate::DETAIL_CHROME - 2.0 * space;
        assert!(compact(false, tray, 0.0) && compact(false, tray, KEY_ROOM));
        let layout = Layout::sized(&[false], tray, &[COMPACT_H]);
        let card = layout.card(0);
        assert!(card.top() >= 1.0, "{card:?}");
        assert!(card.bottom() <= tray - 1.0, "{card:?} in {tray}");
        assert!(panel_least(COMPACT_H + 28.0) > panel_min_height());
    }

    #[test]
    fn the_chain_turns_compact_past_the_switch_and_back_past_it_the_other_way() {
        let switch = HEADER + BODY + FOOTER + 2.0 * PAD;
        assert!(!compact(false, switch - 7.0, 0.0));
        assert!(compact(false, switch - 9.0, 0.0));
        assert!(compact(true, switch + 7.0, 0.0));
        assert!(!compact(true, switch + 9.0, 0.0));
        assert!(compact(false, switch + KEY_ROOM - 9.0, KEY_ROOM));
        let tray = switch - 7.0;
        let card = Layout::new(&[false], tray).card(0);
        assert!(card.bottom() <= tray, "{card:?} in {tray}");
    }

    #[test]
    fn a_taller_compact_card_grows_down_and_the_cable_still_crosses_every_card() {
        let tall = COMPACT_H + 28.0;
        let tray = panel_least(tall) - crate::DETAIL_CHROME - 2.0 * crate::theme::space::S;
        let layout = Layout::sized(&[false, false], tray, &[COMPACT_H, tall]);
        let (a, b) = (layout.card(0), layout.card(1));
        assert_eq!(a.top(), b.top());
        assert_eq!((a.height(), b.height()), (COMPACT_H, tall));
        assert!(b.bottom() <= tray - 1.0, "{b:?} in {tray}");
        assert!(layout.cable_y > a.top() && layout.cable_y < a.bottom());
    }

    #[test]
    fn the_cards_are_spaced_by_a_cable_and_the_output_follows_the_last() {
        let layout = Layout::new(&[false; 3], 330.0);
        assert_eq!(layout.card(0).left(), PAD + CHIP_W + CABLE);
        assert_eq!(layout.card(1).left() - layout.card(0).right(), CABLE);
        assert_eq!(layout.output().left() - layout.card(2).right(), CABLE);
        assert_eq!(layout.width(), layout.output().right() + PAD);
    }

    #[test]
    fn an_empty_chain_is_two_chips_and_one_cable() {
        let layout = Layout::new(&[], 330.0);
        let cables = layout.cables();
        assert_eq!(cables.len(), 1);
        assert_eq!(cables[0].0, layout.input().right());
        assert_eq!(cables[0].1, layout.output().left());
        assert_eq!(layout.output().left() - layout.input().right(), CABLE);
    }

    #[test]
    fn a_cable_joins_every_pair() {
        let layout = Layout::new(&[false; 2], 330.0);
        let cables = layout.cables();
        assert_eq!(cables.len(), 3);
        for (a, b) in &cables {
            assert_eq!(b - a, CABLE);
        }
    }

    #[test]
    fn a_short_tray_loses_the_picture_before_the_card_shrinks_further() {
        assert_eq!(card_height(1000.0), CARD_H);
        assert_eq!(band(CARD_H), BAND);
        assert_eq!(band(card_height(150.0)), 0.0);
        assert_eq!(card_height(10.0), HEADER + BODY + FOOTER);
        let mid = card_height(HEADER + BODY + FOOTER + 40.0);
        assert_eq!(band(mid), 40.0);
    }

    #[test]
    fn the_drop_gap_is_found_from_the_pointer() {
        let layout = Layout::new(&[false; 3], 330.0);
        assert_eq!(layout.gap_at(0.0), 0);
        assert_eq!(layout.gap_at(layout.card(0).center().x + 1.0), 1);
        assert_eq!(layout.gap_at(layout.card(1).center().x + 1.0), 2);
        assert_eq!(layout.gap_at(layout.card(2).right()), 3);
        assert_eq!(layout.gap_x(0), layout.input().right() + CABLE / 2.0);
        assert_eq!(layout.gap_x(3), layout.card(2).right() + CABLE / 2.0);
    }

    #[test]
    fn a_folded_card_is_a_rail_and_the_others_move_up_to_it() {
        let layout = Layout::new(&[false, true, false], 330.0);
        assert_eq!(layout.card(0).width(), CARD_W);
        assert_eq!(layout.card(1).width(), RAIL_W);
        assert_eq!(layout.card(1).left() - layout.card(0).right(), CABLE);
        assert_eq!(layout.card(2).left() - layout.card(1).right(), CABLE);
        assert_eq!(layout.output().left() - layout.card(2).right(), CABLE);
        assert_eq!(layout.gap_at(layout.card(1).center().x - 1.0), 1);
        assert_eq!(layout.gap_at(layout.card(1).center().x + 1.0), 2);
        let cables = layout.cables();
        assert_eq!(cables.len(), 4);
        for (a, b) in cables {
            assert_eq!(b - a, CABLE);
        }
    }
}
