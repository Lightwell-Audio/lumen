//! Recording: audio input and its takes on disk, the notes of a MIDI take,
//! the span a recording covers, its settings, automatic record and loop
//! passes (M9 review M4: one folder per concern).

pub(crate) mod audio_input;
pub(crate) mod auto_record;
pub(crate) mod loop_passes;
pub(crate) mod record_settings;
pub(crate) mod record_span;
pub(crate) mod recorder;
pub(crate) mod take_notes;
pub(crate) mod take_worker;
