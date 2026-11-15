use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::History;
use lumen_project::automation::Target;
use lumen_project::model::{Color, Device, Id, Marker, Monitor, Project, Track, TrackKind};

pub const MAX_BEATS_PER_BAR: u32 = 64;

mod audio_takes;
mod automation;
mod bounce;
mod clip_ops;
mod clips;
mod consolidate;
mod crossfades;
mod devices;
mod drums;
mod drums_edit;
mod drums_route;
mod freeze;
mod groups;
mod import;
pub(crate) mod instrument;
mod lanes;
mod mappings;
mod media;
mod midi_clips;
mod midi_import;
mod mixer;
mod notes;
mod outputs;
mod overrides;
mod record;
mod settings;
mod song_maps;
mod split;
mod takes;
pub(crate) mod warp;
pub use automation::LaneRefusal;
pub use clips::{CLIP_GAIN, ClipMove};
pub use devices::{Refusal, Slot, find_device, name_of as device_name};
pub use drums_route::Routed;
pub use groups::Place;
pub use import::{Awaiting, DropInto, FollowTempo, Imported, Places, import_place};
pub use lanes::{Drag, Line, Shift};
pub use midi_clips::NewClip;
pub use mixer::Refused;
pub use notes::{LaneMove, Resize};
pub use outputs::{Leave, OutputEdit};
pub use overrides::Overrides;
pub use record::Recording;
pub use settings::RATES;

fn track_color(chosen: Option<Color>, index: usize) -> Color {
    chosen.unwrap_or_else(|| Color::in_turn(index))
}

pub struct Document {
    history: History,
    pub path: Option<PathBuf>,
    saved: u64,
    inside: u64,
    inside_saved: u64,
    inside_at_save: u64,
    next_id: u64,
    pub media_files: HashMap<Id, PathBuf>,
    session: u64,
    overrides: Overrides,
    touched: Vec<(Id, Target)>,
    rests: BTreeMap<(Id, Target), f32>,
    lock_envelopes: bool,
    open_inputs: u16,
    new_tracks: NewTracks,
    follow_tempo: FollowTempo,
    awaiting_follow: Vec<import::Awaiting>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewTracks {
    pub monitor: Monitor,
    pub instrument: Option<lumen_devices::Preset>,
}

impl NewTracks {
    pub(crate) fn give(&self, track: &mut Track) {
        if track.kind == TrackKind::Audio {
            track.monitor = self.monitor;
        }
    }

    pub(crate) fn instrument(&self, id: Id) -> Vec<Device> {
        self.instrument
            .map(|preset| instrument::synth_device(id, preset))
            .into_iter()
            .collect()
    }
}

impl Default for NewTracks {
    fn default() -> Self {
        Self {
            monitor: Monitor::Auto,
            instrument: Some(instrument::DEFAULT_PRESET),
        }
    }
}

impl Document {
    pub fn new(mut project: Project) -> Self {
        instrument::fill_synth_values(&mut project);
        let next_id = highest_id(&project) + 1;
        let history = History::new(project);
        static SESSIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            saved: history.state(),
            inside: 0,
            inside_saved: 0,
            inside_at_save: 0,
            history,
            path: None,
            next_id,
            media_files: HashMap::new(),
            session: SESSIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            overrides: Overrides::default(),
            touched: Vec::new(),
            rests: BTreeMap::new(),
            lock_envelopes: false,
            open_inputs: 0,
            new_tracks: NewTracks::default(),
            follow_tempo: FollowTempo::default(),
            awaiting_follow: Vec::new(),
        }
    }

    pub fn set_open_inputs(&mut self, open: u16) {
        self.open_inputs = open;
    }

    pub fn set_lock_envelopes(&mut self, lock: bool) {
        self.lock_envelopes = lock;
    }

    pub fn set_new_tracks(&mut self, new_tracks: NewTracks) {
        self.new_tracks = new_tracks;
    }

    pub fn set_follow_tempo(&mut self, follow: FollowTempo) {
        self.follow_tempo = follow;
    }

    pub fn set_undo_limit(&mut self, steps: usize) {
        self.history.set_limit(steps);
    }

    pub fn undo_limit(&self) -> usize {
        self.history.limit()
    }

    pub fn session(&self) -> u64 {
        self.session
    }

    pub fn project(&self) -> &Project {
        self.history.project()
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn state(&self) -> u64 {
        self.history.state()
    }

    pub fn is_dirty(&self) -> bool {
        self.history.state() != self.saved || self.inside != self.inside_saved
    }

    pub fn mark_changed_inside(&mut self) {
        self.inside += 1;
    }

    pub fn begin_save(&mut self) {
        self.inside_at_save = self.inside;
    }

    pub fn change_mark(&self) -> u64 {
        self.history
            .state()
            .wrapping_add(self.inside.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    pub fn name(&self) -> String {
        self.path
            .as_deref()
            .and_then(|p| p.file_stem())
            .map_or_else(
                || "Untitled".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            )
    }

    pub fn title(&self) -> String {
        let dot = if self.is_dirty() { " •" } else { "" };
        format!(
            "{}{dot} — {}",
            self.name(),
            lumen_core::product::display_name()
        )
    }

    pub fn mark_saved_at(&mut self, state: u64) {
        self.saved = state;
        self.inside_saved = self.inside_at_save;
    }

    pub fn recovered(
        project: Project,
        path: Option<PathBuf>,
        media_files: HashMap<Id, PathBuf>,
    ) -> Self {
        let mut doc = Self::new(project);
        doc.path = path;
        doc.media_files = media_files;
        doc.saved = u64::MAX;
        doc
    }

    pub fn opened(project: Project, path: PathBuf, media_files: HashMap<Id, PathBuf>) -> Self {
        let mut doc = Self::new(project);
        doc.path = Some(path);
        doc.media_files = media_files;
        doc
    }

    pub fn from_template(project: Project, media_files: HashMap<Id, PathBuf>) -> Self {
        let mut doc = Self::new(project);
        doc.media_files = media_files;
        doc
    }

    pub fn fresh_id(&mut self) -> Id {
        let id = Id(self.next_id);
        self.next_id += 1;
        id
    }

    #[cfg(test)]
    pub fn apply(
        &mut self,
        label: &str,
        merge: Option<&str>,
        edit: impl FnOnce(&mut Project),
    ) -> bool {
        self.history.apply(label, merge, edit)
    }

    pub fn seal(&mut self) {
        self.history.seal();
    }

    pub fn undo(&mut self) -> Option<String> {
        self.history.undo()
    }

    pub fn redo(&mut self) -> Option<String> {
        self.history.redo()
    }

    pub fn jump_to(&mut self, done: usize) {
        self.history.jump_to(done);
    }
}

impl Document {
    pub fn add_track(&mut self, kind: TrackKind, after: Option<Id>) -> Id {
        self.insert_track(kind, after, None)
    }

    pub(super) fn insert_track(
        &mut self,
        kind: TrackKind,
        after: Option<Id>,
        first: Option<Device>,
    ) -> Id {
        let id = self.fresh_id();
        let (word, label) = match kind {
            TrackKind::Audio => ("Audio", "Add audio track"),
            TrackKind::Midi => ("MIDI", "Add MIDI track"),
            TrackKind::Group => ("Group", "Add group track"),
            TrackKind::Return => ("Return", "Add return track"),
            TrackKind::Master => return self.project().master.id,
        };
        if kind == TrackKind::Return && self.returns_full() {
            let last = self.project().tracks.iter().rev();
            return last
                .clone()
                .find(|t| t.kind == TrackKind::Return)
                .map_or(self.project().master.id, |t| t.id);
        }
        let label = match &first {
            Some(device) => format!("Add {} on a new track", devices::name_of(device)),
            None => label.to_owned(),
        };
        let new_tracks = self.new_tracks;
        let synth = new_tracks
            .instrument
            .filter(|_| kind == TrackKind::Midi && first.is_none())
            .map(|preset| (self.fresh_id(), preset));
        self.history.apply(&label, None, |project| {
            let number = project.tracks.iter().filter(|t| t.kind == kind).count() + 1;
            let after = after.and_then(|a| project.tracks.iter().position(|t| t.id == a));
            let index = after.map_or(project.tracks.len(), |i| {
                lumen_project::groups::span(&project.tracks, i).end
            });
            let parent = after
                .and_then(|i| project.tracks[i].parent)
                .filter(|_| lumen_project::groups::can_be_grouped(kind));
            let color = track_color(project.new_track_color, project.tracks.len());
            let mut track = Track {
                parent,
                devices: first
                    .clone()
                    .or_else(|| synth.map(|(d, preset)| instrument::synth_device(d, preset)))
                    .into_iter()
                    .collect(),
                ..Track::new(id, format!("{word} {number}"), kind, color)
            };
            new_tracks.give(&mut track);
            project.tracks.insert(index, Arc::new(track));
            lumen_project::groups::normalize(&mut project.tracks);
        });
        id
    }

    pub fn returns_full(&self) -> bool {
        let returns = self.project().tracks.iter();
        returns.filter(|t| t.kind == TrackKind::Return).count() >= mixer::MAX_RETURNS
    }

    pub fn delete_tracks(&mut self, ids: &[Id]) {
        let ids = lumen_project::groups::with_contents(&self.project().tracks, ids);
        let label = plural(ids.len(), "Delete track", "Delete {} tracks");
        self.history.apply(&label, None, |project| {
            project.tracks.retain(|t| !ids.contains(&t.id));
            mixer::forget_routes_to(project, &ids);
        });
    }

    pub fn duplicate_tracks(&mut self, ids: &[Id]) -> Vec<Id> {
        let mut heads = lumen_project::groups::outermost(&self.project().tracks, ids);
        let tracks = &self.project().tracks;
        let mut room = mixer::MAX_RETURNS.saturating_sub(
            tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Return)
                .count(),
        );
        heads.retain(|id| {
            let is_return = tracks
                .iter()
                .any(|t| t.id == *id && t.kind == TrackKind::Return);
            if !is_return {
                return true;
            }
            let fits = room > 0;
            room = room.saturating_sub(1);
            fits
        });
        if heads.is_empty() {
            return Vec::new();
        }
        let tracks = self.project().tracks.clone();
        let runs = groups::copy_runs(&tracks, &heads, || self.fresh_id());
        let new_ids: Vec<Id> = runs
            .iter()
            .filter_map(|(_, run)| run.first())
            .map(|t| t.id)
            .collect();
        let label = plural(runs.len(), "Duplicate track", "Duplicate {} tracks");
        self.history.apply(&label, None, |project| {
            for (original, run) in runs {
                if let Some(i) = project.tracks.iter().position(|t| t.id == original) {
                    let end = lumen_project::groups::span(&project.tracks, i).end;
                    project
                        .tracks
                        .splice(end..end, run.into_iter().map(Arc::new));
                }
            }
        });
        new_ids
    }

    pub fn rename_track(&mut self, id: Id, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.edit_tracks("Rename track", &[id], |t| t.name = name.to_owned());
    }

    pub fn recolor_tracks(&mut self, ids: &[Id], color: Color) {
        self.edit_tracks("Change track color", ids, |t| t.color = color);
    }

    pub fn set_mute(&mut self, ids: &[Id], mute: bool) {
        for &id in ids {
            self.moved(id, Target::Mute);
        }
        let label = if mute { "Mute" } else { "Unmute" };
        self.edit_tracks(label, ids, |t| t.mute = mute);
    }

    pub fn set_solo(&mut self, ids: &[Id], solo: bool, exclusive: bool) {
        let label = if solo { "Solo" } else { "Unsolo" };
        self.history.apply(label, None, |project| {
            for track in &mut project.tracks {
                let wanted = if ids.contains(&track.id) {
                    solo
                } else if exclusive {
                    false
                } else {
                    track.solo
                };
                if track.solo != wanted {
                    Arc::make_mut(track).solo = wanted;
                }
            }
        });
    }

    pub fn clear_solos(&mut self) {
        self.history.apply("Clear solos", None, |project| {
            for track in &mut project.tracks {
                if track.solo {
                    Arc::make_mut(track).solo = false;
                }
            }
        });
    }

    pub fn set_armed(&mut self, ids: &[Id], armed: bool) {
        let label = if armed { "Arm for recording" } else { "Disarm" };
        let ids = self.armable(ids);
        let open = self.open_inputs;
        self.history.apply(label, None, |project| {
            for track in &mut project.tracks {
                if ids.contains(&track.id) && track.armed != armed {
                    Arc::make_mut(track).armed = armed;
                }
            }
            if armed {
                lumen_project::edit::input::route_armed(project, &ids, open);
            }
        });
    }

    pub fn arm_only(&mut self, ids: &[Id]) {
        let ids = self.armable(ids);
        let open = self.open_inputs;
        self.history.apply("Arm for recording", None, |project| {
            for track in &mut project.tracks {
                let armed = ids.contains(&track.id);
                if track.armed != armed {
                    Arc::make_mut(track).armed = armed;
                }
            }
            lumen_project::edit::input::route_armed(project, &ids, open);
        });
    }

    fn armable(&self, ids: &[Id]) -> Vec<Id> {
        let tracks = &self.project().tracks;
        lumen_project::groups::with_contents(tracks, ids)
            .into_iter()
            .filter(|id| {
                tracks
                    .iter()
                    .any(|t| t.id == *id && t.kind != TrackKind::Group)
            })
            .collect()
    }

    pub fn set_volume(&mut self, id: Id, db: f32) {
        let db = db.clamp(-60.0, 6.0);
        self.moved(id, Target::Volume);
        self.history
            .apply("Change volume", Some(&format!("volume {:x}", id.0)), |p| {
                if let Some(t) = mixer::tracks_mut(p).find(|t| t.id == id) {
                    Arc::make_mut(t).gain_db = db;
                }
            });
    }

    pub fn set_pan(&mut self, id: Id, pan: f32) {
        let pan = pan.clamp(-1.0, 1.0);
        self.moved(id, Target::Pan);
        self.history
            .apply("Change pan", Some(&format!("pan {:x}", id.0)), |p| {
                if let Some(t) = mixer::tracks_mut(p).find(|t| t.id == id) {
                    Arc::make_mut(t).pan = pan;
                }
            });
    }

    pub fn set_loop(
        &mut self,
        label: &str,
        merge: Option<&str>,
        start: Ticks,
        end: Ticks,
        enabled: bool,
    ) {
        if end <= start || start < Ticks::ZERO {
            return;
        }
        self.history.apply(label, merge, |project| {
            let t = &mut project.transport;
            t.loop_start = start;
            t.loop_end = end;
            t.loop_enabled = enabled;
        });
    }

    pub fn set_loop_enabled(&mut self, enabled: bool) {
        let label = if enabled { "Loop on" } else { "Loop off" };
        self.history.apply(label, None, |project| {
            project.transport.loop_enabled = enabled
        });
    }

    pub fn set_punch(&mut self, punch_in: bool, punch_out: bool) {
        self.history.apply("Punch", None, |project| {
            project.transport.punch_in = punch_in;
            project.transport.punch_out = punch_out;
        });
    }

    pub fn add_marker(&mut self, at: Ticks, name: Option<&str>) -> Id {
        let id = self.fresh_id();
        let at = at.max(Ticks::ZERO);
        self.history.apply("Add locator", None, |project| {
            let name = name.map_or_else(|| next_marker_name(&project.markers), str::to_owned);
            project.markers.push(Marker { id, at, name });
            sort_markers(&mut project.markers);
        });
        id
    }

    pub fn move_marker(&mut self, id: Id, at: Ticks, merge: Option<&str>) {
        let at = at.max(Ticks::ZERO);
        self.history.apply("Move locator", merge, |project| {
            if let Some(m) = project.markers.iter_mut().find(|m| m.id == id) {
                m.at = at;
            }
            sort_markers(&mut project.markers);
        });
    }

    pub fn rename_marker(&mut self, id: Id, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.history.apply("Rename locator", None, |project| {
            if let Some(m) = project.markers.iter_mut().find(|m| m.id == id) {
                m.name = name.to_owned();
            }
        });
    }

    pub fn delete_marker(&mut self, id: Id) {
        self.history.apply("Delete locator", None, |project| {
            project.markers.retain(|m| m.id != id);
        });
    }

    fn edit_tracks(&mut self, label: &str, ids: &[Id], change: impl Fn(&mut Track)) {
        self.history.apply(label, None, |project| {
            for track in &mut project.tracks {
                if ids.contains(&track.id) {
                    let mut edited = (**track).clone();
                    change(&mut edited);
                    if edited != **track {
                        *track = Arc::new(edited);
                    }
                }
            }
        });
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_owned()
    } else {
        many.replace("{}", &n.to_string())
    }
}

pub fn audible(project: &Project) -> impl Fn(&Track) -> bool + use<> {
    let heard = lumen_project::solo::hearing(project);
    move |t: &Track| heard.of(t.id).output
}

fn tempo_map(project: &Project) -> Option<lumen_core::time::TempoMap> {
    lumen_core::time::TempoMap::new(project.tempo.clone()).ok()
}

fn seconds_at(project: &Project, at: Ticks) -> f64 {
    tempo_map(project).map_or(0.0, |m| m.seconds_at(at))
}

fn ticks_at_seconds(project: &Project, seconds: f64) -> Ticks {
    tempo_map(project).map_or(Ticks::ZERO, |m| m.ticks_at_seconds(seconds))
}

fn sort_markers(markers: &mut [Marker]) {
    markers.sort_by_key(|m| m.at);
}

fn next_marker_name(markers: &[Marker]) -> String {
    let n = markers
        .iter()
        .filter_map(|m| m.name.strip_prefix("Locator ")?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("Locator {}", n + 1)
}

fn highest_id(project: &Project) -> u64 {
    let tracks = lumen_project::routing::all_tracks(project).flat_map(|t| {
        std::iter::once(t.id.0)
            .chain(t.devices.iter().map(|d| d.id.0))
            .chain(t.clips.iter().map(|c| c.id.0))
            .chain(t.frozen.iter().map(|f| f.id.0))
    });
    tracks
        .chain(project.media.iter().map(|m| m.id.0))
        .chain(project.markers.iter().map(|m| m.id.0))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_inside_a_plugin_is_unsaved_until_a_save_that_began_after_it() {
        let mut doc = Document::new(crate::empty_project());
        assert!(!doc.is_dirty());
        let undoable = doc.history().state();
        doc.mark_changed_inside();
        assert!(doc.is_dirty(), "a patch is a change");
        assert_eq!(doc.history().state(), undoable, "but nothing to undo");
        let before = doc.change_mark();
        doc.begin_save();
        let state = doc.state();
        doc.mark_changed_inside();
        assert_ne!(doc.change_mark(), before, "an autosave sees it");
        doc.mark_saved_at(state);
        assert!(doc.is_dirty(), "the later change is not in the file");
        doc.begin_save();
        doc.mark_saved_at(doc.state());
        assert!(!doc.is_dirty());
    }

    #[test]
    fn new_tracks_start_with_the_chosen_monitoring_and_instrument() {
        use lumen_devices::Preset;
        use lumen_project::model::PluginRef;
        let mut doc = Document::new(crate::empty_project());
        let track = |doc: &Document, id: Id| {
            doc.project()
                .tracks
                .iter()
                .find(|t| t.id == id)
                .cloned()
                .unwrap()
        };
        let audio = doc.add_track(TrackKind::Audio, None);
        let midi = doc.add_track(TrackKind::Midi, None);
        assert_eq!(track(&doc, audio).monitor, Monitor::Auto);
        let synth = &track(&doc, midi).devices;
        assert_eq!(synth.len(), 1);
        assert_eq!(synth[0].state, b"poly");

        doc.set_new_tracks(NewTracks {
            monitor: Monitor::In,
            instrument: Some(Preset::Pad),
        });
        let audio = doc.add_track(TrackKind::Audio, None);
        let midi = doc.add_track(TrackKind::Midi, None);
        assert_eq!(track(&doc, audio).monitor, Monitor::In);
        assert_eq!(track(&doc, midi).devices[0].state, b"pad");
        let (dropped, _) = doc
            .add_device_on_new_track(
                PluginRef::Vst3 {
                    class_id: "0123456789ABCDEF0123456789ABCDEF".into(),
                    name: "Echo".into(),
                    vendor: String::new(),
                    kind: lumen_project::model::DeviceKind::Effect,
                },
                None,
            )
            .unwrap();
        assert_eq!(track(&doc, dropped).monitor, Monitor::In);
        doc.import(vec![file(Id(900), "drums", Ticks::ZERO)]);
        let imported = doc.project().tracks.last().unwrap();
        assert_eq!(imported.name, "drums");
        assert_eq!(imported.monitor, Monitor::In);

        doc.set_new_tracks(NewTracks {
            monitor: Monitor::Off,
            instrument: None,
        });
        let midi = doc.add_track(TrackKind::Midi, None);
        assert!(track(&doc, midi).devices.is_empty(), "no instrument");
        let group = doc.add_track(TrackKind::Group, None);
        assert_eq!(track(&doc, group).monitor, Monitor::Auto);
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn the_undo_limit_drops_the_oldest_steps_past_it() {
        let mut doc = Document::new(crate::empty_project());
        doc.set_undo_limit(1_000);
        assert_eq!(doc.undo_limit(), 1_000);
        for _ in 0..1_005 {
            doc.add_track(TrackKind::Audio, None);
        }
        assert_eq!(doc.history().steps().0.len(), 1_000);
    }

    pub(super) fn file(media: Id, name: &str, at: Ticks) -> Imported {
        Imported {
            media,
            path: PathBuf::from(format!("C:/audio/{name}.wav")),
            name: name.into(),
            sample_rate: 48_000,
            channels: 2,
            frames: 96_000,
            size_bytes: 384_044,
            tempo: None,
            at,
            into: DropInto::NewTracks {
                before: None,
                group: None,
            },
        }
    }

    #[test]
    fn locators_are_kept_in_order_and_each_change_is_one_step() {
        let mut doc = Document::new(crate::empty_project());
        let b = doc.add_marker(Ticks::quarters(8), None);
        let a = doc.add_marker(Ticks::quarters(4), None);
        let names: Vec<_> = doc
            .project()
            .markers
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(names, ["Locator 2", "Locator 1"]);
        for _ in 0..3 {
            doc.move_marker(b, Ticks::ZERO, Some("drag"));
        }
        assert_eq!(doc.project().markers[0].id, b);
        doc.rename_marker(a, "  Chorus ");
        doc.rename_marker(a, " ");
        assert_eq!(doc.project().markers[1].name, "Chorus");
        doc.delete_marker(b);
        assert_eq!(doc.project().markers.len(), 1);
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
        doc.undo();
        doc.undo();
        assert_eq!(doc.project().markers[1].name, "Locator 2");
        doc.undo();
        assert_eq!(
            doc.project().markers[0].id,
            a,
            "the drag undoes as one step"
        );
        let reopened = Document::new(doc.project().clone()).fresh_id();
        assert!(reopened.0 > a.0.max(b.0));
    }

    #[test]
    fn an_import_is_one_valid_undo_step() {
        let mut doc = Document::new(crate::empty_project());
        assert!(!doc.is_dirty());
        let (a, b) = (doc.fresh_id(), doc.fresh_id());
        doc.import(vec![
            file(a, "Kick", Ticks::ZERO),
            file(b, "Snare", Ticks::quarters(4)),
        ]);
        let project = doc.project();
        assert_eq!(project.tracks.len(), 2);
        assert_eq!(project.media.len(), 2);
        assert_eq!(project.tracks[0].clips[0].length, Ticks::quarters(4));
        assert_eq!(project.tracks[1].clips[0].start, Ticks::quarters(4));
        assert_eq!(lumen_project::validate(project), Ok(()));
        assert!(doc.is_dirty());
        assert_eq!(doc.history().undo_label(), Some("Import 2 files"));
        doc.undo();
        assert!(doc.project().tracks.is_empty());
        assert!(!doc.is_dirty(), "undoing to the saved state is clean again");
        assert!(doc.title().starts_with("Untitled — "));
        doc.redo();
        assert!(doc.title().starts_with("Untitled •"));
        doc.mark_saved_at(doc.state());
        assert!(!doc.is_dirty());
        doc.undo();
        assert!(doc.is_dirty(), "undoing past a save is a change");
    }

    #[test]
    fn track_operations_are_single_undo_steps() {
        let mut doc = Document::new(crate::empty_project());
        let a = doc.add_track(TrackKind::Audio, None);
        let m = doc.add_track(TrackKind::Midi, None);
        let a2 = doc.add_track(TrackKind::Audio, Some(a));
        let names: Vec<&str> = doc
            .project()
            .tracks
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, ["Audio 1", "Audio 2", "MIDI 1"]);
        assert_eq!(doc.project().tracks[1].id, a2);

        doc.rename_track(m, "  Keys  ");
        assert_eq!(doc.project().tracks[2].name, "Keys");
        doc.rename_track(m, "   ");
        assert_eq!(
            doc.history().undo_label(),
            Some("Rename track"),
            "blank names change nothing"
        );

        doc.move_tracks(
            &[m],
            Place {
                index: 0,
                group: None,
            },
        )
        .unwrap();
        assert_eq!(doc.project().tracks[0].id, m);
        let copies = doc.duplicate_tracks(&[m]);
        assert_eq!(doc.project().tracks[1].id, copies[0]);
        assert_eq!(doc.project().tracks[1].name, "Keys");

        doc.recolor_tracks(&[m], Color(0x123456));
        doc.set_armed(&[m, a], true);
        assert_eq!(doc.history().undo_label(), Some("Arm for recording"));
        assert!(doc.project().tracks[0].armed && doc.project().tracks[0].color == Color(0x123456));
        doc.undo();
        assert!(!doc.project().tracks[0].armed, "arming undoes in one step");

        doc.delete_tracks(&[a, a2]);
        assert_eq!(doc.project().tracks.len(), 2);
        assert_eq!(doc.history().undo_label(), Some("Delete 2 tracks"));
        doc.undo();
        assert_eq!(doc.project().tracks.len(), 4);
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn clear_solos_is_one_step() {
        let mut doc = Document::new(crate::empty_project());
        let ids: Vec<Id> = (0..3)
            .map(|_| doc.add_track(TrackKind::Audio, None))
            .collect();
        doc.set_solo(&ids[..2], true, false);
        doc.clear_solos();
        assert!(doc.project().tracks.iter().all(|t| !t.solo));
        assert_eq!(doc.history().undo_label(), Some("Clear solos"));
        doc.undo();
        let soloed: Vec<bool> = doc.project().tracks.iter().map(|t| t.solo).collect();
        assert_eq!(soloed, [true, true, false]);
    }

    #[test]
    fn solo_and_mute_decide_what_is_heard() {
        let mut doc = Document::new(crate::empty_project());
        let ids: Vec<Id> = (0..3)
            .map(|_| doc.add_track(TrackKind::Audio, None))
            .collect();
        let heard = |doc: &Document| -> Vec<bool> {
            let audible = audible(doc.project());
            doc.project().tracks.iter().map(|t| audible(t)).collect()
        };
        assert_eq!(heard(&doc), [true, true, true]);
        doc.set_mute(&ids[..1], true);
        assert_eq!(heard(&doc), [false, true, true]);
        doc.set_solo(&ids[1..2], true, true);
        assert_eq!(heard(&doc), [false, true, false]);
        doc.set_solo(&ids[2..], true, false);
        assert_eq!(heard(&doc), [false, true, true]);
        doc.set_solo(&ids[..1], true, true);
        assert_eq!(
            heard(&doc),
            [false, false, false],
            "a muted solo stays muted"
        );
        doc.undo();
        doc.undo();
        doc.undo();
        assert_eq!(heard(&doc), [false, true, true]);
    }

    #[test]
    fn a_volume_or_pan_drag_is_one_step_per_track_and_stays_in_range() {
        let mut doc = Document::new(crate::empty_project());
        let a = doc.add_track(TrackKind::Audio, None);
        let b = doc.add_track(TrackKind::Audio, None);
        for db in [-1.0, -2.0, -3.0, -99.0] {
            doc.set_volume(a, db);
        }
        assert_eq!(doc.project().tracks[0].gain_db, -60.0, "clamped");
        doc.seal();
        doc.set_pan(b, 3.0);
        assert_eq!(doc.project().tracks[1].pan, 1.0, "clamped");
        assert_eq!(doc.history().undo_label(), Some("Change pan"));
        doc.undo();
        assert_eq!(doc.history().undo_label(), Some("Change volume"));
        doc.undo();
        assert_eq!(
            doc.project().tracks[0].gain_db,
            0.0,
            "the whole drag undoes at once"
        );
    }

    #[test]
    fn deleting_a_group_takes_its_tracks() {
        let mut doc = Document::new(crate::empty_project());
        let group = doc.add_track(TrackKind::Group, None);
        let other = doc.add_track(TrackKind::Audio, None);
        let child = doc.add_track(TrackKind::Audio, Some(group));
        doc.apply("Group", None, |p| {
            Arc::make_mut(&mut p.tracks[1]).parent = Some(group);
        });
        assert_eq!(doc.project().tracks[1].id, child);
        doc.delete_tracks(&[group]);
        assert_eq!(doc.project().tracks.len(), 1);
        assert_eq!(doc.project().tracks[0].id, other);
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn a_track_added_after_one_in_a_group_joins_it() {
        let mut doc = Document::new(crate::empty_project());
        let kick = doc.add_track(TrackKind::Audio, None);
        let bass = doc.add_track(TrackKind::Audio, None);
        let drums = doc.group_tracks(&[kick]).unwrap();
        let snare = doc.add_track(TrackKind::Audio, Some(kick));
        let keys = doc.add_track(TrackKind::Midi, Some(drums));
        let ret = doc.add_track(TrackKind::Return, Some(snare));
        let order: Vec<Id> = doc.project().tracks.iter().map(|t| t.id).collect();
        assert_eq!(order, [drums, kick, snare, ret, keys, bass]);
        let parent = |id| {
            doc.project()
                .tracks
                .iter()
                .find(|t| t.id == id)
                .unwrap()
                .parent
        };
        assert_eq!(parent(snare), Some(drums));
        assert_eq!(parent(keys), None);
        assert_eq!(parent(ret), None, "returns stay at the top");
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
    }

    #[test]
    fn arming_only_leaves_the_other_tracks_disarmed_in_one_undo_step() {
        let mut doc = Document::new(crate::empty_project());
        let a = doc.add_track(TrackKind::Midi, None);
        let b = doc.add_track(TrackKind::Midi, None);
        let c = doc.add_track(TrackKind::Midi, None);
        doc.set_armed(&[a, b], true);
        let armed = |doc: &Document| -> Vec<Id> {
            doc.project()
                .tracks
                .iter()
                .filter(|t| t.armed)
                .map(|t| t.id)
                .collect()
        };
        assert_eq!(armed(&doc), [a, b]);
        doc.arm_only(&[c]);
        assert_eq!(armed(&doc), [c]);
        doc.undo();
        assert_eq!(armed(&doc), [a, b], "one step gives both back");
    }

    #[test]
    fn ids_are_never_reused() {
        let mut doc = Document::new(lumen_project::testing::small_project(2));
        let highest = highest_id(doc.project());
        let id = doc.fresh_id();
        assert!(id.0 > highest);
        doc.import(vec![file(id, "Vox", Ticks::ZERO)]);
        doc.undo();
        assert!(doc.fresh_id().0 > id.0);
    }
}
