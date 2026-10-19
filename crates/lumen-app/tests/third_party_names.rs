//! The owner's naming rule (CLAUDE.md, "Documentation naming rule") reaches
//! the code: a comment, a test name or a label the user reads does not use
//! another digital audio workstation as a reference ("as in X"). State the
//! behavior itself. Hardware, driver, platform and legal names are allowed
//! and are not on this list. This scans every crate's Rust sources.

use std::fs;
use std::path::{Path, PathBuf};

/// Names of other workstations, matched without regard to case, with a
/// space, underscore or hyphen between words (`pro tools`, `pro_tools`).
const NAMES: &[&str] = &[
    "ableton",
    "cubase",
    "nuendo",
    "logic pro",
    "pro tools",
    "reaper",
    "fl studio",
    "studio one",
    "bitwig",
    "garageband",
];

fn sources(dir: &Path, into: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("the folder reads") {
        let path = entry.expect("an entry reads").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                sources(&path, into);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            into.push(path);
        }
    }
}

/// The names a line uses.
fn names_in(line: &str) -> Vec<&'static str> {
    let flat: String = line
        .to_lowercase()
        .chars()
        .map(|c| if matches!(c, '_' | '-') { ' ' } else { c })
        .collect();
    NAMES
        .iter()
        .copied()
        .filter(|name| flat.contains(name))
        .collect()
}

#[test]
fn no_source_names_another_workstation() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    sources(&crates, &mut files);
    let this = Path::new(file!())
        .file_name()
        .expect("this file has a name")
        .to_owned();
    files.retain(|f| f.file_name() != Some(&this));
    assert!(files.len() > 300, "the scan found the sources");
    let mut offenders = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).expect("a source reads");
        for (number, line) in source.lines().enumerate() {
            for name in names_in(line) {
                offenders.push(format!("{}:{}: {name}", file.display(), number + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "describe the behavior instead of naming another product:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_scan_sees_a_name_in_any_spelling() {
    assert_eq!(names_in("// as in Ableton"), ["ableton"]);
    assert_eq!(names_in("fn works_like_pro_tools() {}"), ["pro tools"]);
    assert_eq!(names_in("\"Keymap: Pro-Tools\""), ["pro tools"]);
    assert!(names_in("// the steps are tools for a project").is_empty());
}
