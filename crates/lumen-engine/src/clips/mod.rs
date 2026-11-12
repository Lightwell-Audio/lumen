//! Audio clips on the timeline (M2 slice 6). The audio lives in memory
//! (owner decision 2026-10-06); disk streaming replaces that source in M9
//! behind the same [`Source`] interface.
//!
//! A clip starts at a musical position and plays its audio at the audio's
//! own speed, or, when the project asks for it, as a warped stream rendered
//! at the project's tempo (M9.5). Clips fade in and out over 2 ms at their
//! edges; the engine crossfades every transport jump.
//!
//! The clip list is replaced whole: the UI builds a new one and sends it
//! through a lock-free queue, and the old one goes back to be freed on the
//! UI side. The audio thread never allocates or frees.
//!
//! Each clip plays into its track's bus; the mixer does the rest (M4,
//! ADR-0035).
//!
//! Levels glide. Each clip's gain is carried from one list to the next by
//! clip id and moves toward its new value at a fixed rate, so clip gain
//! drags and clip mutes never step or click. A clip that appears partway
//! through (its audio finished loading during playback, or its track
//! joined the routing late) fades in.
//!
//! Clips on a track may overlap; each plays on and the bus sums them. A
//! crossfade is two overlapping clips whose fades are marked `blend`: the
//! outgoing one fades out and the incoming one in along an equal-power law.
//!
//! A clip can fade in and out along any [`Curve`] and repeat its content
//! (loop-extend, M3 slice 6). Each repeat gets the 2 ms edge fades, so the
//! seams never click. Clips with neither take the plain path. The audio
//! thread reads fades from tables built on the UI side, one per distinct
//! curve, shared between clips and lists.
//!
//! A clip that moves, is trimmed or slipped, or goes away while playing
//! crossfades over 5 ms: its old placement fades out while the new one
//! fades in, so a clip dragged under the playhead never clicks (owner,
//! after M3 slice 6c: dragging a playing clip crackled). Clips whose
//! placement did not change play straight through. New lists wait while a
//! crossfade runs, so at most two lists play at once.
//!
//! [`Source`]: crate::source::Source
//! [`Curve`]: lumen_core::curve::Curve

mod clip;
mod crossfades;
mod fades;
mod handle;
mod mix;
mod pages;
mod player;
mod seams;
mod set;

pub use clip::{Clip, ClipAudio};
pub use fades::{ClipShape, FadeFrames};
pub use handle::{ClipsHandle, clip_player, clip_player_for_render};
pub use pages::for_each_page;
pub use player::ClipPlayer;
