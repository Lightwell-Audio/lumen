//! The single instance from tests (M9 R8): requests as later starts send
//! them, and what waits for a recording or an export.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use crate::App;
use crate::association::{Hive, State};
use crate::instance::{Instance, Reply, Request};

/// A request sent as a later start would, and its answer once given.
pub struct HandOver(Receiver<Reply>);

impl HandOver {
    /// "accepted", "rejected: <reason>" or "not answering", once answered.
    pub fn reply(&self) -> Option<String> {
        self.0.try_recv().ok().map(|reply| match reply {
            Reply::Accepted => "accepted".to_owned(),
            Reply::Rejected(reason) => format!("rejected: {reason}"),
            Reply::NotAnswering => "not answering".to_owned(),
        })
    }
}

impl App {
    /// Sends what a later start sends: open `project`, or only come to the
    /// front (`None`). The window answers on its next frame.
    pub fn hand_over(&mut self, project: Option<PathBuf>) -> HandOver {
        let request = project.map_or(Request::Show, Request::Open);
        HandOver(
            self.instance
                .get_or_insert_with(Instance::fake)
                .inject(request),
        )
    }

    /// What the waiting notice says, while something waits for a recording
    /// or an export.
    pub fn holding_words(&self) -> Option<String> {
        let held = self.holding.as_ref()?;
        let busy = self.busy_for_open()?;
        Some(crate::app::handover::words(
            &held.next,
            busy,
            self.is_recording(),
        ))
    }

    /// Whether something waits to open.
    pub fn is_holding(&self) -> bool {
        self.holding.is_some()
    }

    /// Who opens `.lumen` files: "off", "this" or "other: <path>".
    pub fn association(&self) -> String {
        match self.association.status().state {
            State::Off => "off".to_owned(),
            State::This => "this".to_owned(),
            State::Other(path) => format!("other: {}", path.display()),
        }
    }

    /// The association's hive starts afresh: registered to `other` (another
    /// copy), the person's "Open with" choice naming `chosen`, and writes
    /// failing with `fail`.
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
    }

    /// What a normal start does with the association: registers this copy
    /// when none is registered and the setting wants it.
    pub fn associate_as_a_normal_start(&mut self) {
        self.associate_at_start();
    }

    /// Times Default apps was asked for.
    pub fn default_apps_asked(&self) -> usize {
        self.association.default_apps_asked
    }

    /// Whether the settings want the association.
    pub fn wants_association(&self) -> bool {
        self.settings.projects.associate_files
    }

    /// Whether the "Save changes to …?" prompt is showing.
    pub fn save_prompt_open(&self) -> bool {
        self.prompt.is_some()
    }
}

/// The small window of a later start the running Lumen did not take, for
/// shots: refused with `why`, or not answering (`None`), in `appearance`.
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

/// The window [`not_answering_window`] makes.
pub struct NotAnswering(Box<dyn eframe::App>);

impl eframe::App for NotAnswering {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        self.0.ui(ui, frame);
    }
}
