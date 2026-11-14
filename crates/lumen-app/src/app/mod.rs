//! The application's behaviour, by concern. `App` itself, the frame loop and
//! start-up stay in `main.rs`; these modules hold its `impl` blocks (audit
//! M1).

pub(crate) mod association;
pub(crate) mod audio;
pub(crate) mod audio_editor;
pub(crate) mod auto;
mod bench_callback;
pub(crate) mod browser;
pub(crate) mod cache_folder;
pub(crate) mod capture_midi;
pub(crate) mod clip;
pub(crate) mod clipboard_keys;
mod commands;
mod controller_requests;
pub(crate) mod controllers;
pub(crate) mod copies;
mod cue;
pub(crate) mod detail;
mod draw_clip;
pub(crate) mod drum_choice;
pub(crate) mod drum_map_window;
pub(crate) mod editing;
pub(crate) mod follow_detect;
pub(crate) mod grid;
mod handles;
pub(crate) mod handover;
pub(crate) mod hardware;
pub(crate) mod history;
pub(crate) mod info;
pub(crate) mod keyboard;
pub(crate) mod locators;
mod mappings_window;
pub(crate) mod media_pool;
pub(crate) mod midi_files;
mod mixer;
pub(crate) mod navigate;
pub(crate) mod palette;
pub(crate) mod panes;
pub(crate) mod plugin_edits;
pub(crate) mod preferences;
pub(crate) mod presets;
pub(crate) mod project_settings;
pub(crate) mod projects;
pub(crate) mod record;
pub(crate) mod recovery;
pub(crate) mod regions;
pub(crate) mod revert;
pub(crate) mod rulers;
pub(crate) mod safe_mode;
pub(crate) mod sampler;
pub(crate) mod scrub;
pub(crate) mod shortcuts;
mod song_lanes;
mod startup;
pub(crate) mod streaming;
pub(crate) mod tap_tempo;
pub(crate) mod templates;
pub(crate) mod test_modes;
pub(crate) mod tools;
mod tools_gestures;
pub(crate) mod track_freeze;
mod track_input;
pub(crate) mod view;
pub(crate) mod welcome;
mod window_size;

// The paths the rest of the app names as `crate::app::<name>`, kept now that
// these files sit in folders (audit M11); the modules nothing names by path
// are reached inside their folder.
pub(crate) use audio::{audio_export, audio_inputs, audio_record};
pub(crate) use auto::{auto_keys, auto_recording};
pub(crate) use clip::{
    clip_bounce, clip_card, clip_commands, clip_consolidate, clip_controls, clip_file, clip_header,
    clip_normalize, clip_warp,
};
