#![allow(
    dead_code,
    reason = "each test target that includes this file uses part of it"
)]

pub struct Scene {
    pub test: &'static str,
    pub clips: &'static [&'static str],
    pub shots: &'static [&'static str],
    pub walks: &'static [&'static str],
    pub callouts: &'static [(&'static str, &'static [&'static str])],
}

impl Scene {
    pub fn themed(&self) -> bool {
        !self.clips.is_empty() || !self.shots.is_empty()
    }

    pub fn files(&self, themes: &[&str]) -> std::collections::BTreeSet<String> {
        let mut files = std::collections::BTreeSet::new();
        for theme in themes {
            for c in self.clips {
                files.insert(format!("{c}-{theme}.mp4"));
                files.insert(format!("{c}-{theme}.webp"));
            }
            for s in self.shots {
                files.insert(format!("{s}-{theme}.webp"));
            }
        }
        for w in self.walks {
            for ext in ["mp4", "webp", "captions.vtt", "chapters.vtt"] {
                files.insert(format!("{w}.{ext}"));
            }
        }
        for (shot, _) in self.callouts {
            files.insert(format!("{shot}.callouts.json"));
        }
        files
    }
}

pub const THEMES: &[&str] = &["dark", "light"];

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
    Scene {
        test: "audio_clip_editor",
        clips: &["audio-editor-lead"],
        shots: &["audio-editor-whole", "audio-editor-samples"],
        walks: &[],
        callouts: &[(
            "audio-editor-whole",
            &["The part the clip plays", "The rest of the file"],
        )],
    },
    Scene {
        test: "audio_editor_handles",
        clips: &[
            "audio-editor-handles-lead",
            "audio-editor-trim",
            "audio-editor-fade",
            "audio-editor-fade-shape",
            "audio-editor-gain",
            "audio-editor-slip",
        ],
        shots: &["audio-editor-handles"],
        walks: &[],
        callouts: &[(
            "audio-editor-handles",
            &["Trim grip", "Fade handle", "Gain line"],
        )],
    },
    Scene {
        test: "audio_editor_range",
        clips: &[
            "audio-editor-range-lead",
            "audio-editor-range",
            "audio-editor-cursor",
        ],
        shots: &["audio-editor-range"],
        walks: &[],
        callouts: &[("audio-editor-range", &["Range"])],
    },
    Scene {
        test: "audio_clip_properties",
        clips: &[
            "audio-clip-properties-lead",
            "audio-clip-gain",
            "audio-clip-file",
        ],
        shots: &["audio-clip-properties"],
        walks: &[],
        callouts: &[(
            "audio-clip-properties",
            &["Gain", "Fades", "Processing", "File"],
        )],
    },
    Scene {
        test: "audio_editor_envelopes",
        clips: &[
            "audio-editor-envelopes-lead",
            "audio-editor-envelope-point",
            "audio-editor-envelope-draw",
            "audio-editor-loop-brace",
        ],
        shots: &["audio-editor-envelopes"],
        walks: &[],
        callouts: &[(
            "audio-editor-envelopes",
            &["Gain and Pan", "Envelope", "Loop brace"],
        )],
    },
    Scene {
        test: "audio_editor_hearing",
        clips: &["audio-editor-hearing-lead", "audio-editor-scrub"],
        shots: &["audio-editor-hearing"],
        walks: &[],
        callouts: &[("audio-editor-hearing", &["Loop clip", "Ruler"])],
    },
];

pub fn has_clip(name: &str) -> bool {
    SCENES.iter().any(|s| s.clips.contains(&name))
}

pub fn has_shot(name: &str) -> bool {
    SCENES.iter().any(|s| s.shots.contains(&name))
}

pub fn callouts(shot: &str) -> &'static [&'static str] {
    SCENES
        .iter()
        .flat_map(|s| s.callouts)
        .find(|(name, _)| *name == shot)
        .map_or(&[], |(_, names)| names)
}

pub fn has_walk(name: &str) -> bool {
    SCENES.iter().any(|s| s.walks.contains(&name))
}
