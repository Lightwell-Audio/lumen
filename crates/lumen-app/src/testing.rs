//! What the end-to-end tests (`tests/e2e/`, ADR-0029) may see and do,
//! beyond what a user sees on screen: a clean start, and read-only looks at
//! state the screen shows only as pixels (the playhead, the zoom). Tests
//! drive the app through its controls; this is the small, deliberate API
//! for the rest, so the app's internals stay private.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::{Id, Project};

use crate::audio::{self, Status};
use crate::launch::Launch;
use crate::timeview::TimeView;
use crate::{App, files, settings};

impl Launch {
    /// A clean start for a test: default settings that are never saved,
    /// autosaves in `dir/autosave`, and the silent test device.
    pub fn for_test(dir: &Path) -> Self {
        Self {
            started: Instant::now(),
            settings: settings::Settings::default(),
            settings_warning: None,
            settings_path: None,
            audio: audio::Choice::test_device(),
            midi: crate::midi::Source::Test(lumen_midi::fake::FakeControl::new()),
            autosave_dir: Some(dir.join("autosave")),
            autosave_after: None,
            crash_dir: Some(dir.join("crashes")),
            crash_test: None,
            open: None,
            open_audio: None,
            stress: false,
            audio_settings: false,
            recover_latest: false,
            scale: None,
            appearance: None,
            smoke_test: false,
            screenshot: None,
            bench: None,
            bench_size: None,
            autoplay: None,
            test_latency: None,
            save_as: None,
            scripted_dialogs: true,
        }
    }

    /// Opens `project` at start, as `lumen song.lumen` does.
    pub fn opening(mut self, project: PathBuf) -> Self {
        self.open = Some(project);
        self
    }

    /// Starts with MIDI ports from `fake`, which the test plugs and
    /// unplugs.
    pub fn with_midi(mut self, fake: lumen_midi::fake::FakeControl) -> Self {
        self.midi = crate::midi::Source::Test(fake);
        self
    }

    /// Autosaves `after` the first unsaved change, instead of a minute.
    pub fn autosaving_after(mut self, after: Duration) -> Self {
        self.autosave_after = Some(after);
        self
    }

    /// Starts in `appearance` ("light", "dark"), as `--appearance` does.
    pub fn appearing(mut self, appearance: &str) -> Self {
        self.appearance = Some(appearance.to_owned());
        self
    }

    /// Starts with command `id`'s shortcuts saved as `chords` ("Alt+W"),
    /// as a settings file with a changed shortcut would.
    pub fn with_saved_shortcut(mut self, id: &str, chords: &[&str]) -> Self {
        let chords = chords.iter().map(|c| (*c).to_owned()).collect();
        self.settings
            .commands
            .shortcuts
            .insert(id.to_owned(), chords);
        self
    }
}

/// Installs Lumen's GPU renderer (grid, clips, waveforms) when the harness
/// renders with wgpu, as `run` does for the real window.
pub fn install_renderer(cc: &eframe::CreationContext<'_>) -> Result<(), String> {
    match cc.wgpu_render_state.as_ref() {
        Some(state) => crate::canvas::install(state, None).map_err(|e| format!("renderer: {e}")),
        None => Ok(()),
    }
}

/// Where a project called `name` is saved in `dir` (its own folder).
pub fn project_file(dir: &Path, name: &str) -> PathBuf {
    files::project_file_for(&dir.join(name))
}

/// What a meter shows (`App::meter`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterShown {
    pub peak_db: f32,
    pub rms_db: f32,
    pub hold_db: f32,
    pub clipped: bool,
}

/// The extension of project files (and autosaves).
pub const PROJECT_EXTENSION: &str = files::EXTENSION;

impl App {
    /// The open project.
    pub fn project(&self) -> &Project {
        self.document.project()
    }

    /// Unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.document.is_dirty()
    }

    /// The audio device is running.
    pub fn audio_running(&self) -> bool {
        matches!(self.transport.audio.status, Status::Running { .. })
    }

    pub fn is_playing(&self) -> bool {
        self.transport.state().playing
    }

    pub fn playhead(&self) -> Ticks {
        self.transport.state().ticks
    }

    /// Where the playhead is drawn: smoothed while playing (`playhead.rs`).
    pub fn drawn_playhead(&self) -> Ticks {
        self.transport.playhead()
    }

    /// The test device drops `n` buffers, as a real one does under load.
    pub fn cause_dropouts(&mut self, n: u64) {
        if let Some(meters) = self.transport.audio.meters() {
            meters.count_dropouts(n);
        }
    }

    /// Puts the engine's synthetic latent device on track `id`, adding
    /// `frames` of latency (0 takes it off), as a device with latency will
    /// be from M6: what delay compensation is tested with (M4 slice 6).
    pub fn set_test_latency(&mut self, id: Id, frames: u32) {
        let latent = &mut self.lanes.devices.latent;
        if frames == 0 {
            latent.remove(&id);
        } else {
            latent.insert(id, frames);
        }
    }

    /// The delay compensation the engine was last sent, in frames.
    pub fn compensation(&self) -> u32 {
        self.transport.audio.mixer.compensation().frames
    }

    /// What the test device has played since the last call, as (left,
    /// right) frames: the audio a user would hear.
    pub fn heard(&mut self) -> Vec<(f32, f32)> {
        self.heard_frames()
            .into_iter()
            .map(|[l, r, ..]| (l, r))
            .collect()
    }

    /// What the test device has played since the last call, frame by
    /// frame: the main pair's left and right, then the cue pair's (M4
    /// slice 4; silent without a cue pair).
    pub fn heard_all(&mut self) -> Vec<[f32; 4]> {
        self.heard_frames()
    }

    /// What track `id`'s meter shows (M4 slice 5): the louder side's
    /// peak, RMS and hold line in dB, and the clip light.
    pub fn meter(&self, id: Id) -> MeterShown {
        let reading = self.arrangement.meters.reading(id.0);
        MeterShown {
            peak_db: reading.peak[0].max(reading.peak[1]),
            rms_db: reading.rms[0].max(reading.rms[1]),
            hold_db: reading.hold_db(),
            clipped: reading.clipped,
        }
    }

    /// The detail panel's height, in points.
    pub fn detail_size(&self) -> f32 {
        self.detail_size
    }

    /// Sets the detail panel's height, as opening a project with that
    /// layout saved does (the panel takes it on the next frame).
    pub fn set_detail_size(&mut self, size: f32) {
        self.detail_size = size;
    }

    /// Some meter is still moving, so the app keeps drawing frames.
    pub fn meters_moving(&self) -> bool {
        self.arrangement.meters.moving()
    }

    /// How the transport's mark is lit (ADR-0043): how far its sun has
    /// risen (0 to 1) and whether it is red for recording.
    pub fn mark(&self) -> (f32, bool) {
        (self.mark.rise, self.mark.recording)
    }

    /// The master's loudness as shown: momentary, short-term and
    /// integrated LUFS, and the true peak in dBTP.
    pub fn loudness(&self) -> (f64, f64, f64, f32) {
        let l = self.arrangement.meters.loudness();
        (l.momentary, l.short_term, l.integrated, l.true_peak)
    }

    /// A cue pair is open (chosen in the audio settings and running).
    pub fn cue_ready(&self) -> bool {
        self.transport.audio.cue_ready()
    }

    /// The metronome's output as the settings hold it: `main` or `cue`.
    pub fn saved_metronome_output(&self) -> &str {
        &self.settings.metronome.output
    }

    /// The MIDI switches the settings hold: (direction, name, track, remote).
    pub fn saved_midi_ports(&self) -> Vec<(String, String, bool, bool)> {
        let mut ports: Vec<_> = self
            .settings
            .midi
            .ports
            .iter()
            .map(|p| (p.direction.clone(), p.name.clone(), p.track, p.remote))
            .collect();
        ports.sort();
        ports
    }

    fn heard_frames(&mut self) -> Vec<[f32; 4]> {
        let Some(heard) = self.transport.audio.heard.as_mut() else {
            return Vec::new();
        };
        let Ok(chunk) = heard.read_chunk(heard.slots() / 4 * 4) else {
            return Vec::new();
        };
        let samples: Vec<f32> = chunk.into_iter().collect();
        samples.as_chunks::<4>().0.to_vec()
    }

    /// Background work is in progress: a save, an open, or files loading.
    pub fn is_busy(&self) -> bool {
        self.saving.is_some() || self.opening.is_some() || self.lanes.is_loading()
    }

    /// A problem with saving or opening, as shown to the user.
    pub fn file_notice(&self) -> Option<&str> {
        self.file_notice.as_deref()
    }

    /// Whether the audio file `media` is loaded with its waveform.
    pub fn has_waveform(&self, media: Id) -> bool {
        self.lanes
            .library
            .get(media.0)
            .is_some_and(|item| item.peaks.is_some())
    }

    /// The selected tracks.
    pub fn selected_tracks(&self) -> &[Id] {
        &self.arrangement.selected
    }

    /// The arrangement's horizontal view.
    pub fn view(&self) -> TimeView {
        self.nav.view
    }

    /// Jumps to `view`, as zooming and scrolling would.
    pub fn set_view(&mut self, view: TimeView) {
        self.nav.set(view);
    }

    /// The height of every track row together, in points.
    pub fn tracks_height(&self, ctx: &egui::Context) -> f32 {
        self.arrangement
            .height_of_content(ctx, self.document.project(), self.lanes.pending().len())
    }

    /// The loop the engine plays: start, end, on.
    pub fn engine_loop(&self) -> (Ticks, Ticks, bool) {
        let r = self.transport.loop_region;
        (r.start, r.end, r.enabled)
    }

    /// Where clip `id` is drawn on screen, if it is in view.
    pub fn clip_rect(&self, ctx: &egui::Context, id: Id) -> Option<egui::Rect> {
        self.arrangement
            .clip_rects(
                ctx,
                self.canvas,
                self.document.project(),
                self.lanes.pending().len(),
                self.nav.view,
            )
            .into_iter()
            .find(|(_, clip, _)| *clip == id)
            .map(|(_, _, rect)| rect)
    }

    /// Where clip `id` is drawn on the timeline now: on its way while it
    /// glides after an edit (M4.5), else where it is.
    pub fn drawn_clip_start(&self, ctx: &egui::Context, id: Id) -> Option<Ticks> {
        let now = ctx.input(|i| i.time);
        self.arrangement
            .drawn_start(self.document.project(), id, now)
    }

    /// How strongly clip `id` is washed after an undo (M4.5), 0 when not.
    pub fn clip_wash(&self, ctx: &egui::Context, id: Id) -> f32 {
        self.arrangement.wash(id, ctx.input(|i| i.time))
    }

    /// Where the handles of audio clip `id` are on screen (slice 6d), if it
    /// is in view and big enough to show them.
    pub fn clip_handles(&self, ctx: &egui::Context, id: Id) -> Option<ClipHandlesAt> {
        use crate::arrangement::handles::ClipHandles;
        use crate::arrangement::select::Edge;
        let rect = self.clip_rect(ctx, id)?;
        let clip = self
            .document
            .project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .find(|c| c.id == id)?;
        let view = self.nav.view;
        let left = self.canvas.left();
        let h = ClipHandles::of(clip, rect, |t| view.x(left, t.as_quarters()))?;
        Some(ClipHandlesAt {
            fade_in: h.handle_pos(Edge::Start),
            fade_out: h.handle_pos(Edge::End),
            bend_in: h.bend_pos(Edge::Start),
            bend_out: h.bend_pos(Edge::End),
            gain: egui::pos2(h.wave.center().x, h.gain_y()),
            wave: h.wave,
        })
    }

    /// The selected clips.
    pub fn selected_clips(&self) -> Vec<Id> {
        self.arrangement.clips.iter().copied().collect()
    }

    /// The selected time range: start, end and tracks.
    pub fn time_range(&self) -> Option<(Ticks, Ticks, Vec<Id>)> {
        self.arrangement
            .range
            .as_ref()
            .map(|r| (r.start, r.end, r.tracks.clone()))
    }

    /// The info line's lit action ("Ctrl+drag to copy"): the drag the
    /// keys held now choose.
    pub fn info_lit(&self, ctx: &egui::Context) -> Option<String> {
        let held = ctx.input(|i| crate::app::info::Held::of(i.modifiers));
        self.info.as_ref()?.lit_text(&self.keymap, held)
    }

    /// The live value drawn by the pointer in the last frame while dragging
    /// (the tape speed), if any.
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// What the info line says about what is under the pointer this frame
    /// (M4 slice 0), as words: "Clip “kick”: drag to move · …".
    pub fn info_line(&self) -> Option<String> {
        self.info.as_ref().map(|i| i.text(&self.keymap))
    }

    /// The short note shown after undo, save, open or a refused edit.
    pub fn note(&self) -> Option<&str> {
        self.toast.as_ref().map(|(text, _)| text.as_str())
    }

    /// What Undo would undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.document.history().undo_label()
    }

    /// Answers the next file dialog with `paths`, as if the user chose
    /// them (a dialog with no answer given is cancelled).
    pub fn answer_next_dialog(&mut self, paths: Vec<PathBuf>) {
        if let Some(answers) = &mut self.dialog_answers {
            answers.push_back(paths);
        }
    }

    /// Saves to `path`, as Save does once its dialog has a path.
    pub fn save_to(&mut self, path: PathBuf) {
        self.start_save(path);
    }

    /// Whether the command palette is open.
    pub fn palette_open(&self) -> bool {
        self.palette.is_open()
    }

    /// The palette's rows, top to bottom: what each says, and whether it
    /// can run now.
    pub fn palette_shows(&self, ctx: &egui::Context) -> Vec<(String, bool)> {
        self.palette_rows(ctx)
            .into_iter()
            .map(|row| (row.label, row.ready.is_ok()))
            .collect()
    }

    /// The name of the command the palette's top row runs, if it is a
    /// command (its label may say what it acts on instead).
    pub fn palette_first_command(&self, ctx: &egui::Context) -> Option<&'static str> {
        match self.palette_rows(ctx).first()?.entry {
            crate::app::palette::Entry::Command(command) => Some(command.name()),
            _ => None,
        }
    }

    /// The name of every command in the palette's table.
    pub fn command_names() -> Vec<&'static str> {
        crate::commands::ALL.iter().map(|info| info.name).collect()
    }

    /// The shortcuts of the command named `name`, as shown ("Ctrl+D").
    pub fn shortcuts_of(&self, name: &str) -> Vec<String> {
        let info = crate::commands::ALL
            .iter()
            .find(|info| info.name == name)
            .unwrap_or_else(|| panic!("no command named {name}"));
        self.keymap
            .chords(info.command)
            .iter()
            .map(|c| c.text())
            .collect()
    }

    /// The changed shortcuts the settings file will keep, by command id.
    pub fn saved_shortcuts(&self) -> std::collections::BTreeMap<String, Vec<String>> {
        self.settings.commands.shortcuts.clone()
    }

    /// The project as Save writes it: the document with the view stamped
    /// in (zoom, scroll, panels, grid, track heights).
    pub fn project_as_saved(&self) -> Project {
        self.stamped()
    }

    /// The pane whose anchor has the keyboard (F6), by name.
    pub fn focused_pane(&self, ctx: &egui::Context) -> Option<String> {
        crate::app::panes::focused(ctx).map(|p| format!("{p:?}"))
    }

    /// Whether the detail panel is shown.
    pub fn detail_shown(&self) -> bool {
        self.show_detail
    }

    /// Whether the Keyboard shortcuts window is open.
    pub fn shortcuts_open(&self) -> bool {
        self.shortcuts.open
    }

    /// Puts the system's cursor at `at` (or nowhere), as read while files
    /// are dragged in from the system, when egui has no pointer.
    pub fn set_system_cursor(&mut self, at: Option<egui::Pos2>) {
        self.system_cursor = at;
    }
}

/// Where a clip's fade and gain handles are on screen
/// ([`App::clip_handles`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipHandlesAt {
    /// The fade handles, at the top of the wave area.
    pub fade_in: egui::Pos2,
    pub fade_out: egui::Pos2,
    /// The bend dots, when a fade is long enough to show one.
    pub bend_in: Option<egui::Pos2>,
    pub bend_out: Option<egui::Pos2>,
    /// The middle of the gain line.
    pub gain: egui::Pos2,
    /// Where the waveform is drawn.
    pub wave: egui::Rect,
}

/// The `index`th colour of the clip and track palette, as the swatch
/// "Colour {index + 1}" sets it (slice 6e).
pub fn clip_colour(index: usize) -> lumen_project::model::Color {
    crate::arrangement::color_from(crate::theme::CLIP_COLORS[index])
}
