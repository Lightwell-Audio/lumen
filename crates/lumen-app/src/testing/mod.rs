//! What the end-to-end tests (`tests/e2e/`, ADR-0029) may see and do,
//! beyond what a user sees on screen: a clean start, and read-only looks at
//! state the screen shows only as pixels (the playhead, the zoom). Tests
//! drive the app through its controls; this is the small, deliberate API
//! for the rest, so the app's internals stay private.

use std::path::{Path, PathBuf};

use eframe::egui;
use lumen_core::time::Ticks;
use lumen_project::model::{Id, Project};

use crate::timeview::TimeView;
use crate::{App, files};

mod audio_device;
mod audio_record;
mod automation;
mod browser_files;
pub(crate) mod coverage;
mod devices;
pub mod docs;
mod drum_icons;
mod drum_routing;
mod drums;
mod editor;
mod instance;
mod launch;
pub use instance::{HandOver, not_answering_window};
mod media_pool;
mod midi;
mod preferences;
mod streaming;
mod tools;
mod welcome;

pub use crate::audio::{
    BLOCKED_INPUT, InputDeviceTurn, TEST_BLOCKED_INPUT_DRIVER, TEST_DRIVER, TEST_HOLDS_DRIVER,
    TEST_LATE_DRIVER, TEST_REFUSES_DRIVER, TEST_SLOW_DRIVER, input_device, input_device_turn,
};
pub use crate::media::reversed_key;
pub use drum_icons::{drum_icon_sheet, prepare_look};
pub use editor::{AudioEditorShown, AudioHandlesShown, DetectionShown, PianoRollDemo};

/// A note edit for tests (`App::edit_notes`).
#[derive(Debug, Clone, Copy)]
pub enum NoteEdit {
    Move {
        time: Ticks,
        pitch: i32,
    },
    Delete,
    Mute(bool),
    /// Lengthens the end by this much.
    Lengthen(Ticks),
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
    /// Opens a project file as the Open command does after its dialog.
    pub fn open_project_file(&mut self, path: PathBuf) {
        self.begin_opening(path);
    }

    /// The open project.
    pub fn project(&self) -> &Project {
        self.document.project()
    }

    /// Unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.document.is_dirty()
    }

    /// The current document state has finished autosaving, including the
    /// worker's handoff of the session lock to the app.
    pub fn autosave_finished(&self) -> bool {
        self.autosave
            .as_ref()
            .is_some_and(|a| a.has_saved(self.document.change_mark()))
    }

    /// An autosave is being written on its worker thread.
    pub fn autosave_writing(&self) -> bool {
        self.autosave.as_ref().is_some_and(|a| a.is_writing())
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

    /// The browser's width, in points.
    pub fn browser_size(&self) -> f32 {
        self.browser_size
    }

    /// Sets the browser's width, as a saved layout does.
    pub fn set_browser_size(&mut self, size: f32) {
        self.browser_size = size;
    }

    /// The browser's least width and the least height of the detail panel
    /// on the Devices tab, in points.
    pub fn panel_limits(&self) -> (f32, f32) {
        (crate::BROWSER_MIN, self.chain.least_panel())
    }

    /// The device chain is drawn as compact cards (D2).
    pub fn chain_is_compact(&self) -> bool {
        self.chain.compact
    }

    /// Shows `peak` (1.0 is the whole buffer) as the CPU load for the next
    /// second, as if the engine had measured it.
    pub fn show_cpu_load(&mut self, peak: f32) {
        self.transport.show_cpu_load(peak);
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

    /// A bounce has started: it waits on the plugins or is rendering.
    pub fn bounce_started(&self) -> bool {
        self.bouncing.is_some() || self.bounce_waiting.is_some()
    }

    /// Background work is in progress: a save, an open, or files loading.
    pub fn is_busy(&self) -> bool {
        self.export.busy()
            || self.saving.is_some()
            || self.save_waiting.is_some()
            || self.collecting.is_some()
            || self.opening.is_some()
            || self.lanes.is_loading()
            || self.midi_reader.busy()
            || self.midi_exporting.is_some()
            || self.midi_writing.is_some()
    }

    /// The templates last found in the templates folder, by name.
    pub fn templates(&self) -> Vec<String> {
        self.templates
            .listing
            .found()
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    /// Where a Save as or Open dialog starts for this project.
    pub fn dialog_folder(&self) -> Option<PathBuf> {
        self.dialog_start()
    }

    /// The folder templates are kept in.
    pub fn templates_folder(&self) -> PathBuf {
        self.templates_dir()
    }

    /// Whether the templates folder is being listed.
    pub fn templates_busy(&self) -> bool {
        self.templates.listing.busy()
    }

    /// Whether the window that names a template is showing.
    pub fn template_window_open(&self) -> bool {
        self.templates.naming_open()
    }

    /// Whether the prompt that asks before Revert to saved is showing.
    pub fn revert_prompt_open(&self) -> bool {
        self.revert.asking()
    }

    /// Where the last revert put the changes it dropped, while the message
    /// that offers them shows.
    pub fn reverted_changes(&self) -> Option<&Path> {
        self.revert.kept.as_deref()
    }

    /// Whether the Project settings window is showing.
    pub fn project_settings_open(&self) -> bool {
        self.project_settings.is_open()
    }

    /// The folders "Open templates folder" showed, oldest first.
    pub fn revealed_template_folders(&self) -> Vec<PathBuf> {
        self.templates.revealed.clone()
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

    /// Whether the reversed copy of the audio file `media` is ready, with
    /// its waveform (M7b slice E1).
    pub fn has_reversed_waveform(&self, media: Id) -> bool {
        self.has_waveform(Id(crate::media::reversed_key(media.0)))
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

    /// Sets the arrangement's view to `left` beats at `zoom` points a beat, as
    /// zooming and scrolling would (a deep zoom for a test).
    pub fn set_view_to(&mut self, left: f64, zoom: f64) {
        self.nav.set(TimeView { left, zoom });
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
        let project = self.document.project();
        let track = project
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == id))?;
        let clip = track.clips.iter().find(|c| c.id == id)?;
        let shown = crate::crossfades::preview(project, track.id, clip);
        let view = self.nav.view;
        let left = self.canvas.left();
        let h = ClipHandles::of(&shown, rect, |t| view.x(left, t.as_quarters()))?;
        Some(ClipHandlesAt {
            fade_in: h.handle_pos(Edge::Start),
            fade_out: h.handle_pos(Edge::End),
            bend_in: h.bend_pos(Edge::Start),
            bend_out: h.bend_pos(Edge::End),
            gain: egui::pos2(h.wave.center().x, h.gain_y()),
            wave: h.wave,
            crossfade: h.crossfade,
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

    /// Picks a time range over `tracks`, as a drag in the arrangement would.
    pub fn pick_time_range(&mut self, start: Ticks, end: Ticks, tracks: &[Id]) {
        self.arrangement.range = Some(crate::arrangement::select::TimeRange {
            start,
            end,
            tracks: tracks.to_vec(),
        });
    }

    /// The info line's lit action ("Ctrl+drag to copy"): the first that
    /// the keys held now do; none while no key is held.
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

    /// Shows `words` as the short note (the toast), as a command would.
    pub fn say_note(&mut self, words: &str) {
        self.say(words.to_owned());
    }

    /// What Undo would undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.document.history().undo_label()
    }

    /// Times the next tap tempo at `seconds` instead of frame time, so a
    /// test's taps are exact.
    pub fn next_tap_at(&mut self, seconds: f64) {
        self.tap.next_time = Some(seconds);
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

    /// The open project's file, once it has one.
    pub fn project_path(&self) -> Option<&Path> {
        self.document.path.as_deref()
    }

    /// The recent projects, newest first.
    pub fn recent_projects(&self) -> &[String] {
        &self.settings.projects.recent
    }

    /// Turns "Copy imported audio into the project" on or off, as the switch
    /// in the menu does.
    pub fn set_copy_imported(&mut self, on: bool) {
        self.settings.projects.copy_imported = on;
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

    /// Why the command named `name` cannot run now, as the palette and the
    /// menus say it; `None` when it can (the commands' own reasons, review
    /// L4).
    pub fn command_reason(&self, ctx: &egui::Context, name: &str) -> Option<&'static str> {
        let info = crate::commands::ALL
            .iter()
            .find(|info| info.name == name)
            .unwrap_or_else(|| panic!("no command named {name}"));
        self.availability(ctx, info.command).err()
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

    /// Selects these tracks, as clicking their headers would.
    pub fn select_tracks(&mut self, ids: &[Id]) {
        self.arrangement.selected = ids.to_vec();
    }

    /// Selects every clip on track `index`, as a marquee over them would.
    pub fn select_all_clips_on_track(&mut self, index: usize) {
        self.arrangement.selected.clear();
        self.arrangement.range = None;
        self.arrangement.clips = self.document.project().tracks[index]
            .clips
            .iter()
            .map(|c| c.id)
            .collect();
    }

    /// Selects this one clip, as clicking it would.
    pub fn select_clip(&mut self, id: Id) {
        self.arrangement.selected.clear();
        self.arrangement.clips = std::iter::once(id).collect();
    }

    /// Selects exactly these clips, as a marquee would.
    pub fn select_clips(&mut self, ids: &[Id]) {
        self.arrangement.selected.clear();
        self.arrangement.clips = ids.iter().copied().collect();
    }

    /// Reverses and inverts the polarity of these clips, as the two
    /// commands do (for the clip marks' screenshots).
    pub fn mark_clips(&mut self, ids: &[Id]) {
        self.document.reverse_clips(ids);
        self.document.invert_clips(ids);
    }

    /// The audio clip the Clip tab has open, if any.
    pub fn opened_audio_clip(&self) -> Option<Id> {
        self.detail_audio_clip()
    }

    /// Mutes or unmutes a drum sound, as D1's map editor will (M8 D0 ships no
    /// editor, so the scenarios that hear a sound gone do it through this).
    /// One undo step.
    pub fn mute_drum_sound(&mut self, map: Id, input: u8, mute: bool) {
        self.document.set_sound_mute(map, input, mute);
    }

    /// Whether the drum map chooser is open (M8 D0).
    pub fn drum_choice_open(&self) -> bool {
        self.drum_choice.is_some()
    }

    /// Whether the drum map editor is open (M8 D1).
    pub fn drum_map_editor_open(&self) -> bool {
        self.drum_map.is_open()
    }

    /// The map the drum map editor shows.
    pub fn drum_map_editor_map(&self) -> Option<Id> {
        self.drum_map.map()
    }

    /// What the drum map editor last said about a file (its notice).
    pub fn drum_map_notice(&self) -> Option<&str> {
        self.drum_map.notice().map(|notice| notice.words.as_str())
    }

    /// The sound the map editor is waiting for a note on.
    pub fn drum_map_learning(&self) -> Option<u8> {
        self.drum_map.learning().map(|(_, input)| input)
    }

    /// Plays `key` on an input, as the live feed tells the editor (the
    /// Learn button's note).
    pub fn drum_map_played(&mut self, key: u8) {
        self.drum_map_heard(key);
    }

    /// Whether the clip editor's key column shows only the map's sounds.
    pub fn hiding_unmapped_keys(&self) -> bool {
        self.note_editor.hide_unmapped
    }

    /// Folds the clip editor's key column to the map's sounds (the Keys
    /// button, for screenshots).
    pub fn hide_the_unmapped_keys(&mut self) {
        self.toggle_hide_unmapped_keys();
    }

    /// How many pitch rows the clip editor drew this frame (128 on a track
    /// with no map, or the map's sounds when they are folded).
    pub fn note_editor_rows(&self) -> Option<usize> {
        self.note_editor.last_view.map(|view| view.rows.len())
    }

    /// Opens the drum map editor on the selected track, as the command does.
    pub fn open_the_drum_map_editor(&mut self) {
        self.open_drum_map_editor();
    }

    /// Opens a MIDI clip in the clip editor, as a double-click does.
    pub fn open_clip_editor(&mut self, clip: Id) {
        self.open_note_editor(clip);
    }

    /// Closes the clip editor, as the clip going does.
    pub fn close_the_clip_editor(&mut self) {
        self.close_note_editor();
    }

    /// Sets a clip's gain as the properties card does (one undo step).
    pub fn set_clip_gain_like_the_card(&mut self, clip: Id, db: f32) {
        self.document.set_clip_gain(&[clip], db, Some("card gain"));
    }

    /// Trims the end of a clip to `to`, as dragging its right edge would;
    /// with `looping` the clip repeats to fill the new length.
    pub fn trim_clip_end_to(&mut self, clip: Id, to: lumen_core::time::Ticks, looping: bool) {
        let base = self.document.project().clone();
        self.document.trim_clip(
            &base,
            clip,
            lumen_project::edit::Edge::End,
            to,
            looping,
            None,
        );
    }

    /// Trims the start of a clip to `to`, as dragging its left edge would.
    pub fn trim_clip_start_to(&mut self, clip: Id, to: lumen_core::time::Ticks) {
        let base = self.document.project().clone();
        self.document.trim_clip(
            &base,
            clip,
            lumen_project::edit::Edge::Start,
            to,
            false,
            None,
        );
    }

    /// Moves a clip to `start` on its own track, as dragging it would.
    pub fn move_clip_to(&mut self, clip: Id, start: lumen_core::time::Ticks) {
        let base = self.document.project().clone();
        let Some(track) = base
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip))
        else {
            return;
        };
        let moves = [crate::document::ClipMove {
            clip,
            track: track.id,
            start,
        }];
        self.document.move_clips(&base, &moves, None);
    }

    /// Whether the browser pane is shown.
    pub fn browser_shown(&self) -> bool {
        self.show_browser
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
    /// The clip's fade-out is a crossfade into the clip after it.
    pub crossfade: bool,
}

/// The `index`th colour of the clip and track palette, as the swatch
/// "Color {index + 1}" sets it (slice 6e).
pub fn clip_colour(index: usize) -> lumen_project::model::Color {
    crate::arrangement::color_from(crate::theme::CLIP_COLORS[index])
}

/// Export's task and report are visible as labels; these read-only looks let
/// scenarios wait on completion without depending on worker timing.
impl App {
    pub fn audio_export_open(&self) -> bool {
        self.export.open
    }
    pub fn audio_export_busy(&self) -> bool {
        self.export.busy()
    }
    pub fn audio_export_error(&self) -> Option<&str> {
        self.export.error.as_deref()
    }
    pub fn audio_export_files(&self) -> Vec<PathBuf> {
        self.export.reports.iter().map(|r| r.path.clone()).collect()
    }
}
