//! Automation lanes, by concern (audit M11): what a press and a drag on a
//! lane do (`auto_editing`: the pointer, the tools and where Ctrl reaches the
//! clip under the lane; `auto_gestures`: moving, boxing, freehand, shapes and
//! bending a segment), the keys and commands on the chosen track's lane
//! (`auto_keys`), the chooser for a plugin's parameters (`auto_params`, M7
//! slice A3) and the Write, Touch and Latch passes into the lanes
//! (`auto_recording`, M7 slice A4b).

pub(crate) mod auto_editing;
mod auto_gestures;
pub(crate) mod auto_keys;
mod auto_params;
pub(crate) mod auto_recording;
