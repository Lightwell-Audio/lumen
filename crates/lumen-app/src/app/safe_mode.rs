use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use eframe::egui;
use lumen_core::product::PRODUCT_NAME;

use crate::App;
use crate::icons::Icon;
use crate::widgets::raised;

pub(crate) const MARKER: &str = "plugin-loading.marker";

const CLOSING_WAIT: std::time::Duration = std::time::Duration::from_millis(500);
const CLOSING_STEP: std::time::Duration = std::time::Duration::from_millis(2);

pub(crate) struct SafeMode {
    pub(crate) on: bool,
    pub(crate) offer: bool,
    marker: Option<PathBuf>,
    written: bool,
    writer: Option<Sender<Mark>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

enum Mark {
    Set(PathBuf),
    Clear(PathBuf),
}

impl SafeMode {
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
            thread: None,
        }
    }

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

    pub(crate) fn closing(&mut self) {
        if self.written
            && let Some(path) = self.marker.clone()
        {
            self.send(Mark::Clear(path));
            self.written = false;
        }
        self.writer = None;
        if let Some(thread) = self.thread.take() {
            let end = std::time::Instant::now() + CLOSING_WAIT;
            while !thread.is_finished() && std::time::Instant::now() < end {
                std::thread::sleep(CLOSING_STEP);
            }
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }

    fn send(&mut self, mark: Mark) {
        let thread = &mut self.thread;
        let writer = self.writer.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel::<Mark>();
            *thread = Some(std::thread::spawn(move || {
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
            }));
            tx
        });
        let _ = writer.send(mark);
    }
}

#[cfg(windows)]
fn shift_held() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
    unsafe { GetAsyncKeyState(i32::from(VK_SHIFT)) < 0 }
}

#[cfg(not(windows))]
fn shift_held() -> bool {
    false
}

impl App {
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

    pub(crate) fn load_one_plugin(&mut self, device: lumen_project::model::Id) {
        self.hosting.allow(device);
    }

    pub(crate) fn watch_safe_mode(&mut self) {
        let loading = self.hosting.loading_any();
        self.safe.watch(loading);
    }
}
