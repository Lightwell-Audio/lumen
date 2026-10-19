//! The Devices tab and the browser take colours, line widths and timings from
//! the design system (`theme`, `motion`), never inline (the `lumen-design`
//! skill, rule 1; M6 review L2). This scans their sources for the literals
//! that were found and fixed, before the tests of each file.

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

/// What may not appear in these folders' code.
const FORBIDDEN: [&str; 7] = [
    "Color32::from_gray(",
    "Color32::from_rgb(",
    "Color32::from_rgba_",
    "Stroke::new(1.0",
    "Stroke::new(1.5",
    "Stroke::new(2.0",
    "Duration::from_millis(",
];

#[test]
fn the_devices_tab_and_the_browser_use_tokens_for_colours_lines_and_timings() {
    let mut files = Vec::new();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for folder in ["chain", "browser"] {
        sources(&src.join(folder), &mut files);
    }
    assert!(files.len() > 20, "the scan found the sources");
    let mut offenders = Vec::new();
    for file in files {
        let source = fs::read_to_string(&file).expect("a source reads");
        for (number, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("#[cfg(test)]") {
                break;
            }
            if trimmed.starts_with("//") {
                continue;
            }
            // A "Stroke::new(1.0" followed by a token path is not a literal;
            // the forbidden spellings are the number itself.
            for word in FORBIDDEN {
                if line.contains(word) {
                    offenders.push(format!("{}:{}: {}", file.display(), number + 1, trimmed));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "use `theme` and `motion` tokens:\n{}",
        offenders.join("\n")
    );
}
