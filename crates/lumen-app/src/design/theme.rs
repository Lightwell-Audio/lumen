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
    /// Record red lightened for the notes of a clip being recorded (3-A):
    /// lighter than `record`, so notes read on the take's tinted body.
    pub record_note: Color32,
    /// Problems that need attention (device lost, could not start).
    pub warning: Color32,
    pub grid_bar: Color32,
    pub grid_beat: Color32,
    /// Grid lines between beats (M3 slice 5a).
    pub grid_sub: Color32,
    /// The grid drawn over an audio clip's body, under its wave (the audio
    /// editor): bar, beat and between-beat lines, a wash of the ink that
    /// reads on the clip body in each theme, so they sit on any body.
    pub clip_grid_bar: Color32,
    pub clip_grid_beat: Color32,
    pub clip_grid_sub: Color32,
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
    /// Inset wells: fader grooves and meter backgrounds. Darker than every
    /// surface they sit on, in both themes (the interface audit's T2: `app`
    /// was lighter than the strips in the light theme).
    pub well: Color32,
    pub shadow: Color32,
    /// The piano roll's keys and lanes (M5 slice 3b, the owner's look A,
    /// "studio keys"): white keys, black keys, the label on a white key,
    /// the darker lane behind a black-key pitch, and the faint line at B|C
    /// and E|F.
    pub key_white: Color32,
    pub key_black: Color32,
    pub key_text: Color32,
    pub lane_black: Color32,
    pub lane_split: Color32,
    /// The centre of a light (`light::BEAM`, ADR-0043): white in the dark,
    /// the accent itself on the light theme, where light is ink, not glow.
    pub core: Color32,
    /// The logo's sun (the owner's Dawn in Sunrise): its rim, and its
    /// centre. Its water is the accent.
    pub sun: Color32,
    pub sun_core: Color32,
    /// The material (the design system's Material card, ADR-0052): the
    /// deepest layer, under `well` (behind the window, scrims, the app
    /// icon's base).
    pub ground: Color32,
    /// The top of a raised control's gradient, `control_hi` to `control` (a
    /// 3% step); never flat.
    pub control_hi: Color32,
    /// The chosen piece of a segmented control: a raised thumb in the well,
    /// `thumb_hi` to `thumb`, with `text` letters.
    pub thumb_hi: Color32,
    pub thumb: Color32,
    /// The 1 pt line along the top inside edge of every raised surface.
    pub edge_light: Color32,
    /// The sheen along the top inside half of a clip's colour bar and of a
    /// note: white at 18%, in both themes.
    pub sheen: Color32,
    /// The line where the piano roll's keys meet its lanes: black at 19%,
    /// in both themes.
    pub key_edge: Color32,
    /// The 1 pt lip along the top inside edge of a well or a pressed
    /// control.
    pub shade: Color32,
    /// Light level 1 (Ember): `text` at 8%, washed over what the pointer is
    /// on. Raised controls, rows and segmented pieces hover with it.
    pub ember: Color32,
    /// A filled accent control (the one primary of a view, a lit S): a
    /// gradient from `accent_hi` to `accent_lo`, `on_accent` letters.
    pub accent_hi: Color32,
    pub accent_lo: Color32,
    pub on_accent: Color32,
    /// A lit M, and its letter.
    pub mute: Color32,
    pub on_mute: Color32,
    /// The block budget bar's five costliest plugins, darkest first: the
    /// accent stepped in lightness so neighbours stay apart (ADR-0075).
    pub budget: [Color32; 5],
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
    record_note: Color32::from_rgb(0xF2, 0x7B, 0x7F),
    warning: Color32::from_rgb(0xF2, 0xB1, 0x4C),
    grid_bar: Color32::from_rgb(0x34, 0x38, 0x42),
    grid_beat: Color32::from_rgb(0x26, 0x29, 0x30),
    grid_sub: Color32::from_rgb(0x21, 0x23, 0x29),
    clip_grid_bar: Color32::from_rgba_premultiplied(0x33, 0x33, 0x33, 0x33),
    clip_grid_beat: Color32::from_rgba_premultiplied(0x1A, 0x1A, 0x1A, 0x1A),
    clip_grid_sub: Color32::from_rgba_premultiplied(0x0B, 0x0B, 0x0B, 0x0B),
    start_marker: Color32::from_rgb(0xC8, 0xCC, 0xD4),
    lane: Color32::from_rgb(0x1B, 0x1D, 0x22),
    header: Color32::from_rgb(0x1E, 0x21, 0x26),
    selected_row: Color32::from_rgb(0x22, 0x34, 0x49),
    selected_control: Color32::from_rgb(0x43, 0x4D, 0x5E),
    clip_body: Color32::from_rgb(0x26, 0x2A, 0x31),
    well: Color32::from_rgb(0x14, 0x16, 0x19),
    core: Color32::WHITE,
    sun: Color32::from_rgb(0xFF, 0xC4, 0x6B),
    sun_core: Color32::WHITE,
    shadow: Color32::from_black_alpha(96),
    key_white: Color32::from_rgb(0xC9, 0xCD, 0xD4),
    key_black: Color32::from_rgb(0x0E, 0x10, 0x13),
    key_text: Color32::from_rgb(0x3A, 0x3F, 0x4A),
    lane_black: Color32::from_rgb(0x16, 0x18, 0x1C),
    lane_split: Color32::from_rgb(0x2E, 0x32, 0x3A),
    ground: Color32::from_rgb(0x11, 0x13, 0x16),
    control_hi: Color32::from_rgb(0x38, 0x3C, 0x46),
    thumb_hi: Color32::from_rgb(0x4A, 0x51, 0x60),
    thumb: Color32::from_rgb(0x3E, 0x45, 0x51),
    edge_light: Color32::from_rgba_unmultiplied_const(0xFF, 0xFF, 0xFF, 0x12),
    sheen: Color32::from_rgba_unmultiplied_const(0xFF, 0xFF, 0xFF, 46),
    key_edge: Color32::from_black_alpha(48),
    shade: Color32::from_black_alpha(0x80),
    ember: Color32::from_rgba_unmultiplied_const(0xE6, 0xE8, 0xEC, 0x14),
    accent_hi: Color32::from_rgb(0x5D, 0xAE, 0xFF),
    accent_lo: Color32::from_rgb(0x3F, 0x8F, 0xEA),
    on_accent: Color32::from_rgb(0x06, 0x18, 0x2E),
    mute: Color32::from_rgb(0xC8, 0xCC, 0xD4),
    on_mute: Color32::from_rgb(0x0B, 0x0D, 0x10),
    budget: [
        Color32::from_rgb(0x2B, 0x6C, 0xB8),
        Color32::from_rgb(0x4F, 0xA3, 0xFF),
        Color32::from_rgb(0x7D, 0xBA, 0xFF),
        Color32::from_rgb(0xA9, 0xD1, 0xFF),
        Color32::from_rgb(0xD3, 0xE8, 0xFF),
    ],
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
    record_note: Color32::from_rgb(0xE0, 0x60, 0x66),
    warning: Color32::from_rgb(0xA8, 0x64, 0x00),
    grid_bar: Color32::from_rgb(0xD9, 0xDC, 0xE1),
    grid_beat: Color32::from_rgb(0xEC, 0xEE, 0xF1),
    grid_sub: Color32::from_rgb(0xF3, 0xF4, 0xF6),
    clip_grid_bar: Color32::from_black_alpha(0x2E),
    clip_grid_beat: Color32::from_black_alpha(0x17),
    clip_grid_sub: Color32::from_black_alpha(0x0A),
    start_marker: Color32::from_rgb(0x6B, 0x71, 0x7C),
    lane: Color32::from_rgb(0xF6, 0xF7, 0xF8),
    header: Color32::from_rgb(0xEC, 0xEE, 0xF1),
    selected_row: Color32::from_rgb(0xDB, 0xE7, 0xF8),
    selected_control: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    clip_body: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    well: Color32::from_rgb(0xD6, 0xDA, 0xE0),
    core: Color32::from_rgb(0x1F, 0x6F, 0xD1),
    sun: Color32::from_rgb(0xCC, 0x9D, 0x56),
    sun_core: Color32::from_rgb(0x80, 0x62, 0x36),
    shadow: Color32::from_black_alpha(36),
    key_white: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    key_black: Color32::from_rgb(0x3A, 0x3F, 0x4A),
    key_text: Color32::from_rgb(0x5A, 0x61, 0x70),
    lane_black: Color32::from_rgb(0xEC, 0xEE, 0xF1),
    lane_split: Color32::from_rgb(0xD2, 0xD6, 0xDC),
    ground: Color32::from_rgb(0xE7, 0xE9, 0xED),
    control_hi: Color32::from_rgb(0xEE, 0xF0, 0xF3),
    thumb_hi: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    thumb: Color32::from_rgb(0xF3, 0xF4, 0xF6),
    edge_light: Color32::from_rgba_unmultiplied_const(0xFF, 0xFF, 0xFF, 0xCC),
    sheen: Color32::from_rgba_unmultiplied_const(0xFF, 0xFF, 0xFF, 46),
    key_edge: Color32::from_black_alpha(48),
    shade: Color32::from_rgba_unmultiplied_const(0x1C, 0x1F, 0x24, 0x24),
    ember: Color32::from_rgba_unmultiplied_const(0x1C, 0x1F, 0x24, 0x14),
    accent_hi: Color32::from_rgb(0x2A, 0x74, 0xD4),
    accent_lo: Color32::from_rgb(0x1A, 0x62, 0xBD),
    on_accent: Color32::WHITE,
    mute: Color32::from_rgb(0x5A, 0x61, 0x70),
    on_mute: Color32::WHITE,
    budget: [
        Color32::from_rgb(0x0F, 0x3F, 0x7A),
        Color32::from_rgb(0x1F, 0x6F, 0xD1),
        Color32::from_rgb(0x4F, 0x93, 0xE3),
        Color32::from_rgb(0x86, 0xB6, 0xEE),
        Color32::from_rgb(0xB4, 0xD2, 0xF5),
    ],
};

/// A model colour (`0xRRGGBB`) as the interface draws it: the one place a
/// `u32` colour is unpacked.
pub const fn colour_of(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Track and clip colours: the model's palette (`Color::PALETTE`, which
/// new tracks take in turn), drawn as it is. The same in both themes
/// (content keeps its identity).
pub const CLIP_COLORS: [Color32; 12] = {
    let palette = lumen_project::model::Color::PALETTE;
    let mut colors = [Color32::BLACK; 12];
    let mut i = 0;
    while i < colors.len() {
        colors[i] = colour_of(palette[i].0);
        i += 1;
    }
    colors
};
/// The colours of a control, for the surface under it: plain on most
/// surfaces, stepped up on a selected row so it keeps its edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Controls {
    /// The bottom of a raised control's gradient (flat controls use it
    /// alone).
    pub fill: Color32,
    /// The top of a raised control's gradient: `control_hi`; on a selected
    /// row the same as `fill` (the step-up is flat).
    pub fill_hi: Color32,
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
            fill_hi: c.selected_control,
            hover: step(c.selected_control),
            press: step(step(c.selected_control)),
            text: c.text,
        }
    } else {
        Controls {
            fill: c.control,
            fill_hi: c.control_hi,
            hover: c.control_hover,
            press: c.control_press,
            text: c.text_secondary,
        }
    }
}

/// How a control that is on looks (the design system, ADR-0052, replacing
/// ADR-0051's lit tones). State is never colour alone: the shape and the
/// label stay. Filled controls do not glow: their fill is their light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// A toggle, pill or chip that is on: `accent_muted`, `accent` letters
    /// and one accent hairline at 38%.
    On,
    /// Filled accent: the one primary of a view, a lit S, the chosen tool.
    Accent,
    /// A lit M: `mute` with `on_mute` letters.
    Mute,
    /// Record while recording: record red over the control, a 45% hairline
    /// and the Flare (the one lit control that glows).
    Record,
}

/// A lit control's paint: a gradient from `top` to `bottom`, its `edge`
/// line, the `light` along its top inside edge (none on an `On` control)
/// and its `text`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LitFill {
    pub top: Color32,
    pub bottom: Color32,
    pub edge: Color32,
    pub light: Color32,
    pub text: Color32,
}

/// The edge light of a filled control: one stronger line, white at 35%.
pub const EDGE_ON_FILL: Color32 = Color32::from_rgba_unmultiplied_const(0xFF, 0xFF, 0xFF, 0x59);
/// How strong the accent hairline of something that is on is.
pub const ON_HAIRLINE: f32 = 0.38;
/// The hairline of a recording control, in record red.
const RECORD_HAIRLINE: f32 = 0.45;
/// How much record red tints a recording control's fill.
const RECORD_TINT: f32 = 0.18;

impl Tone {
    /// The colour of the light a lit control of this tone gives, if any:
    /// only recording glows.
    pub fn glow(self) -> Option<Color32> {
        match self {
            Tone::Record => Some(p().record),
            _ => None,
        }
    }
}

/// The paint of a control of `tone` that is on.
pub fn lit(tone: Tone) -> LitFill {
    lit_in(p(), tone)
}

fn lit_in(c: &Palette, tone: Tone) -> LitFill {
    match tone {
        Tone::On => LitFill {
            top: c.accent_muted,
            bottom: c.accent_muted,
            edge: c.accent.gamma_multiply(ON_HAIRLINE),
            light: Color32::TRANSPARENT,
            text: c.accent,
        },
        Tone::Accent => LitFill {
            top: c.accent_hi,
            bottom: c.accent_lo,
            edge: c.accent_lo,
            light: EDGE_ON_FILL,
            text: c.on_accent,
        },
        Tone::Mute => LitFill {
            top: c.mute,
            bottom: c.mute,
            edge: c.mute,
            light: EDGE_ON_FILL,
            text: c.on_mute,
        },
        Tone::Record => {
            let fill = c.control.lerp_to_gamma(c.record, RECORD_TINT);
            LitFill {
                top: fill,
                bottom: fill,
                edge: c.record.gamma_multiply(RECORD_HAIRLINE),
                light: Color32::TRANSPARENT,
                text: c.record,
            }
        }
    }
}

/// The 1 pt light along the top inside edge of a raised surface.
pub fn edge_light() -> Color32 {
    p().edge_light
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

/// Meters (M4 slice 5): above this level a meter turns the warning colour.
pub const METER_WARN_DB: f32 = -6.0;
/// How strongly a meter's peak shows behind its RMS.
pub const METER_PEAK_ALPHA: f32 = 0.4;
/// The opacity of what steps back beside something lit: the info line's
/// other actions while a modifier is held (the interface audit's I1). A clip
/// that steps back has its own two levels, [`CLIP_DIMMED`] and
/// [`UNDER_LANE`].
pub const DIMMED: f32 = 0.45;
/// The opacity of clips on tracks that cannot be heard (muted, or a group
/// that is), a step darker than [`DIMMED`] so it reads as silent at a
/// glance.
pub const CLIP_DIMMED: f32 = 0.42;
/// How much of a clip shows through the automation lane over it (M7 slice
/// A1, the owner's L1 B: the clips step back while a lane is shown).
pub const UNDER_LANE: f32 = 0.45;
/// How strong a clip's outline is in its colour.
pub const CLIP_EDGE: f32 = 0.6;
/// The audio editor's file outside the clip's window: the wave in
/// `text_hint` at this strength, on a `well` wash at `OUTSIDE_WASH`.
pub const WAVE_OUTSIDE: f32 = 0.55;
pub const OUTSIDE_WASH: f32 = 0.7;
/// The audio editor's warp strip (M9.5 S5b): a marker's pole at rest and
/// when lit; the ghost notch's fill, and its ink when it is not on a
/// transient; a transient triangle's strength at 0 and what a full-strength
/// transient adds.
pub const WARP_POLE: f32 = 0.45;
pub const WARP_POLE_LIT: f32 = 0.8;
pub const WARP_GHOST_FILL: f32 = 0.15;
pub const WARP_GHOST_FREE: f32 = 0.6;
pub const WARP_TICK_BASE: f32 = 0.45;
pub const WARP_TICK_GAIN: f32 = 0.4;
/// The audio editor's handles at rest: `text` at this strength. They stay
/// out of the wave's way and are still plain to find (M9.5 HS A).
pub const HANDLE_QUIET: f32 = 0.55;
/// The audio editor's trim grips at rest: `text_secondary` at this strength.
pub const GRIP_QUIET: f32 = 0.7;

/// The opacities colours are laid over other surfaces at (`docs/DESIGN.md`
/// §3): a colour token is multiplied by one of these before it is drawn, so
/// the colours themselves stay in the palette. The same in both themes.
pub mod alpha {
    /// Nothing: a gradient's far end.
    pub const NONE: f32 = 0.0;
    /// The faintest wash a lane or a span takes, just enough to say it is
    /// there.
    pub const WASH_FAINT: f32 = 0.05;
    /// A lane tinted with the accent to show what it holds.
    pub const WASH_LANE: f32 = 0.06;
    /// The lane a dragged clip or file would land in.
    pub const WASH_DROP: f32 = 0.08;
    /// An automation lane while its mode writes.
    pub const WASH_WRITE: f32 = 0.09;
    /// A wash under something moving: an insertion band, a hovered row, the
    /// area a curve will fill.
    pub const WASH: f32 = 0.10;
    /// A warning chip's fill.
    pub const WARN_FILL: f32 = 0.10;
    /// The faintest ring of a handle's halo.
    pub const HALO_FAINT: f32 = 0.10;
    /// A take's tint over its body, and a note or a box being drawn.
    pub const WASH_SELECTED: f32 = 0.12;
    /// A band that marks a level: the loudness target, a hovered chip.
    pub const WASH_BAND: f32 = 0.14;
    /// The glow that spreads beside an accent edge.
    pub const GLOW: f32 = 0.15;
    /// The band over a time range or a selection.
    pub const WASH_RANGE: f32 = 0.16;
    /// The accent wash inside an outline while it is drawn.
    pub const WASH_OUTLINE: f32 = 0.18;
    /// The halo around a handle or a point.
    pub const HALO: f32 = 0.20;
    /// A span over a lane: a loop's or a stretch's.
    pub const WASH_SPAN: f32 = 0.22;
    /// The halo around a handle or a point, one step wider than [`HALO`].
    pub const HALO_WIDE: f32 = 0.22;
    /// A clip body's colour at a quarter, under its grid.
    pub const WASH_BODY: f32 = 0.25;
    /// A line or a word that only hints.
    pub const INK_TRACE: f32 = 0.30;
    /// The loop brace's fill.
    pub const WASH_BRACE: f32 = 0.35;
    /// One step of a piece's hover.
    pub const HOVER_STEP: f32 = 0.35;
    /// A warning chip's edge.
    pub const WARN_EDGE: f32 = 0.35;
    /// A line that steps back: a return behind the one on top.
    pub const EDGE_FAINT: f32 = 0.35;
    /// A mark's soft edge: a selection's outline, an armed lane's rim.
    pub const EDGE_SOFT: f32 = 0.45;
    /// A hairline's wash, and the ghost of a control that is not there.
    pub const EDGE: f32 = 0.50;
    /// Quiet ink: what is there but not speaking.
    pub const INK_QUIET: f32 = 0.55;
    /// A dimmed edge, or words that step back.
    pub const EDGE_DIM: f32 = 0.60;
    /// A wash over the cell about to be scrubbed.
    pub const WASH_CELL: f32 = 0.60;
    /// Ink that has stepped back: a clip that is not heard.
    pub const INK_DIM: f32 = 0.70;
    /// A loop brace's edge.
    pub const EDGE_BRACE: f32 = 0.75;
    /// A lit accent edge.
    pub const EDGE_LIT: f32 = 0.80;
    /// A warning's words.
    pub const WARN_INK: f32 = 0.88;
    /// A clip's name at nine tenths.
    pub const TEXT_BRIGHT: f32 = 0.90;
}

/// A meter's colour at `db`: `base` (the accent, or the track's colour with
/// the owner's M2 setting) up to [`METER_WARN_DB`], the warning colour up to
/// full scale, record above it. Clipping is also shown by the clip light's
/// place and shape, never by colour alone.
pub fn meter(db: f32, base: Color32) -> Color32 {
    let c = p();
    if db > 0.0 {
        c.record
    } else if db > METER_WARN_DB {
        c.warning
    } else {
        base
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

/// A warning chip's fill and edge (the dropout chip): the warning colour
/// washed over the bar, so its amber text keeps its contrast.
pub fn warning_fill() -> Color32 {
    p().warning.gamma_multiply(0.14)
}

pub fn warning_edge() -> Color32 {
    p().warning.gamma_multiply(0.45)
}

/// Text on light fills: the dark theme's `app` colour, a little deeper.
pub const TEXT_ON_LIGHT: Color32 = Color32::from_rgb(0x0B, 0x0D, 0x10);

/// Text on clips.
pub const CLIP_TEXT: Color32 = Color32::WHITE;

/// A muted clip (the owner's M1, slice 6e): its colour drains to this grey.
/// Medium lightness like [`CLIP_COLORS`], so its name and
/// waveform stay readable in both themes.
pub const CLIP_MUTED: Color32 = Color32::from_rgb(0x6C, 0x70, 0x78);

/// Each built-in device's signature hue (M8 slice B0, `DESIGN.md` §7k): the
/// colour of its picture and of its marks, the same in both themes, as a
/// clip's colour is. A device keeps its identity wherever it is drawn.
pub mod personality {
    use super::Color32;

    /// Prism, the utility device: a quiet steel grey, the one device that
    /// changes nothing of its own.
    pub const UTILITY: Color32 = Color32::from_rgb(0x9A, 0xA0, 0xAC);

    /// Iris, the parametric EQ (M8 slice B1): a periwinkle blue, the colour of
    /// the curve it draws (`docs/specimens/m8/chosen.html`).
    pub const EQ: Color32 = Color32::from_rgb(0x7C, 0x8C, 0xFF);

    /// The ramp Iris's drawn curve and its analyser run along, low frequency to
    /// high: the spectrum, content rather than chrome, so the same in both
    /// themes. The owner's specimens draw an EQ's curve this way (round 2 of
    /// `docs/specimens/m8/`).
    pub const EQ_SWEEP: [Color32; 5] = [
        Color32::from_rgb(0xFF, 0x5A, 0x6E),
        Color32::from_rgb(0xFF, 0xB5, 0x47),
        Color32::from_rgb(0x5B, 0xE0, 0x7F),
        Color32::from_rgb(0x4F, 0xA3, 0xFF),
        Color32::from_rgb(0xB0, 0x7C, 0xFF),
    ];

    /// Iris's eight bands, one colour each in the order they are numbered:
    /// their point on the screen, their pill, and the dials while one is picked.
    /// Band 2 is the specimen's amber, so the card at rest is the round-2 one.
    pub const EQ_BANDS: [Color32; 8] = [
        Color32::from_rgb(0xFF, 0x5A, 0x6E),
        Color32::from_rgb(0xFF, 0xB5, 0x47),
        Color32::from_rgb(0xE6, 0xD6, 0x4A),
        Color32::from_rgb(0x5B, 0xE0, 0x7F),
        Color32::from_rgb(0x3F, 0xD6, 0xC6),
        Color32::from_rgb(0x4F, 0xA3, 0xFF),
        Color32::from_rgb(0xB0, 0x7C, 0xFF),
        Color32::from_rgb(0xE2, 0x6F, 0xD0),
    ];

    /// Parallax, the stereo delay: a cool teal, the colour of an echo
    /// receding into the dark.
    pub const DELAY: Color32 = Color32::from_rgb(0x3F, 0xD6, 0xC6);

    /// Afterglow, the reverb: a warm pink, the colour of the light that
    /// lingers after the sound.
    pub const REVERB: Color32 = Color32::from_rgb(0xFF, 0x6F, 0xA8);

    /// The deep colours the screens of Parallax and Afterglow fade to from
    /// their hue (round 2's `deep`), the same in both themes.
    pub const DELAY_DEEP: Color32 = Color32::from_rgb(0x07, 0x10, 0x0F);
    pub const REVERB_DEEP: Color32 = Color32::from_rgb(0x12, 0x09, 0x10);

    /// The light at the heart of Afterglow's glow, and the warm ring round it
    /// (also the colour of "to silence"): content, the same in both themes.
    pub const REVERB_CORE: Color32 = Color32::from_rgb(0xFF, 0xF1, 0xD6);
    pub const REVERB_WARM: Color32 = Color32::from_rgb(0xFF, 0xC0, 0x8A);

    /// The screen behind a built-in device's picture (DESIGN.md §7k): deep, and
    /// the same in both themes, as a clip's colour is. Its top is the device's
    /// signature colour, so the band stays the personality's.
    pub const SCREEN: Color32 = Color32::from_rgb(0x0C, 0x0E, 0x12);
    /// How much of the signature colour the screen's top carries.
    pub const SCREEN_TINT: f32 = 0.20;

    /// The angle between Prism's neighbouring beams at a Width of 130 %
    /// (radians), the specimen's 0.12; the fan follows Width in proportion.
    pub const PRISM_FAN: f32 = 0.12;
    /// What a screen's readout is set in: the mono size, and how far in from
    /// the screen's corner it sits.
    pub const READOUT_SIZE: f32 = 9.0;
    pub const READOUT_INSET: f32 = 10.0;

    /// A built-in's card, a set piece (DESIGN.md §7k; the owner's round-2
    /// specimen `docs/specimens/m8/devices.html`, 2026-11-12): sizes in points,
    /// opacities as shares of the device's hue. The same in both themes where
    /// it is a screen, as a clip's colour is.
    pub mod card {
        use eframe::egui::Color32;

        /// The card's corner radius.
        pub const RADIUS: f32 = 14.0;
        /// The header's height, and the hue's opacity at its top fading to
        /// nothing at its foot.
        pub const HEADER_HEIGHT: f32 = 53.0;
        pub const HEADER_TINT: f32 = 0.30;
        /// The power button: its centre from the card's left and top, its
        /// radius, the hue's fill and ring.
        pub const POWER_X: f32 = 22.0;
        pub const POWER_Y: f32 = 27.0;
        pub const POWER_RADIUS: f32 = 9.0;
        pub const POWER_FILL: f32 = 0.22;
        pub const POWER_RING: f32 = 0.8;
        /// The name in the display face: size, left edge, baseline from the
        /// card's top and the extra space between letters; the subtitle's size
        /// and baseline.
        pub const NAME_SIZE: f32 = 15.0;
        pub const NAME_X: f32 = 40.0;
        pub const NAME_BASELINE: f32 = 27.0;
        pub const NAME_SPACING: f32 = 0.4;
        pub const SUBTITLE_SIZE: f32 = 11.0;
        pub const SUBTITLE_BASELINE: f32 = 43.0;
        /// The Preset pill at the header's right: size, its inset from the
        /// right edge and from the top, the fill and ring, and its type size.
        pub const PRESET_SIZE_PT: (f32, f32) = (54.0, 20.0);
        pub const PRESET_INSET: f32 = 12.0;
        pub const PRESET_TOP: f32 = 16.0;
        pub const PRESET_FILL: f32 = 0.16;
        pub const PRESET_RING: f32 = 0.40;
        pub const PRESET_TEXT: f32 = 11.0;
        /// The screen: inset from the card's sides and this far under its top,
        /// its corner radius and the hairline of the hue round it.
        pub const SCREEN_INSET: f32 = 14.0;
        pub const SCREEN_TOP: f32 = 60.0;
        pub const SCREEN_RADIUS: f32 = 10.0;
        pub const SCREEN_RING: f32 = 0.35;
        /// Its glow: the hue at this share at its top middle, over the card's
        /// own colour, fading to the deep colour by this share of its width.
        pub const SCREEN_GLOW: f32 = 0.20;
        pub const SCREEN_REACH: f32 = 1.1;
        /// The dial: radius, its tracks and head, the lit arc's glow, the
        /// pointer, and how far the disc sits inside the ring.
        pub const DIAL_RADIUS: f32 = 14.0;
        pub const DIAL_TRACK: f32 = 3.0;
        pub const DIAL_TRACK_ALPHA: f32 = 0.22;
        pub const DIAL_LIT: f32 = 2.5;
        pub const DIAL_GLOW: [(f32, f32); 2] = [(8.0, 0.12), (5.5, 0.26)];
        /// While the dial is held the glow is this much wider and stronger.
        pub const DIAL_HELD_GROWTH: f32 = 0.6;
        pub const DIAL_HEAD: f32 = 3.0;
        pub const DIAL_POINTER: f32 = 2.0;
        pub const DIAL_POINTER_INSET: f32 = 9.0;
        pub const DIAL_DISC_INSET: f32 = 4.0;
        /// The name and the value under a dial: baselines from its centre
        /// plus its radius, and type sizes.
        pub const DIAL_NAME_BASELINE: f32 = 14.0;
        pub const DIAL_VALUE_BASELINE: f32 = 26.0;
        pub const DIAL_NAME_SIZE: f32 = 10.5;
        pub const DIAL_VALUE_SIZE: f32 = 9.0;
        /// The disc's face on each theme (centre, rim) and the pointer's ink.
        pub const DIAL_DISC_DARK: (Color32, Color32) = (
            Color32::from_rgb(0x3A, 0x3F, 0x4A),
            Color32::from_rgb(0x22, 0x25, 0x2B),
        );
        pub const DIAL_DISC_LIGHT: (Color32, Color32) = (
            Color32::from_rgb(0xFF, 0xFF, 0xFF),
            Color32::from_rgb(0xE2, 0xE5, 0xE9),
        );
        pub const DIAL_POINTER_DARK: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEC);
        pub const DIAL_POINTER_LIGHT: Color32 = Color32::from_rgb(0x3A, 0x3F, 0x4A);
        /// A pill (Ø L): its fill lit and at rest, its ring at rest (shares of
        /// the colour), and its type size.
        pub const PILL_ON_FILL: f32 = 0.18;
        pub const PILL_OFF_FILL: f32 = 0.08;
        pub const PILL_OFF_RING: f32 = 0.40;
        pub const PILL_TEXT: f32 = 10.0;
        /// The names and values on the deep screen, in white at these shares.
        pub const SCREEN_NAME_ALPHA: f32 = 0.62;
        /// The quiet "All n ›" link at the card's foot: type size, inset from
        /// the card's right and centre height from its bottom.
        pub const ALL_TEXT: f32 = 10.0;
        pub const ALL_INSET: f32 = 14.0;
        pub const ALL_FROM_BOTTOM: f32 = 12.0;

        /// The short card (the chain's own width, 272 tall): the screen ends
        /// this far above the card's bottom; the dials sit on its lower edge,
        /// this tall a band, their centres this far above its foot.
        pub const SHORT_BOTTOM: f32 = 26.0;
        pub const SHORT_DIALS_BAND: f32 = 84.0;
        pub const SHORT_DIAL_FROM_FOOT: f32 = 56.0;
        /// The Preset pill on the short card sits a little higher, so the subtitle
        /// under the name can run beneath it without touching.
        pub const SHORT_PRESET_TOP: f32 = 13.0;
        /// The short card's names and values under the dials (size, size, the
        /// baselines from a dial's foot): a little smaller than the whole card's,
        /// so four fit across the screen.
        pub const SHORT_NAME_SIZE: f32 = 9.5;
        pub const SHORT_VALUE_SIZE: f32 = 8.5;
        pub const SHORT_NAME_BASELINE: f32 = 12.0;
        pub const SHORT_VALUE_BASELINE: f32 = 23.0;
        /// The compact card (116 tall): the header's height, the power, the name
        /// and the Preset pill at their smaller sizes, a thin band of the picture
        /// under it (this far from the header, this tall) and, under the band, four
        /// small dials (this gap below it) with their names and values.
        pub const COMPACT_HEADER: f32 = 38.0;
        pub const COMPACT_POWER_Y: f32 = 19.0;
        pub const COMPACT_NAME_SIZE: f32 = 13.0;
        pub const COMPACT_NAME_BASELINE: f32 = 24.0;
        pub const COMPACT_PRESET: (f32, f32) = (46.0, 18.0);
        pub const COMPACT_PRESET_TOP: f32 = 10.0;
        pub const COMPACT_PRESET_TEXT: f32 = 10.0;
        pub const COMPACT_BAND_GAP: f32 = 2.0;
        pub const COMPACT_BAND_HEIGHT: f32 = 22.0;
        pub const COMPACT_DIAL_GAP: f32 = 6.0;
        pub const COMPACT_DIAL_RADIUS: f32 = 10.0;
        pub const COMPACT_NAME_LABEL: f32 = 9.5;
        pub const COMPACT_VALUE_LABEL: f32 = 8.5;
        pub const COMPACT_NAME_BASELINE_UNDER: f32 = 11.0;
        pub const COMPACT_VALUE_BASELINE_UNDER: f32 = 21.0;

        /// A folded card: the hue's bar (inset from the top and bottom, width),
        /// the name's baseline from the rail's left, its size, and how bright
        /// the bar is at rest and at full level.
        pub const RAIL_BAR: (f32, f32) = (14.0, 3.0);
        pub const RAIL_NAME_X: f32 = 24.0;
        pub const RAIL_NAME_SIZE: f32 = 13.0;
        pub const RAIL_BAR_REST: f32 = 0.55;

        /// The meter's floor: a level at or below this many dB is nothing.
        pub const LEVEL_FLOOR_DB: f32 = -60.0;
        /// How wide a handle's grab is, and its lit ring.
        pub const HANDLE_RING: f32 = 6.0;
        /// How strong the ring round a lit handle is, at full light.
        pub const HANDLE_RING_ALPHA: f32 = 0.6;
        /// The live value chip while a handle is dragged: height and type size.
        pub const CHIP_HEIGHT: f32 = 18.0;
        pub const CHIP_TEXT: f32 = 9.0;

        /// A drawing's parts (Iris's bands): how near a press counts on a point,
        /// how far Shift slows a drag, and the numbered pill: its fill at rest
        /// and picked (shares of the part's colour, the text's own colour at
        /// rest), its ring and number when the part is off, and its number's
        /// size as a share of its height.
        pub const PART_REACH: f32 = 11.0;
        pub const PART_FINE: f32 = 0.1;
        pub const PART_PILL_REST: f32 = 0.10;
        pub const PART_PILL_PICKED: f32 = 0.35;
        pub const PART_PILL_OFF: f32 = 0.35;
        pub const PART_PILL_OFF_TEXT: f32 = 0.5;
        pub const PART_PILL_TEXT_SHARE: f32 = 0.5;
    }

    /// The light Prism splits (DESIGN.md §7k): the spectrum its beams fan into.
    /// Content, not chrome, so the same in both themes.
    pub const PRISM_BEAMS: [Color32; 5] = [
        Color32::from_rgb(0xFF, 0x5A, 0x6E),
        Color32::from_rgb(0xFF, 0xB5, 0x47),
        Color32::from_rgb(0x5B, 0xE0, 0x7F),
        Color32::from_rgb(0x4F, 0xA3, 0xFF),
        Color32::from_rgb(0xB0, 0x7C, 0xFF),
    ];
}

/// Spacing on a 4 pt grid (`DESIGN.md` §5).
pub mod space {
    /// Hairline gaps: inside a segmented control, between a strip's rails.
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    /// Panel toolbar insets, and around an empty state's parts.
    pub const L: f32 = 16.0;
    /// First-run layouts (M9 R7b): between Lumi and the title, and above
    /// the Ready step's actions.
    pub const XL: f32 = 24.0;
}

/// Corner radii, by the height of what they round (`DESIGN.md` §5): a
/// piece inside a trough takes the trough's radius less its 2 pt inset.
pub mod radius {
    /// Marks a few points wide: a colour bar, a tick.
    pub const MARK: u8 = 1;
    /// 20 pt controls and troughs, clips, notes, keycaps, chips in 20 pt
    /// rows.
    pub const SMALL: u8 = 4;
    /// 24 pt controls, inputs, menu rows.
    pub const MEDIUM: u8 = 6;
    /// 30 pt controls, popovers and menus.
    pub const LARGE: u8 = 8;
    /// Windows, cards, the command palette.
    pub const XL: u8 = 12;
    /// Pills: switches, lane tabs, chips (egui clamps it to half the
    /// height).
    pub const FULL: u8 = 255;

    /// The radius of a control `height` points tall.
    pub fn for_height(height: f32) -> u8 {
        if height >= super::size::CONTROL_LARGE - 0.5 {
            LARGE
        } else if height >= super::size::CONTROL - 0.5 {
            MEDIUM
        } else {
            SMALL
        }
    }
}

/// Line widths (`DESIGN.md` §5).
/// The tint of a picture that is switched off: the picture, dimmed.
pub const PICTURE_OFF: Color32 = Color32::from_gray(110);

/// The grey of a colour (its average), for what is switched off.
pub fn grey_of(colour: Color32) -> Color32 {
    Color32::from_gray(
        ((u16::from(colour.r()) + u16::from(colour.g()) + u16::from(colour.b())) / 3) as u8,
    )
}

pub mod stroke {
    /// Separators, outlines, edge lights and lips.
    pub const HAIRLINE: f32 = 1.0;
    /// A thin ring around a status light: a step under a hairline.
    pub const RING: f32 = 1.2;
    /// A small hand-drawn glyph's line: between a hairline and a lit line.
    pub const GLYPH: f32 = 1.25;
    /// A hand-drawn glyph at a fixed size, a step above `GLYPH`:
    /// headphones, a track type's icon.
    pub const GLYPH_WIDE: f32 = 1.4;
    /// The crisp line of a Lit or Flare light, and icons.
    pub const LIT: f32 = 1.5;
    /// The keyboard focus ring, and a Beam's line.
    pub const FOCUS: f32 = 2.0;
    /// The accent line under a toggle that is on (the palette and panel
    /// toggles).
    pub const UNDERLINE: f32 = 2.0;
    /// The line of a marker, a handle or a selection's edge: a loop brace,
    /// a drop line, a slider's handle, a reorder line.
    pub const EDGE: f32 = 2.0;
    /// The halo of a lit edge, and the ring of a control drawn as a ghost.
    pub const HALO: f32 = 3.0;
    /// The wide glow that spreads beside an accent edge.
    pub const GLOW_WIDE: f32 = 6.0;
    /// The line weight of an icon, in units of its 16-unit grid: the
    /// ordinary line, a fine one for a small detail, a bold one for a mark
    /// that must carry at a small size, a heavy one for the drum glyphs'
    /// cymbal edge and rim strike (M8 D1), and the strike through a
    /// switched-off icon.
    pub const ICON: f32 = 1.6;
    pub const ICON_FINE: f32 = 1.3;
    pub const ICON_BOLD: f32 = 2.4;
    pub const ICON_HEAVY: f32 = 2.2;
    pub const ICON_STRIKE: f32 = 1.8;
}

/// Type sizes (`DESIGN.md` §4).
pub mod size {
    /// Numbers on meter scales and knob values, a step under `CAPTION`
    /// (M4 slice 5).
    pub const SCALE: f32 = 10.0;
    /// Ruler numbers, units after a readout, clip meta, in the monospace.
    pub const MONO_SMALL: f32 = 10.0;
    /// A unit after a number in a tight field, in hint grey (a strip's
    /// trim, the interface audit's X1–X4 B).
    pub const UNIT: f32 = 9.0;
    pub const CAPTION: f32 = 11.0;
    /// The letters on K5 keycaps: the caption size.
    pub const KEYCAP: f32 = CAPTION;
    pub const BODY: f32 = 13.0;
    pub const TITLE: f32 = 15.0;
    pub const HEADING: f32 = 16.0;
    /// Welcome and first-run titles only, in the display face (M9 R7b).
    pub const DISPLAY: f32 = 24.0;
    /// The loudness column's integrated LUFS, the one figure read from
    /// across the room (M4 slice 5, the owner's L2).
    pub const READOUT: f32 = 20.0;
    /// The "Lumen" wordmark in the transport bar (Unbounded is wide).
    pub const WORDMARK: f32 = 14.0;
    /// The info line's height along the bottom of the window (M4 slice 0).
    pub const INFO_LINE: f32 = 32.0;
    /// Controls: buttons, fields and sliders (`DESIGN.md` §5).
    pub const CONTROL: f32 = 24.0;
    /// Controls in dense rows: M, S and arm, switches, segmented controls,
    /// header sliders (the interface audit's S4: 18, 19, 20 and 22 were in
    /// use).
    pub const CONTROL_SMALL: f32 = 20.0;
    /// Detail-panel toolbars (the tool switch) and the transport's readouts.
    pub const CONTROL_LARGE: f32 = 30.0;
    /// Transport icon buttons (square).
    pub const TRANSPORT_ICON: f32 = 28.0;
    /// The transport bar's height.
    pub const TRANSPORT_BAR: f32 = 44.0;
    /// Track heights move in steps of this: folded 2, unfolded 4 to 14.
    pub const TRACK_STEP: f32 = 22.0;
    /// The track header column's default width (resizable to 480).
    pub const HEADER_COLUMN: f32 = 204.0;
    /// The Monitor badge on a MIDI track's header (a speaker, only when
    /// Monitor is not Auto): width and height.
    pub const BADGE_W: f32 = 16.0;
    pub const BADGE_H: f32 = 14.0;
    /// The right column of an unfolded track header (pan over the In
    /// chip), so their edges line up (ADR-0101).
    pub const HEADER_SIDE: f32 = 60.0;
    /// A piece cut into a 20 pt well (M, S and arm, the magnet, the locator
    /// arrows): its width; it is as tall as the well less `space::XXS` each
    /// side.
    pub const WELL_PIECE_W: f32 = 18.0;
    /// A piece of a mixer strip's M S ● well: the strip has room for wider
    /// pieces than a header (the interface audit's X1–X4).
    pub const STRIP_PIECE_W: f32 = 22.0;
    /// A cell of the transport bar's wells (a button, a readout): as tall
    /// as a 30 pt well less 2 pt each side (the interface audit's TB1).
    pub const WELL_CELL: f32 = CONTROL_LARGE - 2.0 * super::space::XXS;
    /// An icon in a piece of the ruler corner's wells (the tool, the
    /// magnet, tempo lanes): a step above the caption it sits beside.
    pub const CORNER_ICON: f32 = CAPTION + 3.0;
    /// The widest a tooltip grows before its hint wraps.
    pub const TIP_WIDTH: f32 = 280.0;
    /// The level strip beside a header's colour rail.
    pub const LEVEL_STRIP: f32 = 5.0;
    /// A send slot's widest, and the least width that shows its letter.
    pub const SEND_SLOT: f32 = 18.0;
    pub const SEND_SLOT_LETTER: f32 = 12.0;
    /// Send knobs on strips.
    pub const KNOB: f32 = 24.0;
    /// Knobs in lists (a return's senders, the "+N" list).
    pub const KNOB_SMALL: f32 = 18.0;
    /// A window's label column (labels right-aligned in it).
    pub const LABEL_COLUMN: f32 = 96.0;
    /// The label column of a narrow card (the clip's properties).
    pub const LABEL_COLUMN_NARROW: f32 = 72.0;
    /// The Preferences window (M9 R7): its width where there is room; the
    /// width under which its page list folds into a menu above the page;
    /// the least it is drawn; and the page list's column.
    pub const PREFERENCES: f32 = 640.0;
    pub const PREFERENCES_FOLD: f32 = 560.0;
    pub const PREFERENCES_LEAST: f32 = 280.0;
    pub const PAGE_LIST: f32 = 172.0;
    /// How far below the app's top the window opens at most; the room kept
    /// under it; the least height of its page (a short window scrolls it);
    /// and the width of a value slider on a page.
    pub const PREFERENCES_TOP: f32 = 72.0;
    pub const PREFERENCES_BELOW: f32 = 40.0;
    pub const PREFERENCES_LEAST_PAGE: f32 = 80.0;
    pub const PREFERENCES_SLIDER: f32 = 160.0;
    /// The first-run window (M9 R7b): its width where there is room, and
    /// the width under which Lumi is left out and the title wraps; Lumi's
    /// size (the picture is 256 px, sharp to 200%); a sound check light;
    /// the scan's progress bar; and the least width wrapped text keeps
    /// beside a button.
    pub const WELCOME: f32 = 560.0;
    pub const WELCOME_NARROW: f32 = 480.0;
    pub const LUMI: f32 = 84.0;
    pub const SPEAKER_LIGHT: f32 = 20.0;
    pub const PROGRESS_BAR: f32 = 3.0;
    pub const WRAP_LEAST: f32 = 120.0;
    /// The hairline between groups of a tab row, and the detail panel's
    /// subject bar (DP1).
    pub const TAB_DIVIDER: f32 = 14.0;
    /// The detail panel's subject (DP1): the widest its name is drawn
    /// before it truncates, and its colour bar's width.
    pub const SUBJECT_NAME: f32 = 160.0;
    pub const SUBJECT_BAR: f32 = 3.0;
    /// The widest a tab's note (a clip's name after "Clip") is drawn.
    pub const TAB_NOTE: f32 = 140.0;
    /// The Warp popover's controls (M9.5 S5a): the mode dropdown, the tempo
    /// field and the sensitivity slider.
    pub const WARP_MODE: f32 = 120.0;
    pub const WARP_FIELD: f32 = 88.0;
    pub const WARP_SLIDER: f32 = 112.0;
    /// The audio editor's ruler with its loop brace band (M9.5 LB A): the
    /// band is its bottom `BRACE_BAND` points; a brace end's grab reach; the
    /// clip start's flag on the band; the edit cursor's cap.
    pub const EDITOR_RULER: f32 = 37.0;
    pub const BRACE_BAND: f32 = 10.0;
    pub const BRACE_GRAB: f32 = 6.0;
    pub const BRACE_FLAG: f32 = 5.0;
    pub const CURSOR_CAP: f32 = 4.0;
    /// The audio editor's warp strip between its ruler and the wave (M9.5
    /// S5b, P A): its height, a marker's grab reach, how near a transient
    /// the ghost marker lands, the notch's size (a rounded pin pointing at
    /// its place) and a transient's small triangle.
    pub const WARP_STRIP: f32 = 18.0;
    pub const WARP_GRAB: f32 = 6.0;
    pub const WARP_NEAR: f32 = 12.0;
    pub const WARP_NOTCH_W: f32 = 9.0;
    pub const WARP_NOTCH_H: f32 = 12.0;
    /// The notch's gap under the strip's top, the radius of its top
    /// corners and how far its point tapers.
    pub const WARP_NOTCH_INSET: f32 = 2.0;
    pub const WARP_NOTCH_RADIUS: f32 = 2.5;
    pub const WARP_NOTCH_TAPER: f32 = 4.5;
    pub const WARP_TICK_W: f32 = 6.0;
    pub const WARP_TICK_H: f32 = 5.0;
    /// The gap between a transient's triangle and the strip's floor.
    pub const WARP_TICK_FLOOR: f32 = 3.0;
    /// The least room between two of the audio editor's bar numbers before
    /// only every second, fourth… shows.
    pub const BAR_NUMBER_ROOM: f32 = 32.0;
    /// The least room between two of the audio editor's time labels.
    pub const TIME_LABEL_ROOM: f32 = 64.0;
    /// From this height the audio editor draws a stereo file's left over its
    /// right.
    pub const WAVE_SPLIT: f32 = 96.0;
    /// The audio editor's ruler: the room under its time labels, a beat
    /// tick and a time tick, and the dot on each sample at depth.
    pub const RULER_BAND: f32 = 3.0;
    pub const BEAT_TICK: f32 = 5.0;
    pub const TIME_TICK: f32 = 6.0;
    pub const SAMPLE_DOT: f32 = 2.0;
    /// The audio editor's properties strip under its header (M9.5 S3): its
    /// height, the least a gain slider and a fade slider take before they
    /// fold, and the file chip's widest and narrowest.
    pub const EDITOR_STRIP: f32 = 32.0;
    pub const STRIP_GAIN_LEAST: f32 = 56.0;
    pub const STRIP_GAIN: f32 = 104.0;
    pub const STRIP_FADE: f32 = 76.0;
    pub const STRIP_SHAPE: f32 = 70.0;
    pub const STRIP_FILE: f32 = 150.0;
    /// The tab on an audio editor's trim grip.
    pub const GRIP_WIDTH: f32 = 4.0;
    pub const GRIP_HEIGHT: f32 = 22.0;
    /// Within this many points of a list's or a view's side, a drag scrolls it.
    pub const EDGE_SCROLL: f32 = 36.0;
    /// How far a card's shadow reaches past it: what its painter's clip
    /// keeps around the card.
    pub const SHADOW_ROOM: f32 = 24.0;
    /// How far a device card's contents sit in from its edges: the compact
    /// knobs' row at the sides, a footer's buttons above and below.
    pub const CARD_INSET: f32 = 6.0;
    /// One line of caption type: a card's message, a compact knob's name.
    pub const CAPTION_LINE: f32 = 14.0;
    /// A full card's knob names: their line, and the gap above it under
    /// the knob.
    pub const KNOB_NAME_LINE: f32 = 13.0;
    pub const KNOB_NAME_GAP: f32 = 3.0;
    /// A menu row (the interface audit's M1, the owner's choice): rows
    /// touch, and a shortcut's cap and light sit 5 pt in from the
    /// highlight's top and bottom.
    pub const MENU_ROW: f32 = 32.0;
    /// An icon before a menu row's label (the arrangement tools, M6).
    pub const MENU_ICON: f32 = 16.0;
    /// An icon on a square button: 16 pt, and 14 on a small control's.
    pub const BUTTON_ICON: f32 = 16.0;
    pub const BUTTON_ICON_SMALL: f32 = 14.0;
    /// The icon for a square button `height` tall.
    pub fn button_icon(height: f32) -> f32 {
        if height < CONTROL {
            BUTTON_ICON_SMALL
        } else {
            BUTTON_ICON
        }
    }
    /// The tallest a dropdown's list of choices gets before it scrolls.
    pub const MENU_LIST: f32 = 320.0;
    /// The audio panel's block budget bar (ADR-0075): its height, the
    /// swatches beside its legend and list, and how tall its plugin list
    /// gets before it scrolls (about six plugins).
    pub const BUDGET_BAR: f32 = 18.0;
    pub const BUDGET_SWATCH: f32 = 10.0;
    pub const BUDGET_LIST: f32 = 216.0;
    /// Automation lanes (M7 slice A1): a point's radius, how near the
    /// pointer must be to grab one, the room kept above and below the
    /// values, and how far apart a curve's samples are.
    pub const AUTO_POINT: f32 = 4.0;
    /// The dot that bends a segment.
    pub const AUTO_BEND: f32 = 3.5;
    pub const AUTO_GRAB: f32 = 9.0;
    pub const AUTO_PAD: f32 = 8.0;
    pub const AUTO_STEP: f32 = 6.0;
    /// How near a resting place (0 dB, centre...) a dragged value lands on it.
    pub const AUTO_SNAP: f32 = 4.0;
    /// The lane chip: room for its arrow beside the name, and its widths.
    pub const AUTO_CHIP_PAD: f32 = 28.0;
    pub const AUTO_CHIP_MIN: f32 = 64.0;
    pub const AUTO_CHIP_MAX: f32 = 200.0;
    /// The Re-enable button beside the chip of a lane taken over.
    pub const AUTO_REENABLE: f32 = 84.0;
    /// The record-red dot in the chip of a mode that can write.
    pub const AUTO_REC_DOT: f32 = 3.5;
    /// The audio light's radius in the transport's status chip.
    pub const STATUS_DOT: f32 = 4.0;
    /// The chooser's least width.
    pub const AUTO_MENU: f32 = 220.0;
    /// The tempo and meter lanes under the bar ruler (M7 slice A6): their
    /// heights, the room a meter chip keeps around its text, and the width
    /// of the box a value is typed in.
    pub const SONG_TEMPO: f32 = 44.0;
    pub const SONG_METER: f32 = 20.0;
    pub const SONG_CHIP_PAD: f32 = 6.0;
    pub const SONG_TYPE: f32 = 88.0;
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

/// Whether the light theme is in use: lights are halved there
/// (`light::strength`).
pub fn is_light() -> bool {
    LIGHT_IN_USE.load(Ordering::Relaxed)
}

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
    // On the 4 pt grid, with controls 24 tall (`DESIGN.md` §5; the
    // interface audit's S4: the spacing was 6, the padding 3 and controls
    // 22).
    spacing.item_spacing = vec2(space::S, space::XS);
    spacing.button_padding = vec2(space::S, space::XS);
    spacing.interact_size.y = size::CONTROL;
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
    v.window_stroke = Stroke::new(stroke::HAIRLINE, c.border_strong);
    v.window_corner_radius = CornerRadius::same(radius::XL);
    // One flat window surface: no separately tinted title bar.
    v.window_highlight_topmost = false;
    v.menu_corner_radius = CornerRadius::same(radius::LARGE);
    let shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: c.shadow,
    };
    v.window_shadow = shadow;
    v.popup_shadow = shadow;
    v.extreme_bg_color = c.raised;
    // Fields are wells cut into the surface (the Field card).
    v.text_edit_bg_color = Some(c.well);
    v.faint_bg_color = c.raised;
    v.code_bg_color = c.raised;
    v.warn_fg_color = c.warning;
    v.error_fg_color = c.record;
    v.hyperlink_color = c.accent;
    v.override_text_color = None;
    v.weak_text_color = Some(c.text_hint);
    v.selection.bg_fill = c.accent_muted;
    v.selection.stroke = Stroke::new(stroke::HAIRLINE, c.accent);

    // egui's own widgets are 24 pt controls (`interact_size`).
    let radius = CornerRadius::same(radius::MEDIUM);
    let w = &mut v.widgets;
    // Non-interactive: labels, separators, frames.
    w.noninteractive.bg_fill = c.app;
    w.noninteractive.weak_bg_fill = c.app;
    w.noninteractive.bg_stroke = Stroke::new(stroke::HAIRLINE, c.border);
    w.noninteractive.fg_stroke = Stroke::new(stroke::HAIRLINE, c.text);
    w.noninteractive.corner_radius = radius;
    // At rest. Buttons fill with `weak_bg_fill`; checkboxes, radios and
    // slider rails with `bg_fill`, a step stronger so they read as controls.
    w.inactive.bg_fill = c.control_press;
    w.inactive.weak_bg_fill = c.control;
    // A raised control's 1 pt outline.
    w.inactive.bg_stroke = Stroke::new(stroke::HAIRLINE, c.border);
    w.inactive.fg_stroke = Stroke::new(stroke::HAIRLINE, c.text);
    w.inactive.corner_radius = radius;
    // Under the pointer.
    // Hover is Ember: the text colour washed over the fill, never another
    // fill.
    w.hovered.bg_fill = c.control_press.blend(c.ember);
    w.hovered.weak_bg_fill = c.control.blend(c.ember);
    w.hovered.bg_stroke = Stroke::new(stroke::HAIRLINE, c.border_strong);
    w.hovered.fg_stroke = Stroke::new(stroke::HAIRLINE, c.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;
    // Pressed, or focused from the keyboard: the accent outline is the
    // focus ring.
    w.active.bg_fill = c.control_press;
    w.active.weak_bg_fill = c.control_press;
    w.active.bg_stroke = Stroke::new(stroke::FOCUS, c.accent);
    w.active.fg_stroke = Stroke::new(stroke::HAIRLINE, c.text);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;
    // Open menus and combo boxes.
    w.open.bg_fill = c.control_press;
    w.open.weak_bg_fill = c.control_press;
    w.open.bg_stroke = Stroke::new(stroke::HAIRLINE, c.border_strong);
    w.open.fg_stroke = Stroke::new(stroke::HAIRLINE, c.text);
    w.open.corner_radius = radius;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_settings_file_starts_with_the_default_theme() {
        assert_eq!(Appearance::default().key(), crate::settings::DEFAULT_THEME);
        assert_eq!(
            Appearance::from_key(crate::settings::DEFAULT_THEME),
            Appearance::default()
        );
    }

    #[test]
    fn the_clip_colours_are_the_models_palette() {
        for (drawn, kept) in CLIP_COLORS.iter().zip(lumen_project::model::Color::PALETTE) {
            let rgb = u32::from(drawn.r()) << 16 | u32::from(drawn.g()) << 8 | u32::from(drawn.b());
            assert_eq!(rgb, kept.0);
        }
        assert_eq!(CLIP_COLORS[0], Color32::from_rgb(0x4C, 0x7A, 0x9E));
    }

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
    fn wells_sit_below_every_surface_they_are_drawn_on() {
        fn luma(c: Color32) -> u32 {
            u32::from(c.r()) * 2126 + u32::from(c.g()) * 7152 + u32::from(c.b()) * 722
        }
        for (name, c) in [("dark", DARK), ("light", LIGHT)] {
            for surface in [c.app, c.header, c.raised, c.selected_row] {
                assert!(
                    luma(c.well) < luma(surface),
                    "{name}: well above {surface:?}"
                );
            }
            // Visible on the strips in the light theme, where it was lost.
            if name == "light" {
                assert!(contrast(c.well, c.header) >= 1.15, "light well on header");
            }
        }
    }

    #[test]
    fn clip_names_read_on_every_clip_colour() {
        for color in CLIP_COLORS.into_iter().chain([CLIP_MUTED]) {
            assert!(contrast(CLIP_TEXT, color) >= 3.0, "{color:?}");
        }
    }

    #[test]
    fn the_grid_over_a_clip_body_shows_in_both_themes() {
        // The wash laid over the body, as the painter blends it.
        fn over(wash: Color32, body: Color32) -> Color32 {
            let a = f32::from(wash.a()) / 255.0;
            let mix = |w: u8, b: u8| {
                // `wash` is premultiplied, so its colour is already scaled.
                (f32::from(w) + f32::from(b) * (1.0 - a)).round().min(255.0) as u8
            };
            Color32::from_rgb(
                mix(wash.r(), body.r()),
                mix(wash.g(), body.g()),
                mix(wash.b(), body.b()),
            )
        }
        for (name, c) in [("dark", DARK), ("light", LIGHT)] {
            for (wash, least) in [
                (c.clip_grid_bar, 1.3),
                (c.clip_grid_beat, 1.15),
                (c.clip_grid_sub, 1.05),
            ] {
                let ratio = contrast(over(wash, c.clip_body), c.clip_body);
                assert!(ratio >= least, "{name} {wash:?}: {ratio}");
            }
        }
    }

    #[test]
    fn recording_notes_are_lighter_red_than_the_record_red() {
        fn luma(c: Color32) -> u32 {
            u32::from(c.r()) * 2126 + u32::from(c.g()) * 7152 + u32::from(c.b()) * 722
        }
        for (name, c) in [("dark", DARK), ("light", LIGHT)] {
            assert!(luma(c.record_note) > luma(c.record), "{name} note red");
            // Still red: the red channel leads both others.
            assert!(
                c.record_note.r() > c.record_note.g() && c.record_note.r() > c.record_note.b(),
                "{name} note red hue"
            );
            // Notes stand out from a take's body (a faint red over the clip body).
            assert!(contrast(c.record_note, c.clip_body) >= 2.0, "{name} notes");
        }
    }

    #[test]
    fn lit_controls_keep_their_letters_in_both_themes() {
        for (name, c) in [("dark", DARK), ("light", LIGHT)] {
            for tone in [Tone::Accent, Tone::Mute] {
                let fill = lit_in(&c, tone);
                for end in [fill.top, fill.bottom] {
                    let ratio = contrast(fill.text, end);
                    assert!(ratio >= 4.5, "{name} {tone:?} {end:?}: {ratio}");
                }
            }
            // On: accent letters on accent_muted (the source values, kept:
            // 4.4:1 dark, 3.9:1 light; 13 pt and up).
            let on = lit_in(&c, Tone::On);
            assert!(contrast(on.text, on.top) >= 3.5, "{name} on");
            // Recording: the design system's recipe (record letters on an 18%
            // record tint) is 2.7:1 on dark, under the 3:1 icons want; kept
            // as the source and flagged in KNOWN_ISSUES (Record arrives in M9).
            let rec = lit_in(&c, Tone::Record);
            assert!(contrast(rec.text, rec.top) >= 2.5, "{name} record");
            // The raised thumb stands out from its well, and reads on it.
            for thumb in [c.thumb_hi, c.thumb] {
                assert!(contrast(thumb, c.well) >= 1.2, "{name} thumb");
                assert!(contrast(c.text, thumb) >= 4.5, "{name} thumb text");
            }
        }
    }

    #[test]
    fn surfaces_step_lighter_as_they_rise() {
        fn luma(c: Color32) -> u32 {
            u32::from(c.r()) * 2126 + u32::from(c.g()) * 7152 + u32::from(c.b()) * 722
        }
        for (name, c) in [("dark", DARK), ("light", LIGHT)] {
            assert!(luma(c.ground) < luma(c.app), "{name} ground");
            assert!(luma(c.control_hi) > luma(c.control), "{name} control");
            assert!(luma(c.accent_hi) > luma(c.accent_lo), "{name} accent");
            assert!(luma(c.thumb_hi) >= luma(c.thumb), "{name} thumb");
            assert!(contrast(c.text_secondary, c.control) >= 4.5, "{name}");
            // The gradient's lighter top is a 3% step: 4.2:1 on dark.
            assert!(contrast(c.text_secondary, c.control_hi) >= 4.0, "{name}");
        }
        assert!(luma(DARK.ground) < luma(DARK.well));
    }

    #[test]
    fn the_opacity_levels_keep_the_values_the_interface_drew() {
        // The levels that named existing literals (sweep M14): each equals
        // the number it replaced, so nothing moves.
        for (level, drawn) in [
            (alpha::NONE, 0.0),
            (alpha::WASH_FAINT, 0.05),
            (alpha::WASH_LANE, 0.06),
            (alpha::WASH_DROP, 0.08),
            (alpha::WASH_WRITE, 0.09),
            (alpha::WASH, 0.10),
            (alpha::WARN_FILL, 0.10),
            (alpha::HALO_FAINT, 0.10),
            (alpha::WASH_SELECTED, 0.12),
            (alpha::WASH_BAND, 0.14),
            (alpha::GLOW, 0.15),
            (alpha::WASH_RANGE, 0.16),
            (alpha::WASH_OUTLINE, 0.18),
            (alpha::HALO, 0.20),
            (alpha::WASH_SPAN, 0.22),
            (alpha::HALO_WIDE, 0.22),
            (alpha::WASH_BODY, 0.25),
            (alpha::INK_TRACE, 0.30),
            (alpha::WASH_BRACE, 0.35),
            (alpha::HOVER_STEP, 0.35),
            (alpha::WARN_EDGE, 0.35),
            (alpha::EDGE_FAINT, 0.35),
            (alpha::EDGE_SOFT, 0.45),
            (alpha::EDGE, 0.50),
            (alpha::INK_QUIET, 0.55),
            (alpha::EDGE_DIM, 0.60),
            (alpha::WASH_CELL, 0.60),
            (alpha::INK_DIM, 0.70),
            (alpha::EDGE_BRACE, 0.75),
            (alpha::EDGE_LIT, 0.80),
            (alpha::WARN_INK, 0.88),
            (alpha::TEXT_BRIGHT, 0.90),
        ] {
            assert_eq!(level, drawn);
        }
        // The two colours that were written out at the paint sites.
        for c in [DARK, LIGHT] {
            assert_eq!(c.sheen, Color32::WHITE.gamma_multiply(0.18));
            assert_eq!(c.key_edge, Color32::from_black_alpha(48));
        }
    }

    #[test]
    fn radii_follow_the_control_height() {
        assert_eq!(radius::for_height(size::CONTROL_SMALL), radius::SMALL);
        assert_eq!(radius::for_height(size::CONTROL), radius::MEDIUM);
        assert_eq!(radius::for_height(size::CONTROL_LARGE), radius::LARGE);
    }

    #[test]
    fn appearance_keys_round_trip() {
        for a in Appearance::ALL {
            assert_eq!(Appearance::from_key(a.key()), a);
        }
        assert_eq!(Appearance::from_key("nonsense"), Appearance::Dark);
    }
}
