//! The design system: colour, type, spacing and shape tokens with the egui
//! style built from them (`theme`), the typefaces (`fonts`), the vector icon
//! set (`icons`), the five levels of light (`light`) and motion (`motion`).
//!
//! Everything the app paints takes its values from here, never inline
//! (sweep M11: one folder per concern).

pub(crate) mod fonts;
pub(crate) mod icons;
pub(crate) mod light;
pub(crate) mod motion;
pub(crate) mod theme;
