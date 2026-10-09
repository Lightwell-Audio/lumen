//! The design system's colour, type, spacing and shape tokens
//! (`docs/DESIGN.md` §3–§5), and the egui style built from them. Everything
//! the app paints takes its values from here; motion lives in `motion.rs`.
//!
//! The palette in use is global (one window, one theme), so painting code
//! can call [`p()`] without threading a context through.

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme, vec2,
};

/// Every colour token, for one theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub app: Color32,
    pub canvas: Color32,
    pub raised: Color32,
    pub overlay: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub control_press: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_hint: Color32,
    pub text_disabled: Color32,
    /// The single accent: active, selected, playing, focus.
    pub accent: Color32,
    pub accent_muted: Color32,
    /// Record is the one semantic exception to the single accent.
    pub record: Color32,
    /// Problems that need attention (device lost, could not start).
    pub warning: Color32,
    pub grid_bar: Color32,
    pub grid_beat: Color32,
    /// Grid lines between beats (M3 slice 5a).
    pub grid_sub: Color32,
    /// The start marker (where Play starts): neutral, so it never reads as
    /// the playhead.
    pub start_marker: Color32,
    /// Lane background behind clips.
    pub lane: Color32,
    /// Track header column (M3 slice 4).
    pub header: Color32,
    /// A selected row (a track header), tinted toward the accent.
    pub selected_row: Color32,
    /// Controls on a selected row: the plain control grey has the same
    /// brightness as the selection tint and would vanish, so they step up
    /// (owner, after slice 4a). See [`controls`].
    pub selected_control: Color32,
    /// Clip body: dark, so the track-coloured title bar and waveform carry
    /// the colour (owner's choice, slice 4).
    pub clip_body: Color32,
    pub shadow: Color32,
}

pub const DARK: Palette = Palette {
    app: Color32::from_rgb(0x17, 0x19, 0x1D),
    canvas: Color32::from_rgb(0x1B, 0x1D, 0x22),
    raised: Color32::from_rgb(0x23, 0x26, 0x2D),
    overlay: Color32::from_rgb(0x26, 0x2A, 0x31),
    control: Color32::from_rgb(0x31, 0x35, 0x3E),
    control_hover: Color32::from_rgb(0x3A, 0x3F, 0x4A),
    control_press: Color32::from_rgb(0x44, 0x4A, 0x56),
    border: Color32::from_rgb(0x2E, 0x32, 0x3A),
    border_strong: Color32::from_rgb(0x3C, 0x41, 0x4B),
    text: Color32::from_rgb(0xE6, 0xE8, 0xEC),
    text_secondary: Color32::from_rgb(0x9A, 0xA0, 0xAC),
    text_hint: Color32::from_rgb(0x6E, 0x74, 0x80),
    text_disabled: Color32::from_rgb(0x55, 0x5A, 0x65),
    accent: Color32::from_rgb(0x4F, 0xA3, 0xFF),
    accent_muted: Color32::from_rgb(0x24, 0x3A, 0x55),
    record: Color32::from_rgb(0xE5, 0x48, 0x4D),
    warning: Color32::from_rgb(0xF2, 0xB1, 0x4C),
    grid_bar: Color32::from_rgb(0x34, 0x38, 0x42),
    grid_beat: Color32::from_rgb(0x26, 0x29, 0x30),
    grid_sub: Color32::from_rgb(0x21, 0x23, 0x29),
    start_marker: Color32::from_rgb(0xC8, 0xCC, 0xD4),
    lane: Color32::from_rgb(0x1B, 0x1D, 0x22),
    header: Color32::from_rgb(0x1E, 0x21, 0x26),
    selected_row: Color32::from_rgb(0x22, 0x34, 0x49),
    selected_control: Color32::from_rgb(0x43, 0x4D, 0x5E),
    clip_body: Color32::from_rgb(0x26, 0x2A, 0x31),
    shadow: Color32::from_black_alpha(96),
};

pub const LIGHT: Palette = Palette {
    app: Color32::from_rgb(0xF1, 0xF2, 0xF4),
    canvas: Color32::from_rgb(0xFA, 0xFA, 0xFB),
    raised: Color32::from_rgb(0xE9, 0xEB, 0xEE),
    overlay: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    control: Color32::from_rgb(0xE2, 0xE5, 0xE9),
    control_hover: Color32::from_rgb(0xD7, 0xDB, 0xE0),
    control_press: Color32::from_rgb(0xCB, 0xD0, 0xD7),
    border: Color32::from_rgb(0xDA, 0xDD, 0xE2),
    border_strong: Color32::from_rgb(0xC3, 0xC8, 0xCF),
    text: Color32::from_rgb(0x1C, 0x1F, 0x24),
    text_secondary: Color32::from_rgb(0x5A, 0x61, 0x70),
    text_hint: Color32::from_rgb(0x87, 0x8D, 0x98),
    text_disabled: Color32::from_rgb(0xB3, 0xB8, 0xC0),
    accent: Color32::from_rgb(0x1F, 0x6F, 0xD1),
    accent_muted: Color32::from_rgb(0xD5, 0xE5, 0xFA),
    record: Color32::from_rgb(0xD3, 0x3A, 0x40),
    warning: Color32::from_rgb(0xA8, 0x64, 0x00),
    grid_bar: Color32::from_rgb(0xD9, 0xDC, 0xE1),
    grid_beat: Color32::from_rgb(0xEC, 0xEE, 0xF1),
    grid_sub: Color32::from_rgb(0xF3, 0xF4, 0xF6),
    start_marker: Color32::from_rgb(0x6B, 0x71, 0x7C),
    lane: Color32::from_rgb(0xF6, 0xF7, 0xF8),
    header: Color32::from_rgb(0xEC, 0xEE, 0xF1),
    selected_row: Color32::from_rgb(0xDB, 0xE7, 0xF8),
    selected_control: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    clip_body: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    shadow: Color32::from_black_alpha(36),
};

/// Track and clip colours: muted, medium lightness, so white names read on
/// them in both themes. The same in both themes (content keeps its
/// identity).
pub const CLIP_COLORS: [Color32; 12] = [
    Color32::from_rgb(0x4C, 0x7A, 0x9E),
    Color32::from_rgb(0x7A, 0x6A, 0x9E),
    Color32::from_rgb(0x5E, 0x8C, 0x6A),
    Color32::from_rgb(0x9E, 0x7A, 0x4C),
    Color32::from_rgb(0x9E, 0x5A, 0x6A),
    Color32::from_rgb(0x5A, 0x8E, 0x8E),
    Color32::from_rgb(0x6A, 0x7E, 0xB0),
    Color32::from_rgb(0x8E, 0x6A, 0x8E),
    Color32::from_rgb(0x7E, 0x8E, 0x56),
    Color32::from_rgb(0xA0, 0x6E, 0x5A),
    Color32::from_rgb(0x56, 0x86, 0xA0),
    Color32::from_rgb(0x88, 0x76, 0x60),
];
/// The colours of a control, for the surface under it: plain on most
/// surfaces, stepped up on a selected row so it keeps its edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Controls {
    pub fill: Color32,
    pub hover: Color32,
    /// Pressed, and the filled part of a slider.
    pub press: Color32,
    /// Letters on an unlit control.
    pub text: Color32,
}

/// Control colours for a surface that is `selected` or not.
pub fn controls(selected: bool) -> Controls {
    let c = p();
    if selected {
        // One step lighter on dark, one step toward white on light.
        let step = |from: Color32| from.lerp_to_gamma(c.text, 0.08);
        Controls {
            fill: c.selected_control,
            hover: step(c.selected_control),
            press: step(step(c.selected_control)),
            text: c.text,
        }
    } else {
        Controls {
            fill: c.control,
            hover: c.control_hover,
            press: c.control_press,
            text: c.text_secondary,
        }
    }
}

/// A waveform inside a clip of `track` colour: brightened on the dark
/// clip body so it reads (owner's choice: dark clips with bright
/// waveforms); on the light theme's white body, the colour itself.
pub fn wave(track: Color32) -> Color32 {
    if LIGHT_IN_USE.load(Ordering::Relaxed) {
        track
    } else {
        track.lerp_to_gamma(Color32::WHITE, 0.3)
    }
}

/// Text on a filled control (a lit button): near-black on light fills,
/// white on dark ones, whatever the theme.
pub fn text_on(fill: Color32) -> Color32 {
    let luma =
        0.2126 * f32::from(fill.r()) + 0.7152 * f32::from(fill.g()) + 0.0722 * f32::from(fill.b());
    if luma > 140.0 {
        TEXT_ON_LIGHT
    } else {
        Color32::WHITE
    }
}

/// Text on light fills: the dark theme's `app` colour, a little deeper.
pub const TEXT_ON_LIGHT: Color32 = Color32::from_rgb(0x0B, 0x0D, 0x10);

/// Text on clips.
pub const CLIP_TEXT: Color32 = Color32::WHITE;

/// A muted clip (the owner's M1, slice 6e): its colour drains to this grey,
/// as in Ableton. Medium lightness like [`CLIP_COLORS`], so its name and
/// waveform stay readable in both themes.
pub const CLIP_MUTED: Color32 = Color32::from_rgb(0x6C, 0x70, 0x78);

/// Spacing on a 4 pt grid (`DESIGN.md` §5).
pub mod space {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
}

/// Corner radii.
pub mod radius {
    pub const SMALL: u8 = 4;
    pub const MEDIUM: u8 = 6;
    pub const LARGE: u8 = 8;
}

/// Type sizes (`DESIGN.md` §4).
pub mod size {
    pub const CAPTION: f32 = 11.0;
    pub const BODY: f32 = 13.0;
    pub const TITLE: f32 = 15.0;
    pub const HEADING: f32 = 16.0;
    /// The "Lumen" wordmark in the transport bar (Unbounded is wide).
    pub const WORDMARK: f32 = 14.0;
    /// The info line's height along the bottom of the window (M4 slice 0).
    pub const INFO_LINE: f32 = 28.0;
}

/// Which theme the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
    /// Follow the Windows (or macOS) setting.
    System,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::Dark, Appearance::Light, Appearance::System];

    pub fn name(self) -> &'static str {
        match self {
            Appearance::Dark => "Dark",
            Appearance::Light => "Light",
            Appearance::System => "Match system",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Appearance::Dark => "dark",
            Appearance::Light => "light",
            Appearance::System => "system",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|a| a.key() == key)
            .unwrap_or_default()
    }
}

static LIGHT_IN_USE: AtomicBool = AtomicBool::new(false);

/// The palette in use.
pub fn p() -> &'static Palette {
    if LIGHT_IN_USE.load(Ordering::Relaxed) {
        &LIGHT
    } else {
        &DARK
    }
}

/// Applies `appearance`: picks the palette (resolving "Match system" with
/// what the OS reports) and rebuilds egui's style from the tokens. Cheap;
/// call when the setting changes or the OS theme changes.
pub fn apply(ctx: &egui::Context, appearance: Appearance, animation_time: f32) {
    let light = match appearance {
        Appearance::Dark => false,
        Appearance::Light => true,
        Appearance::System => ctx.system_theme() == Some(Theme::Light),
    };
    LIGHT_IN_USE.store(light, Ordering::Relaxed);
    let palette = p();
    let theme = if light { Theme::Light } else { Theme::Dark };
    ctx.set_theme(theme);
    ctx.style_mut_of(theme, |style| {
        build_style(style, palette, light, animation_time);
    });
}

fn build_style(style: &mut egui::Style, c: &Palette, light: bool, animation_time: f32) {
    style.animation_time = animation_time;
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(size::CAPTION, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(size::BODY, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(size::BODY, FontFamily::Proportional),
        ),
        // Window titles use the heading style.
        (
            TextStyle::Heading,
            FontId::new(size::TITLE, FontFamily::Name("display".into())),
        ),
        (
            TextStyle::Monospace,
            FontId::new(size::BODY, FontFamily::Monospace),
        ),
    ]
    .into();

    let spacing = &mut style.spacing;
    spacing.item_spacing = vec2(space::S, space::XS + 2.0);
    spacing.button_padding = vec2(space::S, 3.0);
    spacing.interact_size.y = 22.0;
    spacing.window_margin = Margin::same(space::M as i8);
    spacing.menu_margin = Margin::same(space::S as i8);

    let v = &mut style.visuals;
    *v = if light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    v.panel_fill = c.app;
    v.window_fill = c.overlay;
    v.window_stroke = Stroke::new(1.0, c.border_strong);
    v.window_corner_radius = CornerRadius::same(radius::LARGE);
    // One flat window surface: no separately tinted title bar.
    v.window_highlight_topmost = false;
    v.menu_corner_radius = CornerRadius::same(radius::MEDIUM);
    let shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: c.shadow,
    };
    v.window_shadow = shadow;
    v.popup_shadow = shadow;
    v.extreme_bg_color = c.raised;
    v.faint_bg_color = c.raised;
    v.code_bg_color = c.raised;
    v.warn_fg_color = c.warning;
    v.error_fg_color = c.record;
    v.hyperlink_color = c.accent;
    v.override_text_color = None;
    v.weak_text_color = Some(c.text_hint);
    v.selection.bg_fill = c.accent_muted;
    v.selection.stroke = Stroke::new(1.0, c.accent);

    let radius = CornerRadius::same(radius::SMALL);
    let w = &mut v.widgets;
    // Non-interactive: labels, separators, frames.
    w.noninteractive.bg_fill = c.app;
    w.noninteractive.weak_bg_fill = c.app;
    w.noninteractive.bg_stroke = Stroke::new(1.0, c.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
    w.noninteractive.corner_radius = radius;
    // At rest. Buttons fill with `weak_bg_fill`; checkboxes, radios and
    // slider rails with `bg_fill`, a step stronger so they read as controls.
    w.inactive.bg_fill = c.control_press;
    w.inactive.weak_bg_fill = c.control;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, c.text);
    w.inactive.corner_radius = radius;
    // Under the pointer.
    w.hovered.bg_fill = c.control_hover;
    w.hovered.weak_bg_fill = c.control_hover;
    w.hovered.bg_stroke = Stroke::new(1.0, c.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.5, c.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;
    // Pressed, or focused from the keyboard: the accent outline is the
    // focus ring.
    w.active.bg_fill = c.control_press;
    w.active.weak_bg_fill = c.control_press;
    w.active.bg_stroke = Stroke::new(1.5, c.accent);
    w.active.fg_stroke = Stroke::new(2.0, c.text);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;
    // Open menus and combo boxes.
    w.open.bg_fill = c.control_press;
    w.open.weak_bg_fill = c.control_press;
    w.open.bg_stroke = Stroke::new(1.0, c.border_strong);
    w.open.fg_stroke = Stroke::new(1.0, c.text);
    w.open.corner_radius = radius;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance contrast.
    fn contrast(a: Color32, b: Color32) -> f32 {
        fn lum(c: Color32) -> f32 {
            let ch = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.039_28 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
        }
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_and_signals_meet_contrast_in_both_themes() {
        for (name, c) in [("dark", DARK), ("light", LIGHT)] {
            for bg in [c.app, c.canvas, c.overlay] {
                assert!(contrast(c.text, bg) >= 7.0, "{name} text");
                assert!(contrast(c.text_secondary, bg) >= 4.5, "{name} secondary");
                assert!(contrast(c.accent, bg) >= 3.0, "{name} accent");
                assert!(contrast(c.warning, bg) >= 3.0, "{name} warning");
            }
            assert!(contrast(c.text, c.control) >= 4.5, "{name} button text");
        }
    }

    #[test]
    fn controls_keep_their_edge_on_a_selected_row() {
        for (name, c) in [("dark", DARK), ("light", LIGHT)] {
            // At least as distinct as a control on an unselected header
            // (owner: they vanished on the blue selection).
            let plain = contrast(c.control, c.header);
            assert!(
                contrast(c.selected_control, c.selected_row) >= plain,
                "{name} control on selection"
            );
            assert!(
                contrast(c.text, c.selected_control) >= 4.5,
                "{name} letters"
            );
            assert!(contrast(c.text, c.selected_row) >= 7.0, "{name} names");
        }
    }

    #[test]
    fn clip_names_read_on_every_clip_colour() {
        for color in CLIP_COLORS.into_iter().chain([CLIP_MUTED]) {
            assert!(contrast(CLIP_TEXT, color) >= 3.0, "{color:?}");
        }
    }

    #[test]
    fn appearance_keys_round_trip() {
        for a in Appearance::ALL {
            assert_eq!(Appearance::from_key(a.key()), a);
        }
        assert_eq!(Appearance::from_key("nonsense"), Appearance::Dark);
    }
}
