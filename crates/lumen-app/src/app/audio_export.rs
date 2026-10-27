//! The Export window and its task. Choices are session state, shared by the
//! command palette and app-menu command. Export never edits the document.

mod window;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use lumen_project::model::ClipContent;
use lumen_project::routing::all_tracks;

use crate::App;
use crate::app::projects::FileDialog;
use crate::export::{self, Buses, Options, Poll, Report, Task};
use crate::render_audio::RenderAudio;

const READ_WAIT: Duration = Duration::from_secs(4);

#[derive(Default)]
pub(crate) struct Export {
    pub open: bool,
    pub options: Options,
    pub reports: Vec<Report>,
    pub error: Option<String>,
    pub progress: Option<f32>,
    asked: Option<Options>,
    waiting: Option<Waiting>,
    task: Option<Task>,
    initialized: bool,
    pub focus: bool,
}

struct Waiting {
    options: Options,
    path: PathBuf,
    mark: u64,
    since: Instant,
    project: lumen_project::model::Project,
    span: (lumen_core::time::Ticks, lumen_core::time::Ticks),
}

impl Export {
    pub fn busy(&self) -> bool {
        self.task.is_some() || self.waiting.is_some() || self.asked.is_some()
    }
}

impl App {
    pub(crate) fn open_audio_export(&mut self) {
        if !self.export.initialized {
            self.export.options.rate = self.document.project().sample_rate;
            self.export.initialized = true;
        }
        self.export.open = true;
        self.export.focus = true;
    }

    pub(crate) fn export_span(
        &self,
        options: Options,
    ) -> Option<(lumen_core::time::Ticks, lumen_core::time::Ticks)> {
        let loop_region = self.transport.loop_region;
        let selection = self.arrangement.range.as_ref().map(|r| (r.start, r.end));
        export::span(
            self.document.project(),
            options.scope,
            (loop_region.start, loop_region.end),
            selection,
        )
    }

    pub(crate) fn export_unavailable(&self, options: Options) -> Option<&'static str> {
        if self.export.busy() {
            return Some("An export is already under way");
        }
        if self.dialog.is_some() {
            return Some("A file dialog is open");
        }
        if self.transport.state().recording {
            return Some("Stop recording before exporting");
        }
        if self.export_span(options).is_none() {
            return Some("Choose a non-empty project, loop or time selection");
        }
        if export::targets(self.document.project(), options.buses).is_empty() {
            return Some("There are no tracks of that kind to export");
        }
        if options.realtime
            && !matches!(
                self.transport.audio.status,
                crate::audio::Status::Running { .. }
            )
        {
            return Some("Turn audio on before a real-time export");
        }
        None
    }

    /// A file dialog runs on its own thread; tests supply its answer through
    /// the same route as other save commands.
    pub(crate) fn choose_export_destination(&mut self, ctx: &egui::Context) {
        let options = self.export.options;
        if let Some(why) = self.export_unavailable(options) {
            self.export.error = Some(why.into());
            return;
        }
        let start = self.dialog_start();
        let name = format!("{}.{}", self.document.name(), options.format.extension());
        let owner = self.owner;
        self.export.asked = Some(options);
        self.export.error = None;
        self.export.reports.clear();
        self.file_dialog(
            ctx,
            move || {
                let mut dialog = rfd::FileDialog::new().set_title("Export audio");
                if let Some(dir) = start {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(owner) = &owner {
                    dialog = dialog.set_parent(owner);
                }
                let path = if options.buses == Buses::Master {
                    dialog
                        .add_filter("Audio", &[options.format.extension()])
                        .set_file_name(name)
                        .save_file()
                } else {
                    dialog.pick_folder()
                };
                FileDialog::ExportAudio(path)
            },
            |paths| FileDialog::ExportAudio(paths.into_iter().next()),
        );
    }

    pub(crate) fn export_destination_chosen(&mut self, path: Option<PathBuf>, ctx: &egui::Context) {
        let Some(options) = self.export.asked.take() else {
            return;
        };
        let Some(path) = path else {
            return;
        };
        let Some(span) = self.export_span(options) else {
            self.export.error = Some("The export range is empty".into());
            return;
        };
        self.export.waiting = Some(Waiting {
            options,
            path,
            mark: self.document.change_mark(),
            since: Instant::now(),
            project: self.document.project().clone(),
            span,
        });
        self.export.progress = Some(0.0);
        self.hosting.request_captures();
        ctx.request_repaint();
    }

    fn launch_audio_export(&mut self, waiting: Waiting, ctx: &egui::Context) -> Result<(), String> {
        if self.document.change_mark() != waiting.mark
            || self.document.project() != &waiting.project
        {
            return Err(
                "The project changed while its plugin states were being read. Start Export again"
                    .into(),
            );
        }
        if self.hosting.captures_pending() || self.hosting.unread() != 0 {
            return Err(
                "A plugin's current state could not be read. Reload it, then export again".into(),
            );
        }
        let options = waiting.options;
        if self.transport.state().recording {
            return Err("Stop recording before exporting".into());
        }
        if options.realtime
            && !matches!(
                self.transport.audio.status,
                crate::audio::Status::Running { .. }
            )
        {
            return Err("Turn audio on before a real-time export".into());
        }
        let (start, end) = waiting.span;
        if self.document.project().tracks.iter().any(|t| {
            t.midi_to.is_some()
                && !t.mute
                && t.clips
                    .iter()
                    .any(|c| !c.muted && c.start < end && c.start + c.length > start)
        }) {
            return Err(
                "Record external MIDI instruments to audio before exporting their sound".into(),
            );
        }
        let mut project = waiting.project;
        self.hosting.stamp(&mut project);
        // A real-time source runs at the device's rate. Only this snapshot's
        // rate changes; the open project and output-file rate stay as chosen.
        if options.realtime {
            project.sample_rate = self.transport.state().sample_rate;
        }
        let audio =
            RenderAudio::of_tracks(&self.lanes, &project, project.tracks.iter().map(|t| &**t));
        for clip in project.tracks.iter().flat_map(|t| &t.clips) {
            if !clip.muted
                && clip.start < end
                && clip.start + clip.length > start
                && matches!(clip.content, ClipContent::Audio { .. })
                && !audio.has(clip.id)
            {
                return Err("Some audio is missing or still loading. Relink it or wait for it to load, then export".into());
            }
        }
        let mut plugins = Vec::new();
        for track in all_tracks(&project) {
            plugins.extend(self.hosting.offline_starts(
                &project,
                track.id,
                true,
                self.plugins.catalog(),
                audio.rate,
            )?);
        }
        let timeline = lumen_engine::Timeline::new(
            lumen_core::time::TempoMap::new(project.tempo.clone()).map_err(|e| e.to_string())?,
            project.meter_map(),
        );
        let from = timeline.sample_at(start, audio.rate);
        let frames = timeline.sample_at(end, audio.rate) - from;
        if frames <= 0 {
            return Err("The export range is shorter than one sample".into());
        }
        let targets = export::targets(&project, options.buses);
        let live = options
            .realtime
            .then(|| self.transport.audio.export.clone());
        if options.realtime {
            self.transport.stop();
        }
        let job = export::Job {
            project,
            clips: audio.clips(),
            again: audio.again(),
            plugins,
            rate: audio.rate,
            from,
            frames: frames as u64,
            options,
            destination: waiting.path,
            targets,
            live,
            protected: self
                .document
                .media_files
                .values()
                .cloned()
                .chain(self.document.path.iter().cloned())
                .collect(),
        };
        self.export.task = Some(Task::start(job, ctx)?);
        Ok(())
    }

    pub(crate) fn cancel_audio_export(&mut self) {
        if let Some(task) = &self.export.task {
            task.stop();
        }
        self.export.waiting = None;
        self.export.asked = None;
        if self.export.task.is_none() {
            self.export.progress = None;
        }
    }

    pub(crate) fn poll_audio_export(&mut self, ctx: &egui::Context) {
        if let Some(waiting) = &self.export.waiting {
            if self.hosting.captures_pending() && waiting.since.elapsed() < READ_WAIT {
                ctx.request_repaint_after(Duration::from_millis(30));
                return;
            }
            if let Some(waiting) = self.export.waiting.take()
                && let Err(why) = self.launch_audio_export(waiting, ctx)
            {
                self.export.error = Some(why.clone());
                self.export.progress = None;
                self.say(format!("Could not export: {why}"));
            }
        }
        let Some(task) = &self.export.task else {
            return;
        };
        if !self.typing && !self.export.open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            task.stop();
        }
        match task.poll() {
            Poll::Running(progress) => {
                self.export.progress = Some(progress);
                self.toast = Some((
                    format!("Exporting {}% · Esc to stop", (100.0 * progress) as u32),
                    Instant::now(),
                ));
                ctx.request_repaint_after(Duration::from_millis(50));
            }
            Poll::Done(reports) => {
                self.export.task = None;
                self.export.progress = None;
                let n = reports.len();
                self.export.reports = reports;
                self.export.open = true;
                self.say(format!(
                    "Exported {n} audio {}",
                    if n == 1 { "file" } else { "files" }
                ));
            }
            Poll::Stopped => {
                self.export.task = None;
                self.export.progress = None;
                self.say("Export stopped; destination files are unchanged".into());
            }
            Poll::Failed(why) => {
                self.export.task = None;
                self.export.progress = None;
                self.export.error = Some(why.clone());
                self.export.open = true;
                self.say(format!("Could not export: {why}"));
            }
        }
    }
}
