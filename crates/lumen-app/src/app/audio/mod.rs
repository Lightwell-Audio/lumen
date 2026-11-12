//! The app's audio work, by concern (audit M11): the inputs each track
//! listens to, kept in step with the device (`audio_inputs`, M9 slice R1,
//! ADR-0100), recording the armed audio tracks into files and then into clips
//! (`audio_record`, M9 slice R2, ADR-0102), and the Export window with the
//! task behind it (`audio_export`, M9 slice R5). Exporting is the only one of
//! the three that does not touch the device; it lives here because the export
//! engine's own modules are `crate::export` and this is its one app-side
//! file.

pub(crate) mod audio_export;
pub(crate) mod audio_inputs;
pub(crate) mod audio_record;
