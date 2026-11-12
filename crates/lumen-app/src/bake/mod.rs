//! Turning edits into audio: the worker behind Bounce and freeze (`bounce`),
//! Consolidate's render (`consolidate`), the peak scan behind Normalize
//! (`normalize`) and the crossfades between audio clips that touch
//! (`crossfades`).
//!
//! Every render here runs on its own thread and reports progress and a way
//! to stop; the UI only polls it (sweep M11: one folder per concern).

pub(crate) mod bounce;
pub(crate) mod consolidate;
pub(crate) mod crossfades;
pub(crate) mod normalize;
