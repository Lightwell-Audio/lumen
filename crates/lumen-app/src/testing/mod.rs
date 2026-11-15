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

pub use crate::builtin::samples::{Library as SamplerLibrary, Source as SampleSource};
mod media_pool;
mod midi;
mod preferences;
mod sampler;
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

#[derive(Debug, Clone, Copy)]
pub enum NoteEdit {
    Move { time: Ticks, pitch: i32 },
    Delete,
    Mute(bool),
    Lengthen(Ticks),
}

pub fn install_renderer(cc: &eframe::CreationContext<'_>) -> Result<(), String> {
    match cc.wgpu_render_state.as_ref() {
        Some(state) => crate::canvas::install(state, None).map_err(|e| format!("renderer: {e}")),
        None => Ok(()),
    }
}

pub fn project_file(dir: &Path, name: &str) -> PathBuf {
    files::project_file_for(&dir.join(name))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterShown {
    pub peak_db: f32,
    pub rms_db: f32,
    pub hold_db: f32,
    pub clipped: bool,
}

pub const PROJECT_EXTENSION: &str = files::EXTENSION;

impl App {
    pub fn open_project_file(&mut self, path: PathBuf) {
        self.begin_opening(path);
    }

    pub fn project(&self) -> &Project {
        self.document.project()
    }

    pub fn is_dirty(&self) -> bool {
        self.document.is_dirty()
    }

    pub fn autosave_finished(&self) -> bool {
        self.autosave
            .as_ref()
            .is_some_and(|a| a.has_saved(self.document.change_mark()))
    }

    pub fn autosave_writing(&self) -> bool {
        self.autosave.as_ref().is_some_and(|a| a.is_writing())
    }

    pub fn detail_size(&self) -> f32 {
        self.detail_size
    }

    pub fn set_detail_size(&mut self, size: f32) {
        self.detail_size = size;
    }

    pub fn browser_size(&self) -> f32 {
        self.browser_size
    }

    pub fn set_browser_size(&mut self, size: f32) {
        self.browser_size = size;
    }

    pub fn panel_limits(&self) -> (f32, f32) {
        (crate::BROWSER_MIN, self.chain.least_panel())
    }

    pub fn chain_is_compact(&self) -> bool {
        self.chain.compact
    }

    pub fn show_cpu_load(&mut self, peak: f32) {
        self.transport.show_cpu_load(peak);
    }

    pub fn meters_moving(&self) -> bool {
        self.arrangement.meters.moving()
    }

    pub fn mark(&self) -> (f32, bool) {
        (self.mark.rise, self.mark.recording)
    }

    pub fn loudness(&self) -> (f64, f64, f64, f32) {
        let l = self.arrangement.meters.loudness();
        (l.momentary, l.short_term, l.integrated, l.true_peak)
    }

    pub fn cue_ready(&self) -> bool {
        self.transport.audio.cue_ready()
    }

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

    pub fn bounce_started(&self) -> bool {
        self.bouncing.is_some() || self.bounce_waiting.is_some()
    }

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

    pub fn templates(&self) -> Vec<String> {
        self.templates
            .listing
            .found()
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    pub fn dialog_folder(&self) -> Option<PathBuf> {
        self.dialog_start()
    }

    pub fn templates_folder(&self) -> PathBuf {
        self.templates_dir()
    }

    pub fn templates_busy(&self) -> bool {
        self.templates.listing.busy()
    }

    pub fn template_window_open(&self) -> bool {
        self.templates.naming_open()
    }

    pub fn revert_prompt_open(&self) -> bool {
        self.revert.asking()
    }

    pub fn reverted_changes(&self) -> Option<&Path> {
        self.revert.kept.as_deref()
    }

    pub fn project_settings_open(&self) -> bool {
        self.project_settings.is_open()
    }

    pub fn revealed_template_folders(&self) -> Vec<PathBuf> {
        self.templates.revealed.clone()
    }

    pub fn file_notice(&self) -> Option<&str> {
        self.file_notice.as_deref()
    }

    pub fn has_waveform(&self, media: Id) -> bool {
        self.lanes
            .library
            .get(media.0)
            .is_some_and(|item| item.peaks.is_some())
    }

    pub fn has_reversed_waveform(&self, media: Id) -> bool {
        self.has_waveform(Id(crate::media::reversed_key(media.0)))
    }

    pub fn selected_tracks(&self) -> &[Id] {
        &self.arrangement.selected
    }

    pub fn view(&self) -> TimeView {
        self.nav.view
    }

    pub fn set_view(&mut self, view: TimeView) {
        self.nav.set(view);
    }

    pub fn set_view_to(&mut self, left: f64, zoom: f64) {
        self.nav.set(TimeView { left, zoom });
    }
    pub fn tracks_height(&self, ctx: &egui::Context) -> f32 {
        self.arrangement
            .height_of_content(ctx, self.document.project(), self.lanes.pending().len())
    }

    pub fn engine_loop(&self) -> (Ticks, Ticks, bool) {
        let r = self.transport.loop_region;
        (r.start, r.end, r.enabled)
    }

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

    pub fn drawn_clip_start(&self, ctx: &egui::Context, id: Id) -> Option<Ticks> {
        let now = ctx.input(|i| i.time);
        self.arrangement
            .drawn_start(self.document.project(), id, now)
    }

    pub fn clip_wash(&self, ctx: &egui::Context, id: Id) -> f32 {
        self.arrangement.wash(id, ctx.input(|i| i.time))
    }

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

    pub fn selected_clips(&self) -> Vec<Id> {
        self.arrangement.clips.iter().copied().collect()
    }

    pub fn time_range(&self) -> Option<(Ticks, Ticks, Vec<Id>)> {
        self.arrangement
            .range
            .as_ref()
            .map(|r| (r.start, r.end, r.tracks.clone()))
    }

    pub fn pick_time_range(&mut self, start: Ticks, end: Ticks, tracks: &[Id]) {
        self.arrangement.range = Some(crate::arrangement::select::TimeRange {
            start,
            end,
            tracks: tracks.to_vec(),
        });
    }

    pub fn info_lit(&self, ctx: &egui::Context) -> Option<String> {
        let held = ctx.input(|i| crate::app::info::Held::of(i.modifiers));
        self.info.as_ref()?.lit_text(&self.keymap, held)
    }

    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    pub fn info_line(&self) -> Option<String> {
        self.info.as_ref().map(|i| i.text(&self.keymap))
    }

    pub fn note(&self) -> Option<&str> {
        self.toast.as_ref().map(|(text, _)| text.as_str())
    }

    pub fn say_note(&mut self, words: &str) {
        self.say(words.to_owned());
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.document.history().undo_label()
    }

    pub fn next_tap_at(&mut self, seconds: f64) {
        self.tap.next_time = Some(seconds);
    }

    pub fn answer_next_dialog(&mut self, paths: Vec<PathBuf>) {
        if let Some(answers) = &mut self.dialog_answers {
            answers.push_back(paths);
        }
    }

    pub fn save_to(&mut self, path: PathBuf) {
        self.start_save(path);
    }

    pub fn project_path(&self) -> Option<&Path> {
        self.document.path.as_deref()
    }

    pub fn recent_projects(&self) -> &[String] {
        &self.settings.projects.recent
    }

    pub fn set_copy_imported(&mut self, on: bool) {
        self.settings.projects.copy_imported = on;
    }

    pub fn palette_open(&self) -> bool {
        self.palette.is_open()
    }

    pub fn palette_shows(&self, ctx: &egui::Context) -> Vec<(String, bool)> {
        self.palette_rows(ctx)
            .into_iter()
            .map(|row| (row.label, row.ready.is_ok()))
            .collect()
    }

    pub fn palette_first_command(&self, ctx: &egui::Context) -> Option<&'static str> {
        match self.palette_rows(ctx).first()?.entry {
            crate::app::palette::Entry::Command(command) => Some(command.name()),
            _ => None,
        }
    }

    pub fn command_names() -> Vec<&'static str> {
        crate::commands::ALL.iter().map(|info| info.name).collect()
    }

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

    pub fn command_reason(&self, ctx: &egui::Context, name: &str) -> Option<&'static str> {
        let info = crate::commands::ALL
            .iter()
            .find(|info| info.name == name)
            .unwrap_or_else(|| panic!("no command named {name}"));
        self.availability(ctx, info.command).err()
    }

    pub fn saved_shortcuts(&self) -> std::collections::BTreeMap<String, Vec<String>> {
        self.settings.commands.shortcuts.clone()
    }

    pub fn project_as_saved(&self) -> Project {
        self.stamped()
    }

    pub fn focused_pane(&self, ctx: &egui::Context) -> Option<String> {
        crate::app::panes::focused(ctx).map(|p| format!("{p:?}"))
    }

    pub fn detail_shown(&self) -> bool {
        self.show_detail
    }

    pub fn shortcuts_open(&self) -> bool {
        self.shortcuts.open
    }

    pub fn select_tracks(&mut self, ids: &[Id]) {
        self.arrangement.selected = ids.to_vec();
    }

    pub fn select_all_clips_on_track(&mut self, index: usize) {
        self.arrangement.selected.clear();
        self.arrangement.range = None;
        self.arrangement.clips = self.document.project().tracks[index]
            .clips
            .iter()
            .map(|c| c.id)
            .collect();
    }

    pub fn select_clip(&mut self, id: Id) {
        self.arrangement.selected.clear();
        self.arrangement.clips = std::iter::once(id).collect();
    }

    pub fn select_clips(&mut self, ids: &[Id]) {
        self.arrangement.selected.clear();
        self.arrangement.clips = ids.iter().copied().collect();
    }

    pub fn mark_clips(&mut self, ids: &[Id]) {
        self.document.reverse_clips(ids);
        self.document.invert_clips(ids);
    }

    pub fn opened_audio_clip(&self) -> Option<Id> {
        self.detail_audio_clip()
    }

    pub fn mute_drum_sound(&mut self, map: Id, input: u8, mute: bool) {
        self.document.set_sound_mute(map, input, mute);
    }

    pub fn drum_choice_open(&self) -> bool {
        self.drum_choice.is_some()
    }

    pub fn drum_map_editor_open(&self) -> bool {
        self.drum_map.is_open()
    }

    pub fn drum_map_editor_map(&self) -> Option<Id> {
        self.drum_map.map()
    }

    pub fn drum_map_notice(&self) -> Option<&str> {
        self.drum_map.notice().map(|notice| notice.words.as_str())
    }

    pub fn drum_map_learning(&self) -> Option<u8> {
        self.drum_map.learning().map(|(_, input)| input)
    }

    pub fn drum_map_played(&mut self, key: u8) {
        self.drum_map_heard(key);
    }

    pub fn hiding_unmapped_keys(&self) -> bool {
        self.note_editor.hide_unmapped
    }

    pub fn hide_the_unmapped_keys(&mut self) {
        self.toggle_hide_unmapped_keys();
    }

    pub fn note_editor_rows(&self) -> Option<usize> {
        self.note_editor.last_view.map(|view| view.rows.len())
    }

    pub fn open_the_drum_map_editor(&mut self) {
        self.open_drum_map_editor();
    }

    pub fn open_clip_editor(&mut self, clip: Id) {
        self.open_note_editor(clip);
    }

    pub fn close_the_clip_editor(&mut self) {
        self.close_note_editor();
    }

    pub fn set_clip_gain_like_the_card(&mut self, clip: Id, db: f32) {
        self.document.set_clip_gain(&[clip], db, Some("card gain"));
    }

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

    pub fn browser_shown(&self) -> bool {
        self.show_browser
    }

    pub fn set_system_cursor(&mut self, at: Option<egui::Pos2>) {
        self.system_cursor = at;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipHandlesAt {
    pub fade_in: egui::Pos2,
    pub fade_out: egui::Pos2,
    pub bend_in: Option<egui::Pos2>,
    pub bend_out: Option<egui::Pos2>,
    pub gain: egui::Pos2,
    pub wave: egui::Rect,
    pub crossfade: bool,
}

pub fn clip_colour(index: usize) -> lumen_project::model::Color {
    crate::arrangement::color_from(crate::theme::CLIP_COLORS[index])
}

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
