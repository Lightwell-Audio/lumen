//! The product's name lives in `lumen_core::product` (the `lumen-architecture`
//! skill, hard rule 8): text the user reads that names it is built from
//! `PRODUCT_NAME`, and text that cannot be built (a static command
//! description) says "the app". This scans the app's sources for the name
//! spelled out inside a string literal.

use std::fs;
use std::path::{Path, PathBuf};

fn sources(dir: &Path, into: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("the source folder reads") {
        let path = entry.expect("an entry reads").path();
        if path.is_dir() {
            sources(&path, into);
        } else if path.extension().is_some_and(|e| e == "rs") {
            into.push(path);
        }
    }
}

/// The text inside each plain string literal on a line (no raw strings; the
/// app has none that name the product).
fn literals(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut chars = line.chars().peekable();
    let mut current: Option<String> = None;
    while let Some(c) = chars.next() {
        match (&mut current, c) {
            (None, '"') => current = Some(String::new()),
            (None, '/') if chars.peek() == Some(&'/') => break,
            (Some(text), '\\') => {
                text.push(c);
                if let Some(next) = chars.next() {
                    text.push(next);
                }
            }
            (Some(_), '"') => found.extend(current.take()),
            (Some(text), _) => text.push(c),
            (None, _) => {}
        }
    }
    found
}

/// Words that read as the name in a literal: "Lumen" on its own, not part of
/// an identifier such as `lumen-app` (lower case) or `LumenFoo`.
fn names_the_product(text: &str) -> bool {
    text.match_indices("Lumen").any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + 5..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// The product's name spelled out in a literal before a file's tests.
fn spelled_out(source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (number, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        found.extend(
            literals(line)
                .into_iter()
                .filter(|text| names_the_product(text))
                .map(|text| (number + 1, text)),
        );
    }
    found
}

#[test]
fn no_string_the_user_reads_spells_out_the_products_name() {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    assert!(files.len() > 100, "the scan found the sources");
    let mut offenders = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).expect("a source reads");
        for (line, text) in spelled_out(&source) {
            offenders.push(format!("{}:{line}: {text}", file.display()));
        }
    }
    assert!(
        offenders.is_empty(),
        "use PRODUCT_NAME, or say \"the app\":\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_scan_sees_a_name_in_a_literal_and_nothing_else() {
    let source = "\
// \"Lumen\" in a comment
let a = \"Lumen is ready\";
let b = format!(\"{PRODUCT_NAME} is ready\");
let c = \"lumen-app and LumenFoo\";
let d = \"say \\\"hi\\\", Lumen\";
#[cfg(test)]
let e = \"Lumen\";
";
    let found = spelled_out(source);
    assert_eq!(
        found,
        [
            (2, "Lumen is ready".to_owned()),
            (5, "say \\\"hi\\\", Lumen".to_owned()),
        ]
    );
}
