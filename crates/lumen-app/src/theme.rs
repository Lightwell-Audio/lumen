//! Colour and size tokens. The start of the design system (spec §5); the full
//! system, with a light theme, arrives in M3. Everything the app paints takes
//! its colours from here.

use eframe::egui::Color32;

/// Arrangement background.
pub const CANVAS: Color32 = Color32::from_rgb(0x1B, 0x1D, 0x22);
pub const RULER: Color32 = Color32::from_rgb(0x23, 0x26, 0x2D);
pub const RULER_TEXT: Color32 = Color32::from_rgb(0x9A, 0xA0, 0xAC);
pub const HINT_TEXT: Color32 = Color32::from_rgb(0x6E, 0x74, 0x80);

/// The single accent colour: active toggles, the playhead, the loop range.
/// A clear blue that stays distinct for the common colour-vision deficiencies.
pub const ACCENT: Color32 = Color32::from_rgb(0x4F, 0xA3, 0xFF);
/// Record is the one semantic exception to the single accent.
pub const RECORD: Color32 = Color32::from_rgb(0xE5, 0x48, 0x4D);
/// Status text for problems (device lost, no device).
pub const WARNING: Color32 = Color32::from_rgb(0xF2, 0xB1, 0x4C);

/// Icon colours on the transport bar.
pub const ICON: Color32 = Color32::from_rgb(0xD0, 0xD4, 0xDC);
pub const ICON_DISABLED: Color32 = Color32::from_rgb(0x5A, 0x5F, 0x6A);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(0x2E, 0x32, 0x3A);
pub const BUTTON_ACTIVE: Color32 = Color32::from_rgb(0x24, 0x3A, 0x55);

/// Where a click on the ruler will move the playhead: the accent, faint.
pub const GHOST: Color32 = Color32::from_rgba_premultiplied(0x28, 0x52, 0x80, 0x80);

/// The loop range on the ruler: the accent, translucent.
pub const LOOP_RANGE: Color32 = Color32::from_rgba_premultiplied(0x18, 0x31, 0x4D, 0x4D);
