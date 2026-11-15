use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme, vec2,
};

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

    pub accent: Color32,
    pub accent_muted: Color32,

    pub record: Color32,

    pub record_note: Color32,

    pub warning: Color32,
    pub grid_bar: Color32,
    pub grid_beat: Color32,

    pub grid_sub: Color32,

    pub clip_grid_bar: Color32,
    pub clip_grid_beat: Color32,
    pub clip_grid_sub: Color32,

    pub start_marker: Color32,

    pub lane: Color32,

    pub header: Color32,

    pub selected_row: Color32,

    pub selected_control: Color32,

    pub clip_body: Color32,

    pub well: Color32,
    pub shadow: Color32,

    pub key_white: Color32,
    pub key_black: Color32,
    pub key_text: Color32,
    pub lane_black: Color32,
    pub lane_split: Color32,

    pub core: Color32,

    pub sun: Color32,
    pub sun_core: Color32,

    pub ground: Color32,

    pub control_hi: Color32,

    pub thumb_hi: Color32,
    pub thumb: Color32,

    pub edge_light: Color32,

    pub sheen: Color32,

    pub key_edge: Color32,

    pub shade: Color32,

    pub ember: Color32,

    pub accent_hi: Color32,
    pub accent_lo: Color32,
    pub on_accent: Color32,

    pub mute: Color32,
    pub on_mute: Color32,

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

pub const fn colour_of(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Controls {
    pub fill: Color32,

    pub fill_hi: Color32,
    pub hover: Color32,

    pub press: Color32,

    pub text: Color32,
}

pub fn controls(selected: bool) -> Controls {
    let c = p();
    if selected {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    On,

    Accent,

    Mute,

    Record,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LitFill {
    pub top: Color32,
    pub bottom: Color32,
    pub edge: Color32,
    pub light: Color32,
    pub text: Color32,
}

pub const EDGE_ON_FILL: Color32 = Color32::from_rgba_unmultiplied_const(0xFF, 0xFF, 0xFF, 0x59);

pub const ON_HAIRLINE: f32 = 0.38;

const RECORD_HAIRLINE: f32 = 0.45;

const RECORD_TINT: f32 = 0.18;

impl Tone {
    pub fn glow(self) -> Option<Color32> {
        match self {
            Tone::Record => Some(p().record),
            _ => None,
        }
    }
}

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

pub fn edge_light() -> Color32 {
    p().edge_light
}

pub fn wave(track: Color32) -> Color32 {
    wave_on(LIGHT_IN_USE.load(Ordering::Relaxed), track)
}

pub fn wave_on(light: bool, track: Color32) -> Color32 {
    if light {
        track
    } else {
        track.lerp_to_gamma(Color32::WHITE, 0.3)
    }
}

pub const METER_WARN_DB: f32 = -6.0;

pub const METER_PEAK_ALPHA: f32 = 0.4;

pub const DIMMED: f32 = 0.45;

pub const CLIP_DIMMED: f32 = 0.42;

pub const UNDER_LANE: f32 = 0.45;

pub const CLIP_EDGE: f32 = 0.6;

pub const WAVE_OUTSIDE: f32 = 0.55;
pub const OUTSIDE_WASH: f32 = 0.7;

pub const WARP_POLE: f32 = 0.45;
pub const WARP_POLE_LIT: f32 = 0.8;
pub const WARP_GHOST_FILL: f32 = 0.15;
pub const WARP_GHOST_FREE: f32 = 0.6;
pub const WARP_TICK_BASE: f32 = 0.45;
pub const WARP_TICK_GAIN: f32 = 0.4;

pub const HANDLE_QUIET: f32 = 0.55;

pub const GRIP_QUIET: f32 = 0.7;

pub mod alpha {

    pub const NONE: f32 = 0.0;

    pub const WASH_FAINT: f32 = 0.05;

    pub const WASH_LANE: f32 = 0.06;

    pub const WASH_DROP: f32 = 0.08;

    pub const WASH_WRITE: f32 = 0.09;

    pub const WASH: f32 = 0.10;

    pub const WARN_FILL: f32 = 0.10;

    pub const HALO_FAINT: f32 = 0.10;

    pub const WASH_SELECTED: f32 = 0.12;

    pub const WASH_BAND: f32 = 0.14;

    pub const GLOW: f32 = 0.15;

    pub const WASH_RANGE: f32 = 0.16;

    pub const WASH_OUTLINE: f32 = 0.18;

    pub const HALO: f32 = 0.20;

    pub const WASH_SPAN: f32 = 0.22;

    pub const HALO_WIDE: f32 = 0.22;

    pub const WASH_BODY: f32 = 0.25;

    pub const INK_TRACE: f32 = 0.30;

    pub const WASH_BRACE: f32 = 0.35;

    pub const HOVER_STEP: f32 = 0.35;

    pub const WARN_EDGE: f32 = 0.35;

    pub const EDGE_FAINT: f32 = 0.35;

    pub const EDGE_SOFT: f32 = 0.45;

    pub const EDGE: f32 = 0.50;

    pub const INK_QUIET: f32 = 0.55;

    pub const EDGE_DIM: f32 = 0.60;

    pub const WASH_CELL: f32 = 0.60;

    pub const INK_DIM: f32 = 0.70;

    pub const EDGE_BRACE: f32 = 0.75;

    pub const EDGE_LIT: f32 = 0.80;

    pub const WARN_INK: f32 = 0.88;

    pub const TEXT_BRIGHT: f32 = 0.90;

    pub const STEP_WELL_DARK: [f32; 2] = [1.0, 0.75];
    pub const STEP_WELL_LIGHT: [f32; 2] = [0.5, 0.3];
    pub const STEP_EDGE_ON: f32 = 0.85;
    pub const STEP_FILL_SILENT: f32 = 0.35;
    pub const STEP_EDGE_SILENT: f32 = 0.40;

    pub const STEP_PAST_END: f32 = 0.35;
}

pub mod glow {
    use super::Color32;

    pub fn additive(lit: Color32) -> Color32 {
        Color32::from_rgba_premultiplied(lit.r(), lit.g(), lit.b(), 0)
    }

    pub const STOPS: [f32; 4] = [0.0, 0.35, 0.7, 1.0];
    pub const ALPHAS: [f32; 4] = [1.0, 0.42, 0.10, 0.0];
    pub const SIDES: usize = 24;
}

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

pub fn text_on(fill: Color32) -> Color32 {
    let luma =
        0.2126 * f32::from(fill.r()) + 0.7152 * f32::from(fill.g()) + 0.0722 * f32::from(fill.b());
    if luma > 140.0 {
        TEXT_ON_LIGHT
    } else {
        Color32::WHITE
    }
}

pub fn warning_fill() -> Color32 {
    p().warning.gamma_multiply(0.14)
}

pub fn warning_edge() -> Color32 {
    p().warning.gamma_multiply(0.45)
}

pub const TEXT_ON_LIGHT: Color32 = Color32::from_rgb(0x0B, 0x0D, 0x10);

pub const CLIP_TEXT: Color32 = Color32::WHITE;

pub const CLIP_MUTED: Color32 = Color32::from_rgb(0x6C, 0x70, 0x78);

pub mod personality {
    use super::Color32;

    pub const UTILITY: Color32 = Color32::from_rgb(0x9A, 0xA0, 0xAC);

    pub const SYNTH: Color32 = Color32::from_rgb(0xFF, 0xC2, 0x3D);

    #[allow(dead_code)]
    pub const SYNTH_DEEP: Color32 = Color32::from_rgb(0x11, 0x0D, 0x05);

    pub const EQ: Color32 = Color32::from_rgb(0x7C, 0x8C, 0xFF);

    pub const EQ_SWEEP: [Color32; 5] = [
        Color32::from_rgb(0xFF, 0x5A, 0x6E),
        Color32::from_rgb(0xFF, 0xB5, 0x47),
        Color32::from_rgb(0x5B, 0xE0, 0x7F),
        Color32::from_rgb(0x4F, 0xA3, 0xFF),
        Color32::from_rgb(0xB0, 0x7C, 0xFF),
    ];

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

    pub const DELAY: Color32 = Color32::from_rgb(0x3F, 0xD6, 0xC6);

    pub const REVERB: Color32 = Color32::from_rgb(0xFF, 0x6F, 0xA8);

    pub const DELAY_DEEP: Color32 = Color32::from_rgb(0x07, 0x10, 0x0F);
    pub const REVERB_DEEP: Color32 = Color32::from_rgb(0x12, 0x09, 0x10);

    pub const REVERB_CORE: Color32 = Color32::from_rgb(0xFF, 0xF1, 0xD6);
    pub const REVERB_WARM: Color32 = Color32::from_rgb(0xFF, 0xC0, 0x8A);

    pub const SHUTTER: Color32 = Color32::from_rgb(0x6F, 0xA8, 0xFF);

    #[allow(dead_code, reason = "read by Shutter's face when it is built")]
    pub const SHUTTER_DEEP: Color32 = Color32::from_rgb(0x1F, 0x6F, 0xD1);

    pub const GRAVITY: Color32 = Color32::from_rgb(0xB4, 0x6C, 0xFF);

    pub const HORIZON: Color32 = Color32::from_rgb(0xFF, 0x8A, 0x4C);

    pub const GRAVITY_DEEP: Color32 = Color32::from_rgb(0x0B, 0x09, 0x12);
    pub const GRAVITY_ORB: [Color32; 2] = [
        Color32::from_rgb(0xFF, 0xFF, 0xFF),
        Color32::from_rgb(0xE9, 0xD4, 0xFF),
    ];

    pub const HORIZON_DEEP: Color32 = Color32::from_rgb(0x0E, 0x08, 0x10);
    pub const HORIZON_LINE: Color32 = Color32::from_rgb(0xFF, 0xC4, 0x9C);
    pub const HORIZON_WORDS: Color32 = Color32::from_rgb(0xFF, 0xD9, 0xBF);
    pub const HORIZON_GAP: [Color32; 2] = [
        Color32::from_rgb(0xFF, 0xEE, 0xDC),
        Color32::from_rgb(0xFF, 0xAA, 0x6E),
    ];
    pub const HORIZON_GAP_ALPHA: [f32; 2] = [0.85, 0.35];
    pub const HORIZON_PLATE: Color32 = Color32::from_rgb(0x08, 0x05, 0x03);
    pub const HORIZON_PLATE_ALPHA: f32 = 0.42;

    pub const SCREEN: Color32 = Color32::from_rgb(0x0C, 0x0E, 0x12);

    pub const SCREEN_TINT: f32 = 0.20;

    pub const PRISM_FAN: f32 = 0.12;

    pub const READOUT_SIZE: f32 = 9.0;
    pub const READOUT_INSET: f32 = 10.0;

    pub mod card {
        use eframe::egui::Color32;

        pub const RADIUS: f32 = 14.0;

        pub const HEADER_HEIGHT: f32 = 53.0;
        pub const HEADER_TINT: f32 = 0.30;

        pub const POWER_X: f32 = 22.0;
        pub const POWER_Y: f32 = 27.0;
        pub const POWER_RADIUS: f32 = 9.0;
        pub const POWER_FILL: f32 = 0.22;
        pub const POWER_RING: f32 = 0.8;

        pub const NAME_SIZE: f32 = 15.0;
        pub const NAME_X: f32 = 40.0;
        pub const NAME_BASELINE: f32 = 27.0;
        pub const NAME_SPACING: f32 = 0.4;
        pub const SUBTITLE_SIZE: f32 = 11.0;
        pub const SUBTITLE_BASELINE: f32 = 43.0;

        pub const PRESET_SIZE_PT: (f32, f32) = (54.0, 20.0);
        pub const PRESET_INSET: f32 = 12.0;
        pub const PRESET_TOP: f32 = 16.0;
        pub const PRESET_FILL: f32 = 0.16;
        pub const PRESET_RING: f32 = 0.40;
        pub const PRESET_TEXT: f32 = 11.0;

        pub const SCREEN_INSET: f32 = 14.0;
        pub const SCREEN_TOP: f32 = 60.0;
        pub const SCREEN_RADIUS: f32 = 10.0;
        pub const SCREEN_RING: f32 = 0.35;

        pub const SCREEN_GLOW: f32 = 0.20;
        pub const SCREEN_REACH: f32 = 1.1;

        pub const DIAL_RADIUS: f32 = 14.0;
        pub const DIAL_TRACK: f32 = 3.0;
        pub const DIAL_TRACK_ALPHA: f32 = 0.22;
        pub const DIAL_LIT: f32 = 2.5;
        pub const DIAL_GLOW: [(f32, f32); 2] = [(8.0, 0.12), (5.5, 0.26)];

        pub const DIAL_HELD_GROWTH: f32 = 0.6;
        pub const DIAL_HEAD: f32 = 3.0;
        pub const DIAL_POINTER: f32 = 2.0;
        pub const DIAL_POINTER_INSET: f32 = 9.0;
        pub const DIAL_DISC_INSET: f32 = 4.0;

        pub const DIAL_NAME_BASELINE: f32 = 14.0;
        pub const DIAL_VALUE_BASELINE: f32 = 26.0;
        pub const DIAL_NAME_SIZE: f32 = 10.5;
        pub const DIAL_VALUE_SIZE: f32 = 9.0;

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

        pub const PILL_ON_FILL: f32 = 0.18;
        pub const PILL_OFF_FILL: f32 = 0.08;
        pub const PILL_OFF_RING: f32 = 0.40;
        pub const PILL_TEXT: f32 = 10.0;

        pub const SCREEN_NAME_ALPHA: f32 = 0.62;

        pub const ALL_TEXT: f32 = 10.0;
        pub const ALL_INSET: f32 = 14.0;
        pub const ALL_FROM_BOTTOM: f32 = 12.0;

        pub const SHORT_BOTTOM: f32 = 26.0;
        pub const SHORT_DIALS_BAND: f32 = 84.0;
        pub const SHORT_DIAL_FROM_FOOT: f32 = 56.0;

        pub const SHORT_PRESET_TOP: f32 = 13.0;

        pub const SHORT_NAME_SIZE: f32 = 9.5;
        pub const SHORT_VALUE_SIZE: f32 = 8.5;
        pub const SHORT_NAME_BASELINE: f32 = 12.0;
        pub const SHORT_VALUE_BASELINE: f32 = 23.0;

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

        pub const RAIL_BAR: (f32, f32) = (14.0, 3.0);
        pub const RAIL_NAME_X: f32 = 24.0;
        pub const RAIL_NAME_SIZE: f32 = 13.0;
        pub const RAIL_BAR_REST: f32 = 0.55;

        pub const LEVEL_FLOOR_DB: f32 = -60.0;

        pub const HANDLE_RING: f32 = 6.0;

        pub const HANDLE_RING_ALPHA: f32 = 0.6;

        pub const CHIP_HEIGHT: f32 = 18.0;
        pub const CHIP_TEXT: f32 = 9.0;

        pub const KEY_HEIGHT: [f32; 3] = [20.0, 18.0, 14.0];
        pub const KEY_TEXT: [f32; 3] = [10.0, 9.0, 8.0];
        pub const KEY_LEFT: [f32; 3] = [12.0, 8.0, 6.0];
        pub const KEY_FOOT: [f32; 3] = [10.0, 6.0, 4.0];
        pub const KEY_PAD: f32 = 10.0;
        pub const KEY_RING: f32 = 0.6;
        pub const KEY_WORDS: f32 = 0.85;
        pub const KEY_WORDS_NONE: f32 = 0.5;
        pub const KEY_LAMP: f32 = 2.5;

        pub const PART_REACH: f32 = 11.0;
        pub const PART_FINE: f32 = 0.1;
        pub const PART_PILL_REST: f32 = 0.10;
        pub const PART_PILL_PICKED: f32 = 0.35;
        pub const PART_PILL_OFF: f32 = 0.35;
        pub const PART_PILL_OFF_TEXT: f32 = 0.5;
        pub const PART_PILL_TEXT_SHARE: f32 = 0.5;
    }

    pub const PRISM_BEAMS: [Color32; 5] = [
        Color32::from_rgb(0xFF, 0x5A, 0x6E),
        Color32::from_rgb(0xFF, 0xB5, 0x47),
        Color32::from_rgb(0x5B, 0xE0, 0x7F),
        Color32::from_rgb(0x4F, 0xA3, 0xFF),
        Color32::from_rgb(0xB0, 0x7C, 0xFF),
    ];
}

pub mod space {

    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;

    pub const L: f32 = 16.0;

    pub const XL: f32 = 24.0;
}

pub mod radius {

    pub const MARK: u8 = 1;

    pub const SMALL: u8 = 4;

    pub const MEDIUM: u8 = 6;

    pub const LARGE: u8 = 8;

    pub const XL: u8 = 12;

    pub const FULL: u8 = 255;

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

pub const PICTURE_OFF: Color32 = Color32::from_gray(110);

pub fn grey_of(colour: Color32) -> Color32 {
    Color32::from_gray(
        ((u16::from(colour.r()) + u16::from(colour.g()) + u16::from(colour.b())) / 3) as u8,
    )
}

pub mod stroke {

    pub const HAIRLINE: f32 = 1.0;

    pub const RING: f32 = 1.2;

    pub const GLYPH: f32 = 1.25;

    pub const GLYPH_WIDE: f32 = 1.4;

    pub const LIT: f32 = 1.5;

    pub const FOCUS: f32 = 2.0;

    pub const UNDERLINE: f32 = 2.0;

    pub const EDGE: f32 = 2.0;

    pub const HALO: f32 = 3.0;

    pub const GLOW_WIDE: f32 = 6.0;

    pub const ICON: f32 = 1.6;
    pub const ICON_FINE: f32 = 1.3;
    pub const ICON_BOLD: f32 = 2.4;
    pub const ICON_HEAVY: f32 = 2.2;
    pub const ICON_STRIKE: f32 = 1.8;
}

pub mod size {

    pub const SCALE: f32 = 10.0;

    pub const MONO_SMALL: f32 = 10.0;

    pub const UNIT: f32 = 9.0;
    pub const CAPTION: f32 = 11.0;

    pub const KEYCAP: f32 = CAPTION;
    pub const BODY: f32 = 13.0;
    pub const TITLE: f32 = 15.0;
    pub const HEADING: f32 = 16.0;

    pub const DISPLAY: f32 = 24.0;

    pub const READOUT: f32 = 20.0;

    pub const WORDMARK: f32 = 14.0;

    pub const INFO_LINE: f32 = 32.0;

    pub const CONTROL: f32 = 24.0;

    pub const CONTROL_SMALL: f32 = 20.0;

    pub const CONTROL_LARGE: f32 = 30.0;

    pub const TRANSPORT_ICON: f32 = 28.0;

    pub const TRANSPORT_BAR: f32 = 44.0;

    pub const TRACK_STEP: f32 = 22.0;

    pub const HEADER_COLUMN: f32 = 204.0;

    pub const BADGE_W: f32 = 16.0;
    pub const BADGE_H: f32 = 14.0;

    pub const HEADER_SIDE: f32 = 60.0;

    pub const WELL_PIECE_W: f32 = 18.0;

    pub const STRIP_PIECE_W: f32 = 22.0;

    pub const WELL_CELL: f32 = CONTROL_LARGE - 2.0 * super::space::XXS;

    pub const CORNER_ICON: f32 = CAPTION + 3.0;

    pub const TIP_WIDTH: f32 = 280.0;

    pub const LEVEL_STRIP: f32 = 5.0;

    pub const SEND_SLOT: f32 = 18.0;
    pub const SEND_SLOT_LETTER: f32 = 12.0;

    pub const KNOB: f32 = 24.0;

    pub const KNOB_SMALL: f32 = 18.0;

    pub const LABEL_COLUMN: f32 = 96.0;

    pub const LABEL_COLUMN_NARROW: f32 = 72.0;

    pub const PREFERENCES: f32 = 640.0;
    pub const PREFERENCES_FOLD: f32 = 560.0;
    pub const PREFERENCES_LEAST: f32 = 280.0;
    pub const PAGE_LIST: f32 = 172.0;

    pub const PREFERENCES_TOP: f32 = 72.0;
    pub const PREFERENCES_BELOW: f32 = 40.0;
    pub const PREFERENCES_LEAST_PAGE: f32 = 80.0;
    pub const PREFERENCES_SLIDER: f32 = 160.0;

    pub const WELCOME: f32 = 560.0;
    pub const WELCOME_NARROW: f32 = 480.0;
    pub const LUMI: f32 = 84.0;
    pub const SPEAKER_LIGHT: f32 = 20.0;
    pub const PROGRESS_BAR: f32 = 3.0;
    pub const WRAP_LEAST: f32 = 120.0;

    pub const TAB_DIVIDER: f32 = 14.0;

    pub const SUBJECT_NAME: f32 = 160.0;
    pub const SUBJECT_BAR: f32 = 3.0;

    pub const TAB_NOTE: f32 = 140.0;

    pub const LANE_ROW: f32 = 2.0 * super::space::M + super::space::XS;
    pub const LANE_NAME: f32 = 176.0;
    pub const LANE_BAR: f32 = 3.0;
    pub const LANE_ICON: f32 = 16.0;
    pub const LANE_ICON_X: f32 = 10.0;
    pub const LANE_NAME_X: f32 = 34.0;
    pub const LANE_BUTTON_W: f32 = 20.0;
    pub const LANE_BUTTON_H: f32 = 18.0;
    pub const LANE_BUTTON_GAP: f32 = 4.0;
    pub const LANE_BUTTONS_RIGHT: f32 = 8.0;
    pub const LANE_PICKED_RING: f32 = 2.0;
    pub const LANE_HIT_ROW: f32 = 0.34;
    pub const LANE_HIT_STEP: f32 = 0.46;

    pub const DRUM_ROW_MIN: f32 = 6.0;

    pub const STEP_ROW: f32 = 34.0;
    pub const STEP_INSET: f32 = 3.0;
    pub const STEP_STRIP: f32 = 34.0;
    pub const STEP_RIM: f32 = 1.5;
    pub const STEP_FILL_LEAST: f32 = 6.0;
    pub const STEP_LIGHT: f32 = 3.0;
    pub const STEP_MARK: f32 = 9.0;

    pub const WARP_MODE: f32 = 120.0;
    pub const WARP_FIELD: f32 = 88.0;
    pub const WARP_SLIDER: f32 = 112.0;

    pub const EDITOR_RULER: f32 = 37.0;
    pub const BRACE_BAND: f32 = 10.0;
    pub const BRACE_GRAB: f32 = 6.0;
    pub const BRACE_FLAG: f32 = 5.0;
    pub const CURSOR_CAP: f32 = 4.0;

    pub const WARP_STRIP: f32 = 18.0;
    pub const WARP_GRAB: f32 = 6.0;
    pub const WARP_NEAR: f32 = 12.0;
    pub const WARP_NOTCH_W: f32 = 9.0;
    pub const WARP_NOTCH_H: f32 = 12.0;

    pub const WARP_NOTCH_INSET: f32 = 2.0;
    pub const WARP_NOTCH_RADIUS: f32 = 2.5;
    pub const WARP_NOTCH_TAPER: f32 = 4.5;
    pub const WARP_TICK_W: f32 = 6.0;
    pub const WARP_TICK_H: f32 = 5.0;

    pub const WARP_TICK_FLOOR: f32 = 3.0;

    pub const BAR_NUMBER_ROOM: f32 = 32.0;

    pub const TIME_LABEL_ROOM: f32 = 64.0;

    pub const WAVE_SPLIT: f32 = 96.0;

    pub const RULER_BAND: f32 = 3.0;
    pub const BEAT_TICK: f32 = 5.0;
    pub const TIME_TICK: f32 = 6.0;
    pub const SAMPLE_DOT: f32 = 2.0;

    pub const EDITOR_STRIP: f32 = 32.0;
    pub const STRIP_GAIN_LEAST: f32 = 56.0;
    pub const STRIP_GAIN: f32 = 104.0;
    pub const STRIP_FADE: f32 = 76.0;
    pub const STRIP_SHAPE: f32 = 70.0;
    pub const STRIP_FILE: f32 = 150.0;

    pub const GRIP_WIDTH: f32 = 4.0;
    pub const GRIP_HEIGHT: f32 = 22.0;

    pub const EDGE_SCROLL: f32 = 36.0;

    pub const SHADOW_ROOM: f32 = 24.0;

    pub const CARD_INSET: f32 = 6.0;

    pub const CAPTION_LINE: f32 = 14.0;

    pub const KNOB_NAME_LINE: f32 = 13.0;
    pub const KNOB_NAME_GAP: f32 = 3.0;

    pub const MENU_ROW: f32 = 32.0;

    pub const MENU_ICON: f32 = 16.0;

    pub const BUTTON_ICON: f32 = 16.0;
    pub const BUTTON_ICON_SMALL: f32 = 14.0;

    pub fn button_icon(height: f32) -> f32 {
        if height < CONTROL {
            BUTTON_ICON_SMALL
        } else {
            BUTTON_ICON
        }
    }

    pub const MENU_LIST: f32 = 320.0;

    pub const BUDGET_BAR: f32 = 18.0;
    pub const BUDGET_SWATCH: f32 = 10.0;
    pub const BUDGET_LIST: f32 = 216.0;

    pub const AUTO_POINT: f32 = 4.0;

    pub const AUTO_BEND: f32 = 3.5;
    pub const AUTO_GRAB: f32 = 9.0;
    pub const AUTO_PAD: f32 = 8.0;
    pub const AUTO_STEP: f32 = 6.0;

    pub const AUTO_SNAP: f32 = 4.0;

    pub const AUTO_CHIP_PAD: f32 = 28.0;
    pub const AUTO_CHIP_MIN: f32 = 64.0;
    pub const AUTO_CHIP_MAX: f32 = 200.0;

    pub const AUTO_REENABLE: f32 = 84.0;

    pub const AUTO_REC_DOT: f32 = 3.5;

    pub const STATUS_DOT: f32 = 4.0;

    pub const AUTO_MENU: f32 = 220.0;

    pub const SONG_TEMPO: f32 = 44.0;
    pub const SONG_METER: f32 = 20.0;
    pub const SONG_CHIP_PAD: f32 = 6.0;
    pub const SONG_TYPE: f32 = 88.0;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    Dark,
    Light,

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

pub fn is_light() -> bool {
    LIGHT_IN_USE.load(Ordering::Relaxed)
}

pub fn p() -> &'static Palette {
    if LIGHT_IN_USE.load(Ordering::Relaxed) {
        &LIGHT
    } else {
        &DARK
    }
}

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

    let radius = CornerRadius::same(radius::MEDIUM);
    let w = &mut v.widgets;

    w.noninteractive.bg_fill = c.app;
    w.noninteractive.weak_bg_fill = c.app;
    w.noninteractive.bg_stroke = Stroke::new(stroke::HAIRLINE, c.border);
    w.noninteractive.fg_stroke = Stroke::new(stroke::HAIRLINE, c.text);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = c.control_press;
    w.inactive.weak_bg_fill = c.control;

    w.inactive.bg_stroke = Stroke::new(stroke::HAIRLINE, c.border);
    w.inactive.fg_stroke = Stroke::new(stroke::HAIRLINE, c.text);
    w.inactive.corner_radius = radius;

    w.hovered.bg_fill = c.control_press.blend(c.ember);
    w.hovered.weak_bg_fill = c.control.blend(c.ember);
    w.hovered.bg_stroke = Stroke::new(stroke::HAIRLINE, c.border_strong);
    w.hovered.fg_stroke = Stroke::new(stroke::HAIRLINE, c.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = c.control_press;
    w.active.weak_bg_fill = c.control_press;
    w.active.bg_stroke = Stroke::new(stroke::FOCUS, c.accent);
    w.active.fg_stroke = Stroke::new(stroke::HAIRLINE, c.text);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

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
        fn over(wash: Color32, body: Color32) -> Color32 {
            let a = f32::from(wash.a()) / 255.0;
            let mix =
                |w: u8, b: u8| (f32::from(w) + f32::from(b) * (1.0 - a)).round().min(255.0) as u8;
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

            assert!(
                c.record_note.r() > c.record_note.g() && c.record_note.r() > c.record_note.b(),
                "{name} note red hue"
            );

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

            let on = lit_in(&c, Tone::On);
            assert!(contrast(on.text, on.top) >= 3.5, "{name} on");

            let rec = lit_in(&c, Tone::Record);
            assert!(contrast(rec.text, rec.top) >= 2.5, "{name} record");

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

            assert!(contrast(c.text_secondary, c.control_hi) >= 4.0, "{name}");
        }
        assert!(luma(DARK.ground) < luma(DARK.well));
    }

    #[test]
    fn the_opacity_levels_keep_the_values_the_interface_drew() {
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
