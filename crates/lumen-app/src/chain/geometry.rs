//! Where everything in the chain sits, in points from the tray's left edge
//! (scroll is applied by the caller). Pure, so the layout is tested without
//! a window.

use eframe::egui::{Rect, pos2, vec2};

/// A card's width.
pub(super) const CARD_W: f32 = 232.0;
/// A card at its tallest, with the picture.
pub(super) const CARD_H: f32 = 344.0;
/// The picture's height at the tallest.
pub(super) const BAND: f32 = 72.0;
/// The card's header (power, name, badge).
pub(super) const HEADER: f32 = 48.0;
/// The footer (signal and latency).
pub(super) const FOOTER: f32 = 32.0;
/// The row under the header that holds a running plugin's preset field.
pub(super) const PRESET: f32 = 34.0;
/// Below the header: the preset row (when there is one) and the parameter
/// strip, or the message that replaces it.
pub(super) const BODY: f32 = 158.0 + PRESET;
/// What a plugin effect's sidechain row adds to a card: it takes this from
/// the picture, and from the knobs below it where the picture is gone.
pub(super) const KEY_ROOM: f32 = 28.0;
/// A cable between two cards.
pub(super) const CABLE: f32 = 40.0;
/// The chips at each end of the chain.
pub(super) const CHIP_W: f32 = 112.0;
pub(super) const CHIP_H: f32 = 28.0;
/// Space round the chain inside the tray.
pub(super) const PAD: f32 = 16.0;
/// A port's radius.
pub(super) const PORT: f32 = 4.0;

/// The least the detail panel can be on the Devices tab: a compact card
/// `card` tall (D2; [`COMPACT_H`] unless a message makes one taller), in the
/// tray's padding, so the card never sits past the tray.
pub(crate) fn panel_least(card: f32) -> f32 {
    card + 2.0 * PAD + 2.0 * crate::theme::space::S + crate::DETAIL_CHROME
}

/// The least the detail panel can be on the Devices tab, with every card at
/// its compact height.
#[cfg(test)]
fn panel_min_height() -> f32 {
    panel_least(COMPACT_H)
}

/// A compact card (D2, the owner's A): a 36 pt header, one row of the strip's
/// first four knobs at 18 pt, and a 24 pt footer, as wide as a full card.
pub(super) const COMPACT_H: f32 = 116.0;
pub(super) const COMPACT_HEADER: f32 = 36.0;
pub(super) const COMPACT_FOOTER: f32 = 24.0;
/// How far past the switch point the room must go before the chain changes
/// its cards, each way: dragging the panel's edge never makes them flicker.
const HYSTERESIS: f32 = 8.0;

/// Whether the chain shows compact cards in a tray `tray_height` tall, given
/// whether it did last frame. A full card loses its picture first; the
/// chain turns compact when even a full card without one does not fit (with
/// `extra` for a sidechain row), and turns back once one fits again.
pub(super) fn compact(was: bool, tray_height: f32, extra: f32) -> bool {
    let room = tray_height - 2.0 * PAD;
    let switch = HEADER + BODY + FOOTER + extra;
    if was {
        room < switch + HYSTERESIS
    } else {
        room < switch - HYSTERESIS
    }
}

/// The card height when `room` points are free: the card shrinks (the
/// picture first) until there is no picture, then stops.
#[cfg(test)]
pub(super) fn card_height(room: f32) -> f32 {
    card_height_with(room, 0.0)
}

/// As [`card_height`] for a chain with a sidechain row, `extra` taller at the
/// least.
pub(super) fn card_height_with(room: f32, extra: f32) -> f32 {
    room.clamp(HEADER + BODY + FOOTER + extra, CARD_H)
}

/// A picture shorter than this is not drawn: a few points of it would only
/// show a clipped sliver of its lettering above the header.
const BAND_LEAST: f32 = 24.0;

/// The picture's height for a card `height` tall.
pub(super) fn band(height: f32) -> f32 {
    let room = (height - HEADER - BODY - FOOTER).clamp(0.0, BAND);
    if room < BAND_LEAST { 0.0 } else { room }
}

/// A folded card's width: its power button and its name set on end.
pub(super) const RAIL_W: f32 = 48.0;

/// The laid-out chain for devices, folded or not, in a tray `height` tall.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Layout {
    pub cards: usize,
    /// The tallest card's height.
    pub card_h: f32,
    /// The y of the cables and chips, from the tray's top.
    pub cable_y: f32,
    top: f32,
    /// Each card's left edge, width and height, from the chain's left edge.
    spans: Vec<(f32, f32, f32)>,
    /// Where the output chip starts.
    end: f32,
}

impl Layout {
    /// `folded[i]` is true when card `i` is folded.
    #[cfg(test)]
    pub fn new(folded: &[bool], tray_height: f32) -> Self {
        Self::keyed(folded, tray_height, 0.0)
    }

    /// As [`Layout::new`], with `extra` more room for the sidechain row of a
    /// card in the chain (0 when none has one).
    pub fn keyed(folded: &[bool], tray_height: f32, extra: f32) -> Self {
        let card_h = card_height_with(tray_height - 2.0 * PAD, extra);
        Self::build(folded, tray_height, &vec![card_h; folded.len()], card_h)
    }

    /// The chain of compact cards, card `i` `heights[i]` tall (a message can
    /// make one taller than the rest): their tops line up, the tallest is
    /// centred in the tray, and the cables cross every card where a compact
    /// card's would.
    pub fn sized(folded: &[bool], tray_height: f32, heights: &[f32]) -> Self {
        Self::build(folded, tray_height, heights, COMPACT_H)
    }

    /// The chain with card `i` `heights[i]` tall; an empty chain lays its
    /// chips out as if for a card `empty` tall.
    fn build(folded: &[bool], tray_height: f32, heights: &[f32], empty: f32) -> Self {
        let card_h = heights.iter().copied().fold(0.0, f32::max);
        let card_h = if heights.is_empty() { empty } else { card_h };
        let least = heights.iter().copied().fold(card_h, f32::min);
        let top = ((tray_height - card_h) / 2.0).max(PAD.min(tray_height / 2.0));
        let mut x = PAD + CHIP_W + CABLE;
        let mut spans = Vec::with_capacity(folded.len());
        for (&fold, &h) in folded.iter().zip(heights) {
            let w = if fold { RAIL_W } else { CARD_W };
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
        // A cable past the last card (or past the input chip, when empty).
        Rect::from_min_size(
            pos2(self.end, self.cable_y - CHIP_H / 2.0),
            vec2(CHIP_W, CHIP_H),
        )
    }

    /// The whole chain's width, with its padding.
    pub fn width(&self) -> f32 {
        self.output().right() + PAD
    }

    /// The cable segments, left to right: between the input chip, every
    /// card and the output chip, as `(from x, to x)` at `cable_y`.
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

    /// The gap a pointer at `x` (from the chain's left edge) would drop
    /// into: 0 is before the first card, `cards` after the last.
    pub fn gap_at(&self, x: f32) -> usize {
        (0..self.cards)
            .find(|&i| x < self.card(i).center().x)
            .unwrap_or(self.cards)
    }

    /// The x of the middle of gap `i`, where a drop line goes.
    pub fn gap_x(&self, i: usize) -> f32 {
        if i == 0 {
            self.input().right() + CABLE / 2.0
        } else {
            self.card(i - 1).right() + CABLE / 2.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // The owner's figure: 116 + 32 + 16 + 48.
        assert_eq!(panel_min_height(), 212.0);
        let tray = panel_min_height() - crate::DETAIL_CHROME - 2.0 * space;
        assert!(compact(false, tray, 0.0) && compact(false, tray, KEY_ROOM));
        let layout = Layout::sized(&[false], tray, &[COMPACT_H]);
        let card = layout.card(0);
        // The tray's scrolling area is a point in from each edge.
        assert!(card.top() >= 1.0, "{card:?}");
        assert!(card.bottom() <= tray - 1.0, "{card:?} in {tray}");
        assert!(panel_least(COMPACT_H + 28.0) > panel_min_height());
    }

    #[test]
    fn the_chain_turns_compact_past_the_switch_and_back_past_it_the_other_way() {
        let switch = HEADER + BODY + FOOTER + 2.0 * PAD;
        // Full: it stays full until the room is 8 pt short.
        assert!(!compact(false, switch - 7.0, 0.0));
        assert!(compact(false, switch - 9.0, 0.0));
        // Compact: it stays compact until there is 8 pt to spare.
        assert!(compact(true, switch + 7.0, 0.0));
        assert!(!compact(true, switch + 9.0, 0.0));
        // A sidechain row moves the switch by its room.
        assert!(compact(false, switch + KEY_ROOM - 9.0, KEY_ROOM));
        // Even 8 pt short, a full card stays in the tray's padding.
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
        // A pointer past the rail's middle drops after it.
        assert_eq!(layout.gap_at(layout.card(1).center().x - 1.0), 1);
        assert_eq!(layout.gap_at(layout.card(1).center().x + 1.0), 2);
        let cables = layout.cables();
        assert_eq!(cables.len(), 4);
        for (a, b) in cables {
            assert_eq!(b - a, CABLE);
        }
    }
}
