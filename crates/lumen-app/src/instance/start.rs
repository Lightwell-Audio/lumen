use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use lumen_core::product::{PRODUCT_NAME, display_name};

use super::{Attempt, Claim, Instance, Listener, PATIENCE, Place, Request, claim};
use crate::widgets::raised;
use crate::{Launch, brand, fonts, motion, settings, theme};

pub(crate) fn start(args: &[String]) -> Option<Instance> {
    let settings = Launch::settings_path(args)?;
    let Some(place) = Place::for_settings(&settings) else {
        crate::trace::log(|| "instance: the settings folder cannot be made".to_owned());
        return None;
    };
    let request = match Launch::project_arg(args) {
        Some(path) => Request::Open(std::path::absolute(&path).unwrap_or(path)),
        None => Request::Show,
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let listener = if has("--instance-deaf") {
        Listener::Deaf
    } else if has("--instance-busy") {
        Listener::Busy
    } else {
        Listener::Normal
    };
    let attempt = Attempt {
        place: &place,
        request: &request,
        patience: PATIENCE,
        listener,
    };
    let why = match claim(&attempt) {
        Claim::Owner(instance) => return Some(instance),
        Claim::Handed => std::process::exit(0),
        Claim::Refused(reason) => Some(reason),
        Claim::NotAnswering => None,
    };
    std::process::exit(stuck(Stuck {
        args: args.to_vec(),
        place,
        request,
        why,
        retry: None,
        settings,
    }))
}

struct Stuck {
    args: Vec<String>,
    place: Place,
    request: Request,
    why: Option<String>,
    retry: Option<Receiver<Claim>>,
    settings: PathBuf,
}

impl Stuck {
    fn heading(&self) -> String {
        match &self.why {
            Some(_) => format!("{PRODUCT_NAME} did not open the file"),
            None => format!("{PRODUCT_NAME} is already running but is not answering"),
        }
    }

    fn words(&self) -> String {
        match &self.why {
            Some(reason) => reason.clone(),
            None => "Your work in it is safe. If it is busy, try again in a moment.".to_owned(),
        }
    }

    fn try_again(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let place = self.place.clone();
        let request = self.request.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let got = claim(&Attempt {
                place: &place,
                request: &request,
                patience: PATIENCE,
                listener: Listener::Normal,
            });
            let _ = tx.send(got);
            ctx.request_repaint();
        });
        self.retry = Some(rx);
    }

    fn settle_retry(&mut self) {
        let Some(got) = self.retry.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.retry = None;
        match got {
            Claim::Handed => std::process::exit(0),
            Claim::Owner(instance) => {
                drop(instance);
                let restarted = std::env::current_exe().and_then(|exe| {
                    std::process::Command::new(exe)
                        .args(&self.args[1..])
                        .spawn()
                });
                match restarted {
                    Ok(_) => std::process::exit(0),
                    Err(e) => {
                        self.why = Some(format!("{PRODUCT_NAME} could not start again: {e}."));
                    }
                }
            }
            Claim::Refused(reason) => self.why = Some(reason),
            Claim::NotAnswering => self.why = None,
        }
    }
}

impl eframe::App for Stuck {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.settle_retry();
        let frame = egui::Frame::new()
            .fill(theme::p().app)
            .inner_margin(egui::Margin::same(theme::space::XL as i8));
        egui::Panel::bottom("stuck actions")
            .frame(frame)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let trying = self.retry.is_some();
                    let again =
                        ui.add_enabled_ui(!trying, |ui| raised::primary_button(ui, "Try again"));
                    if again.inner.clicked() {
                        self.try_again(&ctx);
                    }
                    if raised::button(ui, "Close").clicked() {
                        std::process::exit(1);
                    }
                });
            });
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(
                    egui::RichText::new(self.heading())
                        .size(theme::size::HEADING)
                        .color(theme::p().text),
                );
                ui.add_space(theme::space::S);
                ui.add(egui::Label::new(self.words()).wrap());
                if self.retry.is_some() {
                    ui.add_space(theme::space::S);
                    ui.label(
                        egui::RichText::new("Trying again. This takes up to 10 seconds.")
                            .color(theme::p().text_secondary),
                    );
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            std::process::exit(1);
        }
    }
}

fn dress(ctx: &egui::Context, look: &settings::Appearance) {
    fonts::install(ctx);
    motion::set_reduced(look.reduce_motion);
    theme::apply(
        ctx,
        theme::Appearance::from_key(&look.theme),
        motion::time(motion::BASE),
    );
    ctx.set_zoom_factor(look.scale());
}

pub(crate) fn preview(
    ctx: &egui::Context,
    why: Option<String>,
    look: &settings::Appearance,
) -> Box<dyn eframe::App> {
    dress(ctx, look);
    let folder = std::env::temp_dir().join("lumen-preview-instance");
    Box::new(Stuck {
        args: Vec::new(),
        place: Place {
            key: "preview".into(),
            lock: folder.join("preview.instance"),
            folder,
        },
        request: Request::Show,
        why,
        retry: None,
        settings: PathBuf::new(),
    })
}

fn stuck(stuck: Stuck) -> i32 {
    eprintln!("{}. {}", stuck.heading(), stuck.words());
    let look = settings::peek(&stuck.settings).appearance;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(display_name())
            .with_inner_size([460.0 * look.scale(), 220.0 * look.scale()])
            .with_min_inner_size([320.0 * look.scale(), 160.0 * look.scale()])
            .with_icon(brand::window_icon()),
        ..Default::default()
    };
    let shown = eframe::run_native(
        PRODUCT_NAME,
        options,
        Box::new(move |cc| {
            dress(&cc.egui_ctx, &look);
            Ok(Box::new(stuck))
        }),
    );
    if let Err(e) = shown {
        eprintln!("The window could not be shown: {e}");
    }
    1
}
