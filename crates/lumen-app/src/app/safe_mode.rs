//! Safe mode (M6 slice 5e, the owner's I2 and F2): Lumen opens a project
//! without loading a single plugin, so a plugin that stops Lumen starting can
//! be got past. No plugin is scanned, loaded or hosted; each plugin card keeps
//! its settings (the project saves them as they were) and can be loaded one at
//! a time; "Leave safe mode" loads the rest.
//!
//! Three ways in: the `--safe-mode` flag, Shift held while Lumen starts
//! (Windows), and an offer after a start that never finished loading plugins:
//! a marker file is written beside the settings when plugins begin loading and
//! removed when they have loaded or Lumen closes properly, so one left behind
//! says Lumen ended while plugins were loading. Until the offer is answered,
//! nothing is loaded.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;

use crate::App;
use crate::icons::Icon;
use crate::widgets::raised;

/// The marker's name, beside the settings file.
pub(crate) const MARKER: &str = "plugin-loading.marker";

pub(crate) struct SafeMode {
    /// No plugin is loaded (but those asked for one at a time).
    pub(crate) on: bool,
    /// The start did not finish loading plugins last time: the strip offers
    /// safe mode, and nothing loads until it is answered.
    pub(crate) offer: bool,
    marker: Option<PathBuf>,
    /// Whether this run has written the marker (and so removes it).
    written: bool,
    writer: Option<Sender<Mark>>,
}

enum Mark {
    Set(PathBuf),
    Clear(PathBuf),
}

impl SafeMode {
    /// How this run starts. `asked` is the flag; the marker is looked for
    /// where `marker` says when plugins can be hosted at all.
    pub(crate) fn start(asked: bool, marker: Option<PathBuf>, plugins: bool) -> Self {
        let marker = marker.filter(|_| plugins);
        let held = asked || shift_held();
        let unfinished = marker.as_deref().is_some_and(Path::exists);
        let offer = unfinished && !held;
        Self {
            on: held || offer,
            offer,
            marker,
            written: false,
            writer: None,
        }
    }

    /// Writes the marker when plugins begin loading and removes it when they
    /// have, in order, off the interface's thread. Called every frame.
    pub(crate) fn watch(&mut self, loading: bool) {
        if self.on {
            return;
        }
        let Some(path) = self.marker.clone() else {
            return;
        };
        if loading && !self.written {
            self.written = true;
            self.send(Mark::Set(path));
        } else if !loading && self.written {
            self.written = false;
            self.send(Mark::Clear(path));
        }
    }

    /// Lumen is closing properly: a start that is cut short leaves the marker,
    /// this does not.
    pub(crate) fn closing(&mut self) {
        if self.written
            && let Some(path) = &self.marker
        {
            let _ = std::fs::remove_file(path);
            self.written = false;
        }
    }

    fn send(&mut self, mark: Mark) {
        let writer = self.writer.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel::<Mark>();
            std::thread::spawn(move || {
                for mark in rx {
                    match mark {
                        Mark::Set(path) => {
                            let _ = std::fs::write(path, b"plugins were loading\n");
                        }
                        Mark::Clear(path) => {
                            let _ = std::fs::remove_file(path);
                        }
                    }
                }
            });
            tx
        });
        let _ = writer.send(mark);
    }
}

/// Whether Shift is down now. Only asked at start: a way into safe mode for
/// someone who cannot get past a plugin to a menu.
#[cfg(windows)]
fn shift_held() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
    // SAFETY: a plain query of the key state, no pointers.
    unsafe { GetAsyncKeyState(i32::from(VK_SHIFT)) < 0 }
}

#[cfg(not(windows))]
fn shift_held() -> bool {
    false
}

impl App {
    /// The strip under the transport bar: safe mode's shield, what it means and
    /// the way out; or, before the offer is answered, the offer.
    pub(crate) fn safe_banner(&mut self, ui: &mut egui::Ui) {
        if !self.safe.on {
            return;
        }
        let offer = self.safe.offer;
        let (mut leave, mut stay) = (false, false);
        egui::Panel::top("safe mode banner")
            .frame(super::view::notice_panel_frame())
            .show(ui, |ui| {
                super::view::notice_card_with(ui, Some(Icon::Shield), |ui| {
                    if offer {
                        ui.label(format!(
                            "Last time {PRODUCT_NAME} didn't finish loading plugins. Start in safe mode to open this project without them."
                        ));
                        stay = raised::primary_button(ui, "Start in safe mode").clicked();
                        leave = raised::button(ui, "Load plugins").clicked();
                    } else {
                        ui.label("Safe mode: plugins are not loaded");
                        leave = raised::button(ui, "Leave safe mode").clicked();
                    }
                });
            });
        if stay {
            self.safe.offer = false;
        }
        if leave {
            self.leave_safe_mode(ui.ctx());
        }
    }

    /// Ends safe mode: the plugins load, and Lumen looks for new ones as the
    /// settings say.
    pub(crate) fn leave_safe_mode(&mut self, ctx: &egui::Context) {
        if !self.safe.on {
            return;
        }
        self.safe.on = false;
        self.safe.offer = false;
        self.hosting.set_safe(false);
        self.plugins.leave_safe(&self.settings.plugins, ctx);
        ctx.request_repaint();
    }

    /// Loads one plugin although safe mode is on (the card's "Load this
    /// plugin").
    pub(crate) fn load_one_plugin(&mut self, device: lumen_project::model::Id) {
        self.hosting.allow(device);
    }

    /// Once a frame, after the hosts have been looked after.
    pub(crate) fn watch_safe_mode(&mut self) {
        let loading = self.hosting.loading_any();
        self.safe.watch(loading);
    }
}
