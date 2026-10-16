//! What the browser is showing: the search words, the type and category
//! filters, which vendors are folded and what is selected. Pure state; the
//! rows it makes are `rows.rs`.

use std::collections::HashSet;

/// Which kind of device to show (the owner's "filter on types").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TypeFilter {
    #[default]
    All,
    Instruments,
    Effects,
}

impl TypeFilter {
    pub(crate) fn kind(self) -> Option<lumen_plugin_catalog::catalog::Kind> {
        use lumen_plugin_catalog::catalog::Kind;
        match self {
            TypeFilter::All => None,
            TypeFilter::Instruments => Some(Kind::Instrument),
            TypeFilter::Effects => Some(Kind::Effect),
        }
    }
}

/// The chip chosen under the search box. One at a time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Chip {
    #[default]
    All,
    Favorites,
    Recent,
    Category(String),
}

/// What is selected in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Selection {
    /// A vendor's header, by lower-cased name.
    Vendor(String),
    /// The header of the blocked section.
    BlockedHeader,
    /// A device, by id.
    Device(String),
    /// A blocked module, by path.
    Blocked(String),
}

#[derive(Debug, Default)]
pub(crate) struct View {
    pub text: String,
    pub kind: TypeFilter,
    pub chip: Chip,
    /// Vendors folded shut, lower-cased.
    pub folded: HashSet<String>,
    /// The blocked section is folded shut.
    pub blocked_folded: bool,
    pub selected: Option<Selection>,
}

impl View {
    pub(crate) fn is_folded(&self, vendor: &str) -> bool {
        self.folded.contains(&vendor.to_lowercase())
    }

    /// Folds a vendor shut or opens it; true if it is folded now.
    pub(crate) fn toggle_fold(&mut self, vendor: &str) -> bool {
        let key = vendor.to_lowercase();
        if self.folded.remove(&key) {
            false
        } else {
            self.folded.insert(key);
            true
        }
    }

    /// Whether any filter or search is narrowing the list.
    pub(crate) fn is_filtering(&self) -> bool {
        !self.text.trim().is_empty() || self.kind != TypeFilter::All || self.chip != Chip::All
    }

    /// Back to everything: no words, no filters. Folds and selection stay.
    pub(crate) fn clear_filters(&mut self) {
        self.text.clear();
        self.kind = TypeFilter::All;
        self.chip = Chip::All;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_toggles_case_insensitively() {
        let mut v = View::default();
        assert!(v.toggle_fold("Vendor A"));
        assert!(v.is_folded("vendor a"));
        assert!(!v.toggle_fold("VENDOR A"));
        assert!(!v.is_folded("Vendor A"));
    }

    #[test]
    fn filtering_is_words_a_type_or_a_chip_and_clearing_keeps_folds_and_selection() {
        let mut v = View::default();
        assert!(!v.is_filtering());
        v.text = "  ".into();
        assert!(!v.is_filtering());
        v.kind = TypeFilter::Effects;
        assert!(v.is_filtering());
        v.chip = Chip::Category("Reverb".into());
        v.text = "plate".into();
        v.toggle_fold("A");
        v.selected = Some(Selection::Device("x".into()));
        v.clear_filters();
        assert!(!v.is_filtering());
        assert!(v.is_folded("a"));
        assert!(v.selected.is_some());
    }
}
