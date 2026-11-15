use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use crate::App;
use crate::association::{Hive, State};
use crate::instance::{Instance, Reply, Request};

pub struct HandOver(Receiver<Reply>);

impl HandOver {
    pub fn reply(&self) -> Option<String> {
        self.0.try_recv().ok().map(|reply| match reply {
            Reply::Accepted => "accepted".to_owned(),
            Reply::Rejected(reason) => format!("rejected: {reason}"),
            Reply::NotAnswering => "not answering".to_owned(),
        })
    }
}

impl App {
    pub fn hand_over(&mut self, project: Option<PathBuf>) -> HandOver {
        let request = project.map_or(Request::Show, Request::Open);
        HandOver(
            self.instance
                .get_or_insert_with(Instance::fake)
                .inject(request),
        )
    }

    pub fn holding_words(&self) -> Option<String> {
        let held = self.holding.as_ref()?;
        let busy = self.busy_for_open()?;
        Some(crate::app::handover::words(
            &held.next,
            busy,
            self.is_recording(),
        ))
    }

    pub fn is_holding(&self) -> bool {
        self.holding.is_some()
    }

    pub fn association(&self) -> String {
        let status = self
            .association_status()
            .cloned()
            .unwrap_or_else(|| self.association.status());
        match status.state {
            State::Off => "off".to_owned(),
            State::This => "this".to_owned(),
            State::Other(path) => format!("other: {}", path.display()),
        }
    }

    pub fn fake_association(
        &mut self,
        other: Option<&std::path::Path>,
        chosen: Option<&str>,
        fail: Option<&str>,
    ) {
        self.association.seed(|hive| {
            if let Some(other) = other {
                crate::association::register(hive, other).expect("a map takes writes");
            }
            if let Some(chosen) = chosen {
                hive.set(crate::association::USER_CHOICE, "ProgId", chosen)
                    .expect("a map takes writes");
            }
            hive.fail = fail.map(str::to_owned);
        });
        let status = self.association.status();
        self.association_status.set(status);
    }

    pub fn change_association_hive(&mut self, other: Option<&std::path::Path>) {
        self.association.seed(|hive| {
            if let Some(other) = other {
                crate::association::register(hive, other).expect("a map takes writes");
            }
        });
    }

    pub fn associate_as_a_normal_start(&mut self) {
        self.associate_at_start();
    }

    pub fn default_apps_asked(&self) -> usize {
        self.association.default_apps_asked
    }

    pub fn wants_association(&self) -> bool {
        self.settings.projects.associate_files
    }

    pub fn save_prompt_open(&self) -> bool {
        self.prompt.is_some()
    }
}

pub fn not_answering_window(
    ctx: &eframe::egui::Context,
    why: Option<&str>,
    appearance: &str,
) -> NotAnswering {
    let look = crate::settings::Appearance {
        theme: appearance.to_owned(),
        ..Default::default()
    };
    NotAnswering(crate::instance::start::preview(
        ctx,
        why.map(str::to_owned),
        &look,
    ))
}

pub struct NotAnswering(Box<dyn eframe::App>);

impl eframe::App for NotAnswering {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        self.0.ui(ui, frame);
    }
}
