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

/// Inter's cap height and x-height, as fractions of the size (1490 and
/// 1118 of 2048 units).
const CAP_HEIGHT: f32 = 0.7275;
const X_HEIGHT: f32 = 0.5459;

/// How far below the top of a one-line `galley` its words look centred:
/// halfway between the middle of its capitals and the middle of its
/// lower-case letters. An icon or keycap beside a label is centred here;
/// centred on the galley's box (the capitals' middle in Inter) it reads high
/// beside mixed-case words such as "Pointer".
pub fn optical_middle(galley: &egui::Galley) -> f32 {
    let size = galley
        .job
        .sections
        .first()
        .map_or(0.0, |section| section.format.font_id.size);
    let baseline = galley
        .rows
        .first()
        .and_then(|row| row.row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y));
    match baseline {
        Some(baseline) => baseline - size * (CAP_HEIGHT + X_HEIGHT) / 4.0,
        None => galley.size().y / 2.0,
    }
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

    #[test]
    fn the_optical_middle_is_between_the_capitals_and_the_lower_case() {
        let ctx = egui::Context::default();
        install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let size = crate::theme::size::BODY;
        let galley = ctx.fonts_mut(|fonts| {
            fonts.layout_no_wrap(
                "Pointer".to_owned(),
                FontId::proportional(size),
                egui::Color32::WHITE,
            )
        });
        let row = &galley.rows[0];
        let baseline = row.pos.y + row.row.glyphs[0].pos.y;
        let capitals = baseline - size * CAP_HEIGHT / 2.0;
        let lower_case = baseline - size * X_HEIGHT / 2.0;
        let middle = optical_middle(&galley);
        assert!(
            capitals < middle && middle < lower_case,
            "{capitals} {middle} {lower_case}"
        );
        // The box's middle is the capitals' in Inter, which reads high.
        assert!(
            middle > galley.size().y / 2.0,
            "{middle} {:?}",
            galley.size()
        );
    }
}
