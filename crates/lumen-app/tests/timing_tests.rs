//! A test that reads the wall clock can fail on a slow runner for no fault of
//! the code. CI runs the ones whose result depends on timing apart from the
//! others, one at a time and with retries (`.github/timing-tests.txt`,
//! `.github/scripts/ci-test.sh`). This keeps that list from going stale: a
//! test file under `crates/*/tests` that reads the clock must be
//!
//! - part of a test target listed in `.github/timing-tests.txt`, or
//! - an E2E file that takes the `timing_sensitive()` turn, or
//! - named in `KNOWN` below.
//!
//! `KNOWN` is the set of files that read the clock on 2026-10-31 and were not
//! flaky then. It is a snapshot, not a review: most only bound a wait with a
//! generous deadline. When a file in it flakes, move it to the timing list.
//! A new file is not added to `KNOWN` without a look at what it asserts: a
//! deadline of seconds on a wait is harmless, a bound on how long work took
//! is a timing test.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Files that read the clock and are not on the timing list (see above).
const KNOWN: &[&str] = &[
    "crates/lumen-app/tests/e2e/clips.rs",
    "crates/lumen-app/tests/e2e/cue.rs",
    "crates/lumen-app/tests/e2e/dropdown.rs",
    "crates/lumen-app/tests/e2e/live_lights.rs",
    "crates/lumen-app/tests/e2e/main.rs",
    "crates/lumen-app/tests/e2e/measure_latency.rs",
    "crates/lumen-app/tests/e2e/plugins.rs",
    "crates/lumen-app/tests/e2e/pops.rs",
    "crates/lumen-app/tests/e2e/preferences.rs",
    "crates/lumen-app/tests/e2e/record.rs",
    "crates/lumen-app/tests/e2e/streaming.rs",
    "crates/lumen-app/tests/smoke.rs",
    // An ignored measurement: it prints times and asserts none.
    "crates/lumen-engine/tests/midi_scale.rs",
    // Shared by the lumen-hosting tests; the timing ones are listed.
    "crates/lumen-hosting/tests/support/mod.rs",
    "crates/lumen-media/tests/files.rs",
    "crates/lumen-plugin-catalog/tests/scan.rs",
    "crates/lumen-plugin-host/tests/conformance.rs",
    "crates/lumen-plugin-host/tests/editor.rs",
    "crates/lumen-plugin-host/tests/faults.rs",
    "crates/lumen-plugin-host/tests/orphans.rs",
    "crates/lumen-plugin-host/tests/probe.rs",
    "crates/lumen-plugin-host/tests/scan.rs",
    "crates/lumen-plugin-host/tests/shm.rs",
    "crates/lumen-plugin-host/tests/shm_latency.rs",
    "crates/lumen-plugin-host/tests/shm_rt.rs",
    "crates/lumen-test-plugin/tests/faults.rs",
    "crates/lumen-test-plugin/tests/host.rs",
];

/// What the scan counts as reading the wall clock.
const CLOCK: &[&str] = &["Instant::now", "SystemTime::now", ".elapsed()"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, into: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("the folder reads") {
        let path = entry.expect("an entry reads").path();
        if path.is_dir() {
            rust_files(&path, into);
        } else if path.extension().is_some_and(|e| e == "rs") {
            into.push(path);
        }
    }
}

/// The text of a source, lossily: one support file has a stray byte.
fn text(path: &Path) -> String {
    String::from_utf8_lossy(&fs::read(path).expect("a source reads")).into_owned()
}

fn reads_the_clock(source: &str) -> bool {
    CLOCK.iter().any(|call| source.contains(call))
}

/// The lines of the timing list: `(package, target)` for a whole test target
/// and `(package, name)` for unit tests, told apart by the space.
struct Listed {
    targets: BTreeSet<(String, String)>,
    units: Vec<(String, String)>,
}

fn timing_list(list: &str) -> Listed {
    let mut listed = Listed {
        targets: BTreeSet::new(),
        units: Vec::new(),
    };
    for line in list.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some((package, name)) = line.split_once(' ') {
            listed
                .units
                .push((package.to_owned(), name.trim().to_owned()));
        } else {
            let (package, target) = line
                .split_once("::")
                .unwrap_or_else(|| panic!("`{line}` is neither package::target nor package name"));
            listed
                .targets
                .insert((package.to_owned(), target.to_owned()));
        }
    }
    listed
}

/// The test target a file under `crates/<package>/tests` belongs to: its
/// stem, or the folder holding its `main.rs`. `None` for a file that is part
/// of no target by itself (a support module).
fn target_of(relative: &Path) -> Option<String> {
    let mut parts = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy());
    let (_crates, _package, tests) = (parts.next()?, parts.next()?, parts.next()?);
    if tests != "tests" {
        return None;
    }
    let below: Vec<_> = parts.collect();
    match below.as_slice() {
        [file] => Some(file.trim_end_matches(".rs").to_owned()),
        [folder, ..] if folder != "support" => Some(folder.to_string()),
        _ => None,
    }
}

#[test]
fn every_test_file_that_reads_the_clock_is_classified() {
    let root = root();
    let listed = timing_list(
        &fs::read_to_string(root.join(".github/timing-tests.txt")).expect("the list reads"),
    );
    let mut files = Vec::new();
    for krate in fs::read_dir(root.join("crates")).expect("crates read") {
        let tests = krate.expect("an entry reads").path().join("tests");
        if tests.is_dir() {
            rust_files(&tests, &mut files);
        }
    }
    assert!(files.len() > 100, "the scan found the test files");

    let mut unclassified = Vec::new();
    let mut seen_known = BTreeSet::new();
    for file in &files {
        let relative = file.strip_prefix(&root).expect("under the root");
        let name = relative.to_string_lossy().replace('\\', "/");
        let source = text(file);
        if !reads_the_clock(&source) {
            continue;
        }
        let package = relative
            .components()
            .nth(1)
            .expect("a package")
            .as_os_str()
            .to_string_lossy()
            .into_owned();
        let on_the_list = target_of(relative)
            .is_some_and(|target| listed.targets.contains(&(package.clone(), target)));
        if KNOWN.contains(&name.as_str()) {
            assert!(
                !on_the_list,
                "{name} is on the timing list: take it out of KNOWN"
            );
            seen_known.insert(name);
        } else if !on_the_list && !source.contains("timing_sensitive()") {
            unclassified.push(name);
        }
    }
    assert!(
        unclassified.is_empty(),
        "these test files read the wall clock and are not classified. If their \
         result depends on timing, list them in .github/timing-tests.txt; if \
         they only bound a wait with a generous deadline, add them to KNOWN in \
         {}:\n{}",
        file!(),
        unclassified.join("\n")
    );
    let stale: Vec<_> = KNOWN
        .iter()
        .filter(|name| !seen_known.contains(**name))
        .collect();
    assert!(
        stale.is_empty(),
        "these files in KNOWN no longer exist or no longer read the clock: {stale:?}"
    );
}

#[test]
fn every_entry_of_the_timing_list_names_a_test_that_exists() {
    let root = root();
    let listed = timing_list(
        &fs::read_to_string(root.join(".github/timing-tests.txt")).expect("the list reads"),
    );
    assert!(!listed.targets.is_empty(), "the list names test targets");
    for (package, target) in &listed.targets {
        let tests = root.join("crates").join(package).join("tests");
        assert!(
            tests.join(format!("{target}.rs")).is_file()
                || tests.join(target).join("main.rs").is_file(),
            "{package}::{target} is on the timing list but is not a test target"
        );
    }
    for (package, name) in &listed.units {
        // ci-test.sh supports unit tests of lumen-app only.
        assert_eq!(package, "lumen-app", "unit tests of {package}");
        let (module, _test) = name.split_once("::").unwrap_or((name, ""));
        let sources = {
            let mut files = Vec::new();
            rust_files(&root.join("crates").join(package).join("src"), &mut files);
            files
        };
        assert!(
            sources
                .iter()
                .any(|f| f.file_stem().is_some_and(|s| s == module) && text(f).contains("#[test]")),
            "{package} {name} is on the timing list but no module `{module}` has tests"
        );
    }
}

#[test]
fn the_scan_tells_a_test_target_from_a_support_file() {
    let target = |path: &str| target_of(Path::new(path));
    assert_eq!(
        target("crates/lumen-hosting/tests/soak.rs").as_deref(),
        Some("soak")
    );
    assert_eq!(
        target("crates/lumen-app/tests/e2e/clips.rs").as_deref(),
        Some("e2e")
    );
    assert_eq!(target("crates/lumen-hosting/tests/support/mod.rs"), None);
    let list = timing_list("# note\nlumen-hosting::soak\nlumen-app midi_out::sender_tests # why\n");
    assert!(
        list.targets
            .contains(&("lumen-hosting".into(), "soak".into()))
    );
    assert_eq!(
        list.units,
        [("lumen-app".to_owned(), "midi_out::sender_tests".to_owned())]
    );
    assert!(reads_the_clock("let t = Instant::now();"));
    assert!(!reads_the_clock("let t = Duration::from_secs(1);"));
}
