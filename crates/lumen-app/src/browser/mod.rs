//! The browser's Devices tab (M6 slice 1, ADR-0068): installed plugins and
//! Lumen's own devices, grouped by vendor, narrowed by type, category and
//! words (the owner's choice B with a type filter). `view` is what is being
//! shown, `rows` turns the catalog into rows, `list` paints them, `controls`
//! are the search field, the switch, the chips and the scan line, and
//! `panel` puts them in the pane and applies what was done.

mod controls;
mod list;
mod panel;
pub(crate) mod rows;
pub(crate) mod view;

/// The browser's own state: what it shows, and one-frame requests from
/// commands.
#[derive(Default)]
pub(crate) struct Browser {
    pub view: view::View,
    /// A command asked for the search field's keyboard.
    pub focus_search: bool,
}
