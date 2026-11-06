//! The crates depend on each other one way only (the `lumen-architecture`
//! skill, hard rule 7; `docs/ARCHITECTURE.md`, "Layering"; M9 review M2).
//!
//! Each crate in `crates/` may use only the Lumen crates its row below
//! allows, through its normal and build dependencies. Dev-dependencies are
//! free (a test may use a crate above it), since they never ship. A crate
//! that is not listed fails the test, so a new crate is given a place in the
//! layers when it is added.

use std::fs;
use std::path::Path;

/// Each crate and the Lumen crates it may depend on. `*` is any crate but
/// the tools and test fixtures (`lumen-perf`, `lumen-test-plugin`,
/// `lumen-testkit`).
const ALLOWED: &[(&str, &[&str])] = &[
    // The bottom: nothing of Lumen's.
    ("lumen-core", &[]),
    ("lumen-brand", &[]),
    ("lumen-rt", &[]),
    // Stand alone, but for the real-time tooling (the fake device's thread
    // asks for an audio thread's scheduling, as a driver's has).
    ("lumen-audio", &["lumen-rt"]),
    ("lumen-media", &[]),
    ("lumen-midi", &[]),
    ("lumen-render", &["lumen-shader-check"]),
    ("lumen-shader-check", &[]),
    ("lumen-test-plugin", &[]),
    // Test support (waits), for dev-dependencies only.
    ("lumen-testkit", &[]),
    // On the core.
    ("lumen-devices", &["lumen-core"]),
    ("lumen-project", &["lumen-core"]),
    ("lumen-engine", &["lumen-core", "lumen-rt", "lumen-devices"]),
    // Plugins: the child process, its catalogue, and hosting.
    ("lumen-plugin-host", &["lumen-rt"]),
    ("lumen-plugin-catalog", &["lumen-plugin-host"]),
    (
        "lumen-hosting",
        &["lumen-core", "lumen-devices", "lumen-plugin-host"],
    ),
    // The performance gate's tool.
    (
        "lumen-perf",
        &[
            "lumen-core",
            "lumen-devices",
            "lumen-engine",
            "lumen-project",
        ],
    ),
    // Only the app joins them.
    ("lumen-app", &["*"]),
];

/// The Lumen crates a manifest names in its normal and build dependencies.
fn lumen_dependencies(manifest: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut counted = false;
    for line in manifest.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[') {
            let header = header.trim_end_matches(']');
            let dev = header.contains("dev-dependencies");
            // `[target.'cfg(..)'.dependencies.name]` names one dependency.
            if let Some((section, name)) = header.rsplit_once("dependencies.") {
                counted = false;
                if !dev && !section.is_empty() && name.starts_with("lumen-") {
                    found.push(name.to_owned());
                }
            } else {
                counted = header.ends_with("dependencies") && !dev;
            }
            continue;
        }
        if !counted {
            continue;
        }
        let name = line
            .split(|c: char| c == '=' || c == '.' || c.is_whitespace())
            .next()
            .unwrap_or("");
        if name.starts_with("lumen-") {
            found.push(name.to_owned());
        }
    }
    found
}

#[test]
fn the_crates_depend_on_each_other_one_way() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut broken = Vec::new();
    let mut seen = 0;
    for entry in fs::read_dir(&crates).expect("the crates folder reads") {
        let dir = entry.expect("an entry reads").path();
        let Ok(manifest) = fs::read_to_string(dir.join("Cargo.toml")) else {
            continue;
        };
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .expect("a crate folder has a name")
            .to_owned();
        seen += 1;
        let Some((_, allowed)) = ALLOWED.iter().find(|(c, _)| *c == name) else {
            broken.push(format!("{name} has no place in the layers (ALLOWED)"));
            continue;
        };
        for dependency in lumen_dependencies(&manifest) {
            let ok = if allowed == &["*"] {
                dependency != "lumen-perf"
                    && dependency != "lumen-test-plugin"
                    && dependency != "lumen-testkit"
            } else {
                allowed.contains(&dependency.as_str())
            };
            if !ok {
                broken.push(format!("{name} depends on {dependency}"));
            }
        }
    }
    assert!(seen >= ALLOWED.len(), "every listed crate was read");
    assert!(broken.is_empty(), "layering broken:\n{}", broken.join("\n"));
}

#[test]
fn a_manifest_is_read_for_its_lumen_dependencies() {
    let manifest = r#"
[package]
name = "x"

[dependencies]
lumen-core.workspace = true
lumen-rt = { path = "../lumen-rt" }
serde.workspace = true

[target.'cfg(windows)'.dependencies]
lumen-devices.workspace = true

[target.'cfg(windows)'.dependencies.lumen-media]
path = "../lumen-media"

[build-dependencies]
lumen-brand.workspace = true

[dev-dependencies]
lumen-engine.workspace = true

[target.'cfg(windows)'.dev-dependencies.lumen-audio]
path = "../lumen-audio"
"#;
    assert_eq!(
        lumen_dependencies(manifest),
        [
            "lumen-core",
            "lumen-rt",
            "lumen-devices",
            "lumen-media",
            "lumen-brand"
        ]
    );
}
