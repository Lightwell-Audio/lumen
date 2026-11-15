#[macro_export]
macro_rules! product_name {
    () => {
        "Lumen"
    };
}

pub const PRODUCT_NAME: &str = product_name!();

pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const PROJECT_EXTENSION: &str = "lumen";

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
