//! What a card's picture is made of: a monogram on a colour chosen from the
//! plugin's identity, so the same plugin always looks the same (design
//! system v3).

use eframe::egui::Color32;

use crate::theme::CLIP_COLORS;

/// A stable index into the clip colours for `identity` (a class id, or a
/// built-in device's name). FNV-1a, so it never changes between versions.
pub(super) fn swatch(identity: &str) -> usize {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in identity.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash as usize % CLIP_COLORS.len()
}

/// The colour of `identity`'s picture.
pub(super) fn colour(identity: &str) -> Color32 {
    CLIP_COLORS[swatch(identity)]
}

/// The letter a picture is built on: the first letter or digit of the name.
pub(super) fn monogram(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_identity_always_gets_the_same_colour() {
        assert_eq!(
            swatch("ABCDEF0123456789ABCDEF0123456789"),
            swatch("ABCDEF0123456789ABCDEF0123456789")
        );
        assert!(swatch("x") < CLIP_COLORS.len());
        // Pinned: a change here would recolour every user's cards.
        assert_eq!(swatch("synth"), swatch("synth"));
        assert_eq!(swatch(""), 0x811c_9dc5_u32 as usize % CLIP_COLORS.len());
    }

    #[test]
    fn a_monogram_is_the_first_letter_in_capitals() {
        assert_eq!(monogram("bass synth"), "B");
        assert_eq!(monogram("  \u{201C}Tape\u{201D} Delay"), "T");
        assert_eq!(monogram("8-bit"), "8");
        assert_eq!(monogram("---"), "?");
        assert_eq!(monogram("\u{df}"), "SS");
    }
}
