//! The clip, by concern (audit M11): the commands on the selection
//! (`clip_commands`: join, crop, unloop, nudge and the selection variants;
//! `clip_warp`: the tempo of an audio clip's file), the edits the channels it
//! plays on own (`clip_card`: mute, lock, rename, colour and the pinned
//! properties card; `clip_controls`: the rows the card and the audio editor's
//! strip share; `clip_envelopes`: the clip's own gain and pan lane;
//! `clip_file`: the file an audio clip plays), the renders a clip or a track
//! can ask for (`clip_consolidate`, `clip_bounce`, `clip_normalize`), and the
//! clip editors' shared header (`clip_header`).

pub(crate) mod clip_bounce;
pub(crate) mod clip_card;
pub(crate) mod clip_commands;
pub(crate) mod clip_consolidate;
pub(crate) mod clip_controls;
mod clip_envelopes;
pub(crate) mod clip_file;
pub(crate) mod clip_header;
pub(crate) mod clip_normalize;
pub(crate) mod clip_warp;
