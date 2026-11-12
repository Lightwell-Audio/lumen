//! A card's preset menu (M6 slice 5f, the owner's A): a field in the card
//! with the name of the sound, arrows to the previous and next, and a menu
//! that lists the plugin's factory programs and the preset files found for it,
//! with a search, Save, Save as and Show presets folder. A dot marks a sound
//! changed since it was chosen.
//!
//! Choosing is `App::load_program` or `App::load_preset` (slice 5c: one undo
//! step); the module is the list, the name remembered for the card, and saving
//! a file, with the disk work in `preset_files` off the interface's thread.
//! `state` keeps each card's sound and the menu's, `choose` lists and chooses a
//! sound, `save` writes one, and `menu` is the menu's interface.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, FontId, Ui};
use lumen_hosting::{Preset, info_xml, parse_class_id};
use lumen_project::model::{Id, PluginRef};

use crate::App;
use crate::document::find_device;
use crate::hosting::State;
use crate::preset_files::{Done, Entry, Files, NAME_LIMIT, Places};
use crate::{theme, widgets};

/// The menu's width and the height its list may reach.
const WIDTH: f32 = 272.0;
const LIST_HEIGHT: f32 = 264.0;
/// How long a save waits for the plugin to be read.
const SAVE_WAIT: Duration = Duration::from_secs(8);

mod choose;
mod menu;
mod save;
mod state;

pub(crate) use state::{FieldLook, Presets};

#[cfg(test)]
mod tests;
