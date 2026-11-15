use eframe::egui::epaint::text::VariationCoords;
use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId, FontTweak};

const INTER: &[u8] = include_bytes!("../../assets/fonts/Inter.ttf");
const UNBOUNDED: &[u8] = include_bytes!("../../assets/fonts/Unbounded.ttf");
const MARTIAN_MONO: &[u8] = include_bytes!("../../assets/fonts/MartianMono.ttf");
const MONO_WIDTH: f32 = 87.5;

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

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
    let mono = defs.families.entry(FontFamily::Monospace).or_default();
    mono.insert(0, "martian-mono".to_owned());
    defs
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

const CAP_HEIGHT: f32 = 0.7275;
const X_HEIGHT: f32 = 0.5459;

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
        assert!(
            middle > galley.size().y / 2.0,
            "{middle} {:?}",
            galley.size()
        );
    }
}
