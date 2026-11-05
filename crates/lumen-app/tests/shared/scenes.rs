//! The docs' scenes (docs/USER_DOCS.md §6.2): each renders the real app
//! into the clips and screenshots it names. `tests/docs_media/` renders
//! them and checks that a scene writes exactly these; `tests/docs_coverage.rs`
//! checks that every clip and shot a page shows is one of them.

#![allow(
    dead_code,
    reason = "each test target that includes this file uses part of it"
)]

/// A scene: the test in `docs_media/` that renders it, and what it writes.
pub struct Scene {
    pub test: &'static str,
    /// Clips: `<name>-<theme>.mp4` with a `.webp` poster.
    pub clips: &'static [&'static str],
    /// Screenshots: `<name>-<theme>.webp`, and `<name>.callouts.json` when
    /// the page marks controls on it.
    pub shots: &'static [&'static str],
    /// Walkthroughs, in the dark theme only: `<name>.mp4` with a `.webp`
    /// poster, captions and chapters.
    pub walks: &'static [&'static str],
    /// The controls marked on a screenshot, by name: what the scene records
    /// in `<shot>.callouts.json`, and what a page may mark on it.
    pub callouts: &'static [(&'static str, &'static [&'static str])],
}

pub const SCENES: &[Scene] = &[
    Scene {
        test: "split_a_clip",
        clips: &["split-lead"],
        shots: &["split-before", "split-after"],
        walks: &[],
        callouts: &[("split-after", &["First half", "Second half"])],
    },
    Scene {
        test: "split_tool",
        clips: &["split-tool"],
        shots: &[],
        walks: &[],
        callouts: &[],
    },
    Scene {
        test: "split_while_playing",
        clips: &[],
        shots: &[],
        walks: &["split-playing"],
        callouts: &[],
    },
];

/// Whether a scene writes the clip `name`.
pub fn has_clip(name: &str) -> bool {
    SCENES.iter().any(|s| s.clips.contains(&name))
}

/// Whether a scene writes the screenshot `name`.
pub fn has_shot(name: &str) -> bool {
    SCENES.iter().any(|s| s.shots.contains(&name))
}

/// The controls a scene marks on the screenshot `shot`.
pub fn callouts(shot: &str) -> &'static [&'static str] {
    SCENES
        .iter()
        .flat_map(|s| s.callouts)
        .find(|(name, _)| *name == shot)
        .map_or(&[], |(_, names)| names)
}

/// Whether a scene writes the walkthrough `name`.
pub fn has_walk(name: &str) -> bool {
    SCENES.iter().any(|s| s.walks.contains(&name))
}
