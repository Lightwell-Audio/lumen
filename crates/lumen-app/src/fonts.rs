//! Typefaces (`docs/DESIGN.md` §4): **Inter** for the interface and
//! **Unbounded** as the companion face for personality (the wordmark, window
//! and panel titles). Chosen by the owner from side-by-side specimens,
//! 2026-10-07. Unbounded is wide, so it is set smaller than a narrow face.
//! **Martian Mono** for numbers that must not jitter (the interface audit's
//! S1, chosen by the owner, M4.5): its square, wide shapes echo Unbounded;
//! it is set at width 87.5 so readouts keep their room.
//!
//! All three are variable fonts; each weight (and Martian Mono's width) is
//! the same file registered with its own coordinates. egui's bundled fonts
//! stay behind them as fallbacks for symbols (⚠, ⟳, emoji), and Hack behind
//! Martian Mono.

use eframe::egui::epaint::text::VariationCoords;
use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId, FontTweak};

const INTER: &[u8] = include_bytes!("../assets/fonts/Inter.ttf");
const UNBOUNDED: &[u8] = include_bytes!("../assets/fonts/Unbounded.ttf");
const MARTIAN_MONO: &[u8] = include_bytes!("../assets/fonts/MartianMono.ttf");
/// Martian Mono's width: narrower than its default 100, so the position,
/// tempo and meter scales keep the room Hack had.
const MONO_WIDTH: f32 = 87.5;

/// Medium-weight interface text: section headings, emphasis.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

/// The companion face: wordmark, window and panel titles.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

fn weighted(bytes: &'static [u8], weight: f32) -> FontData {
    FontData::from_static(bytes).tweak(FontTweak {
        coords: VariationCoords::new([("wght", weight)]),
        ..FontTweak::default()
    })
}

fn mono() -> FontData {
    FontData::from_static(MARTIAN_MONO).tweak(FontTweak {
        coords: VariationCoords::new([("wght", 400.0), ("wdth", MONO_WIDTH)]),
        ..FontTweak::default()
    })
}

pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let fallbacks = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (name, data) in [
        ("inter-400", weighted(INTER, 400.0)),
        ("inter-560", weighted(INTER, 560.0)),
        ("unbounded-560", weighted(UNBOUNDED, 560.0)),
        ("martian-mono", mono()),
    ] {
        defs.font_data.insert(name.into(), data.into());
    }
    let family = |first: &str| {
        let mut list = vec![first.to_owned()];
        list.extend(fallbacks.iter().cloned());
        list
    };
    defs.families
        .insert(FontFamily::Proportional, family("inter-400"));
    defs.families
        .insert(FontFamily::Name("medium".into()), family("inter-560"));
    defs.families
        .insert(FontFamily::Name("display".into()), family("unbounded-560"));
    // Martian Mono first, Hack and the symbol fallbacks behind it.
    let mono = defs.families.entry(FontFamily::Monospace).or_default();
    mono.insert(0, "martian-mono".to_owned());
    defs
}

/// Installs the typefaces. Takes effect from the next frame.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_three_families_exist_with_fallbacks() {
        let defs = definitions();
        for family in [
            FontFamily::Proportional,
            FontFamily::Monospace,
            FontFamily::Name("medium".into()),
            FontFamily::Name("display".into()),
        ] {
            let list = &defs.families[&family];
            assert!(list.len() > 1, "{family:?} has no fallbacks");
            for name in list {
                assert!(defs.font_data.contains_key(name), "{name} missing");
            }
        }
        assert_eq!(defs.families[&FontFamily::Monospace][0], "martian-mono");
    }
}
