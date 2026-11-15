use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, FontId, Ui};
use lumen_hosting::{Preset, info_xml, parse_class_id};
use lumen_project::model::{Id, PluginRef};

use crate::App;
use crate::document::find_device;
use crate::hosting::State;
pub(crate) use crate::preset_files::Sound;
use crate::preset_files::{Done, Entry, Files, NAME_LIMIT, Places};
use crate::{theme, widgets};

const WIDTH: f32 = 272.0;
const LIST_HEIGHT: f32 = 264.0;
const SAVE_WAIT: Duration = Duration::from_secs(8);

mod choose;
mod menu;
mod save;
mod state;

pub(crate) use state::{FieldLook, Presets};

#[cfg(test)]
mod tests;
