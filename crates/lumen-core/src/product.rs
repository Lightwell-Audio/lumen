//! Product identity: the single source of truth for user-visible naming.
//!
//! "Lumen" is a working name. Every user-facing string (window titles, file
//! dialogs, about box, installer metadata, log file names) must be built from
//! these constants so that a rename is a one-line change. Crate names
//! (`lumen-*`) are internal identifiers and never shown to users.

/// The product name as a literal, for text that must be built at compile
/// time (`concat!("Set up ", product_name!(), "…")`, a command's name).
/// [`PRODUCT_NAME`] is made from it, so a rename stays one line.
#[macro_export]
macro_rules! product_name {
    () => {
        "Lumen"
    };
}

/// User-visible product name.
pub const PRODUCT_NAME: &str = product_name!();

/// Product version, taken from the workspace version.
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// File extension (without the dot) of project documents.
///
/// If the product is ever renamed, the old extension must still be accepted
/// when opening files: projects have to open forever.
pub const PROJECT_EXTENSION: &str = "lumen";

/// Display string such as `"Lumen 0.0.1"`.
pub fn display_name() -> String {
    format!("{PRODUCT_NAME} {PRODUCT_VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_combines_name_and_version() {
        assert_eq!(display_name(), format!("{PRODUCT_NAME} {PRODUCT_VERSION}"));
        assert!(!PRODUCT_NAME.is_empty());
    }

    #[test]
    fn the_literal_is_the_name() {
        assert_eq!(
            concat!("Set up ", product_name!()),
            format!("Set up {PRODUCT_NAME}")
        );
    }

    #[test]
    fn project_extension_has_no_dot() {
        assert!(!PROJECT_EXTENSION.starts_with('.'));
    }
}
