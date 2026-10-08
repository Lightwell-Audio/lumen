//! The open project (M3 slice 3): the model with its undo history, where it
//! is saved, and whether it has changed since.
//!
//! Every edit goes through [`Document::apply`], so it is one undo step
//! (ADR-0024). The loop and punch are edits too (slice 5c, as in Ableton);
//! view state is not: it is stamped into the project when it is saved.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_project::model::{
    Clip, ClipContent, Color, Id, Marker, Media, Project, Track, TrackKind,
};
use lumen_project::{History, edit};

/// Track colours, cycled for new tracks (`theme::CLIP_COLORS`, as `#RRGGBB`).
fn track_color(index: usize) -> Color {
    let c = crate::theme::CLIP_COLORS[index % crate::theme::CLIP_COLORS.len()];
    Color(u32::from(c.r()) << 16 | u32::from(c.g()) << 8 | u32::from(c.b()))
}

/// An audio file that finished loading, ready to become a track.
#[derive(Debug, Clone)]
pub struct Imported {
    pub media: Id,
    pub path: PathBuf,
    pub name: String,
    /// The file's own format.
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub size_bytes: u64,
    /// Where the clip starts.
    pub at: Ticks,
}

pub struct Document {
    history: History,
    /// The `.lumen` file, once saved or opened.
    pub path: Option<PathBuf>,
    /// The history state that matches the file on disk.
    saved: u64,
    next_id: u64,
    /// Where each media file actually is on this machine.
    pub media_files: HashMap<Id, PathBuf>,
}

impl Document {
    pub fn new(project: Project) -> Self {
        let next_id = highest_id(&project) + 1;
        let history = History::new(project);
        Self {
            saved: history.state(),
            history,
            path: None,
            next_id,
            media_files: HashMap::new(),
        }
    }

    pub fn project(&self) -> &Project {
        self.history.project()
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Identifies the current project state (changes with every edit, undo
    /// and redo).
    pub fn state(&self) -> u64 {
        self.history.state()
    }

    /// Unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.history.state() != self.saved
    }

    /// The project's name for the title bar: its file name, or "Untitled".
    pub fn name(&self) -> String {
        self.path
            .as_deref()
            .and_then(|p| p.file_stem())
            .map_or_else(
                || "Untitled".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            )
    }

    /// The window title: name, a dot while there are unsaved changes, and
    /// the product.
    pub fn title(&self) -> String {
        let dot = if self.is_dirty() { " •" } else { "" };
        format!(
            "{}{dot} — {}",
            self.name(),
            lumen_core::product::display_name()
        )
    }

    /// Records that `state` is what is on disk now (the state when a save
    /// started: edits made during the save stay unsaved).
    pub fn mark_saved_at(&mut self, state: u64) {
        self.saved = state;
    }

    /// A document recovered from an autosave: it belongs to `path` (if the
    /// project had been saved) and has unsaved changes.
    pub fn recovered(
        project: Project,
        path: Option<PathBuf>,
        media_files: HashMap<Id, PathBuf>,
    ) -> Self {
        let mut doc = Self::new(project);
        doc.path = path;
        doc.media_files = media_files;
        // No history state is ever this: the recovered changes are unsaved.
        doc.saved = u64::MAX;
        doc
    }

    /// A document for a project read from `path`.
    pub fn opened(project: Project, path: PathBuf, media_files: HashMap<Id, PathBuf>) -> Self {
        let mut doc = Self::new(project);
        doc.path = Some(path);
        doc.media_files = media_files;
        doc
    }

    /// A new id, unique in this project for its lifetime (never reused,
    /// even after undo).
    pub fn fresh_id(&mut self) -> Id {
        let id = Id(self.next_id);
        self.next_id += 1;
        id
    }

    /// One undo step; see [`History::apply`].
    pub fn apply(
        &mut self,
        label: &str,
        merge: Option<&str>,
        edit: impl FnOnce(&mut Project),
    ) -> bool {
        self.history.apply(label, merge, edit)
    }

    /// Ends the gesture in progress (call when no pointer button is down).
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

/// Track operations (M3 slice 4a), called from the track headers.
impl Document {
    /// Adds an empty track of `kind` after `after` (or at the end), named
    /// "Audio 3", "MIDI 1" and so on. Returns its id.
    pub fn add_track(&mut self, kind: TrackKind, after: Option<Id>) -> Id {
        let id = self.fresh_id();
        let (word, label) = match kind {
            TrackKind::Audio => ("Audio", "Add audio track"),
            TrackKind::Midi => ("MIDI", "Add MIDI track"),
            TrackKind::Group => ("Group", "Add group track"),
            TrackKind::Return => ("Return", "Add return track"),
            TrackKind::Master => ("Master", "Add master track"),
        };
        self.history.apply(label, None, |project| {
            let number = project.tracks.iter().filter(|t| t.kind == kind).count() + 1;
            let index = after
                .and_then(|a| project.tracks.iter().position(|t| t.id == a))
                .map_or(project.tracks.len(), |i| i + 1);
            let color = track_color(project.tracks.len());
            project.tracks.insert(
                index,
                Arc::new(Track {
                    id,
                    name: format!("{word} {number}"),
                    kind,
                    color,
                    parent: None,
                    gain_db: 0.0,
                    pan: 0.0,
                    mute: false,
                    solo: false,
                    armed: false,
                    devices: vec![],
                    clips: vec![],
                }),
            );
        });
        id
    }

    /// Deletes tracks. Their clips go with them; their media stays in the
    /// project (the media pool, M9). Tracks inside a deleted group move to
    /// the top level.
    pub fn delete_tracks(&mut self, ids: &[Id]) {
        let label = plural(ids.len(), "Delete track", "Delete {} tracks");
        self.history.apply(&label, None, |project| {
            project.tracks.retain(|t| !ids.contains(&t.id));
            for track in &mut project.tracks {
                if track.parent.is_some_and(|p| ids.contains(&p)) {
                    Arc::make_mut(track).parent = None;
                }
            }
        });
    }

    /// Duplicates tracks with their clips (new ids), each placed right
    /// after its original. Returns the new tracks' ids.
    pub fn duplicate_tracks(&mut self, ids: &[Id]) -> Vec<Id> {
        let mut copies = Vec::new();
        for &id in ids {
            let Some(track) = self.project().tracks.iter().find(|t| t.id == id) else {
                continue;
            };
            let mut copy = (**track).clone();
            copy.id = self.fresh_id();
            for clip in &mut copy.clips {
                let mut c = (**clip).clone();
                c.id = self.fresh_id();
                *clip = Arc::new(c);
            }
            copies.push((id, copy));
        }
        let new_ids: Vec<Id> = copies.iter().map(|(_, c)| c.id).collect();
        let label = plural(copies.len(), "Duplicate track", "Duplicate {} tracks");
        self.history.apply(&label, None, |project| {
            for (original, copy) in copies {
                if let Some(i) = project.tracks.iter().position(|t| t.id == original) {
                    project.tracks.insert(i + 1, Arc::new(copy));
                }
            }
        });
        new_ids
    }

    /// Renames a track (empty names are ignored).
    pub fn rename_track(&mut self, id: Id, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.edit_tracks("Rename track", &[id], |t| t.name = name.to_owned());
    }

    pub fn recolor_tracks(&mut self, ids: &[Id], color: Color) {
        self.edit_tracks("Change track colour", ids, |t| t.color = color);
    }

    pub fn set_mute(&mut self, ids: &[Id], mute: bool) {
        let label = if mute { "Mute" } else { "Unmute" };
        self.edit_tracks(label, ids, |t| t.mute = mute);
    }

    /// Solos `ids`. `exclusive` unsolos every other track first (a plain
    /// click on Solo; Ctrl+click adds to the soloed set).
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

    pub fn set_armed(&mut self, ids: &[Id], armed: bool) {
        let label = if armed { "Arm for recording" } else { "Disarm" };
        self.edit_tracks(label, ids, |t| t.armed = armed);
    }

    /// Track volume in dB (−60 to +6). A drag merges into one undo step per
    /// track.
    pub fn set_volume(&mut self, id: Id, db: f32) {
        let db = db.clamp(-60.0, 6.0);
        self.history
            .apply("Change volume", Some(&format!("volume {:x}", id.0)), |p| {
                if let Some(t) = p.tracks.iter_mut().find(|t| t.id == id) {
                    Arc::make_mut(t).gain_db = db;
                }
            });
    }

    /// Track pan, −1 (left) to +1 (right). A drag merges into one undo step
    /// per track.
    pub fn set_pan(&mut self, id: Id, pan: f32) {
        let pan = pan.clamp(-1.0, 1.0);
        self.history
            .apply("Change pan", Some(&format!("pan {:x}", id.0)), |p| {
                if let Some(t) = p.tracks.iter_mut().find(|t| t.id == id) {
                    Arc::make_mut(t).pan = pan;
                }
            });
    }

    /// Moves `ids` (kept in their current order) so the first lands at
    /// `index` among the remaining tracks.
    pub fn move_tracks(&mut self, ids: &[Id], index: usize) {
        let label = plural(ids.len(), "Move track", "Move {} tracks");
        self.history.apply(&label, None, |project| {
            let (moving, mut rest): (Vec<_>, Vec<_>) =
                project.tracks.drain(..).partition(|t| ids.contains(&t.id));
            let at = index.min(rest.len());
            rest.splice(at..at, moving);
            project.tracks = rest;
        });
    }

    /// Sets the loop brace, as one step named `label` (`merge` joins the
    /// moves of one drag into one step). A region that ends before it
    /// starts is ignored.
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

    /// Turns looping on or off.
    pub fn set_loop_enabled(&mut self, enabled: bool) {
        let label = if enabled { "Loop on" } else { "Loop off" };
        self.history.apply(label, None, |project| {
            project.transport.loop_enabled = enabled
        });
    }

    /// Turns punch in and punch out on or off (at the loop's edges).
    pub fn set_punch(&mut self, punch_in: bool, punch_out: bool) {
        self.history.apply("Punch", None, |project| {
            project.transport.punch_in = punch_in;
            project.transport.punch_out = punch_out;
        });
    }

    /// Adds a locator at `at` (M3 slice 5d), named "Locator 1", "Locator 2"
    /// and so on when `name` is `None`. Returns its id.
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

    /// Moves a locator (`merge` joins the moves of one drag into one step).
    pub fn move_marker(&mut self, id: Id, at: Ticks, merge: Option<&str>) {
        let at = at.max(Ticks::ZERO);
        self.history.apply("Move locator", merge, |project| {
            if let Some(m) = project.markers.iter_mut().find(|m| m.id == id) {
                m.at = at;
            }
            sort_markers(&mut project.markers);
        });
    }

    /// Renames a locator (empty names are ignored).
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

    /// Puts clips at new places (M3 slice 6): each [`ClipMove`] moves that
    /// clip to its start on its track. Locked clips, and moves to tracks
    /// that do not exist, are left out. `merge` joins the moves of one drag
    /// into one step.
    pub fn move_clips(&mut self, moves: &[ClipMove], merge: Option<&str>) {
        let label = plural(moves.len(), "Move clip", "Move {} clips");
        self.history
            .apply(&label, merge, |project| place_clips(project, moves));
    }

    /// Puts copies of clips at new places, as Ctrl+drag does: the copy of
    /// `moves[i].clip` gets the id `ids[i]`. A drag calls this every frame
    /// with the same `ids` and `merge`: the copies made last time are
    /// replaced, so the gesture stays one step.
    pub fn copy_clips(&mut self, moves: &[ClipMove], ids: &[Id], merge: Option<&str>) {
        let label = plural(moves.len(), "Copy clip", "Copy {} clips");
        self.history.apply(&label, merge, |project| {
            for track in &mut project.tracks {
                if track.clips.iter().any(|c| ids.contains(&c.id)) {
                    Arc::make_mut(track).clips.retain(|c| !ids.contains(&c.id));
                }
            }
            let copies: Vec<(Clip, ClipMove)> = moves
                .iter()
                .zip(ids)
                .filter_map(|(m, &id)| {
                    let clip = project
                        .tracks
                        .iter()
                        .flat_map(|t| &t.clips)
                        .find(|c| c.id == m.clip)?;
                    let mut copy = (**clip).clone();
                    copy.id = id;
                    copy.locked = false;
                    copy.start = m.start.max(Ticks::ZERO);
                    Some((copy, *m))
                })
                .collect();
            for (copy, m) in copies {
                if let Some(t) = project.tracks.iter_mut().find(|t| t.id == m.track) {
                    let t = Arc::make_mut(t);
                    t.clips.push(Arc::new(copy));
                    t.clips.sort_by_key(|c| c.start);
                }
            }
        });
    }

    /// Deletes clips; locked ones stay. Their media stays in the project
    /// (the media pool, M9).
    pub fn delete_clips(&mut self, ids: &[Id]) {
        let label = plural(ids.len(), "Delete clip", "Delete {} clips");
        self.history.apply(&label, None, |project| {
            for track in &mut project.tracks {
                if track.clips.iter().any(|c| ids.contains(&c.id) && !c.locked) {
                    Arc::make_mut(track)
                        .clips
                        .retain(|c| c.locked || !ids.contains(&c.id));
                }
            }
        });
    }

    /// Empties `[a, b)` on `tracks` (a time range's Delete, or its Cut):
    /// clips are cut back at its edges or split around it.
    pub fn clear_range(&mut self, label: &str, tracks: &[Id], a: Ticks, b: Ticks) {
        let next = &mut self.next_id;
        self.history.apply(label, None, |project| {
            edit::clear(project, tracks, a, b, &mut fresh_ids(next));
        });
    }

    /// Deletes `[a, b)` from the whole song (Ableton's Delete Time).
    pub fn delete_time(&mut self, a: Ticks, b: Ticks) {
        let next = &mut self.next_id;
        self.history.apply("Delete time", None, |project| {
            edit::delete_time(project, a, b, &mut fresh_ids(next));
        });
    }

    /// Inserts `length` of silence at `at` across the whole song.
    pub fn insert_time(&mut self, at: Ticks, length: Ticks) {
        let next = &mut self.next_id;
        self.history.apply("Insert time", None, |project| {
            edit::insert_time(project, at, length, &mut fresh_ids(next));
        });
    }

    /// Pastes `clipboard` at `at` from the track with index `track`,
    /// overwriting what is there, as one step named `label` ("Paste",
    /// "Duplicate"). Returns the new clips' ids.
    pub fn paste(
        &mut self,
        label: &str,
        clipboard: &edit::Clipboard,
        at: Ticks,
        track: usize,
    ) -> Vec<Id> {
        let next = &mut self.next_id;
        let mut pasted = Vec::new();
        self.history.apply(label, None, |project| {
            pasted = edit::paste(project, clipboard, at, track, &mut fresh_ids(next));
        });
        pasted
    }

    /// Deletes clips as a Cut (the clipboard holds them).
    pub fn cut_clips(&mut self, ids: &[Id]) {
        let label = plural(ids.len(), "Cut clip", "Cut {} clips");
        self.history.apply(&label, None, |project| {
            for track in &mut project.tracks {
                if track.clips.iter().any(|c| ids.contains(&c.id) && !c.locked) {
                    Arc::make_mut(track)
                        .clips
                        .retain(|c| c.locked || !ids.contains(&c.id));
                }
            }
        });
    }

    /// Changes the given tracks, as one step named `label`.
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

    /// Adds loaded files as new audio tracks, each with one clip, as one
    /// undo step.
    pub fn import(&mut self, files: Vec<Imported>) {
        if files.is_empty() {
            return;
        }
        let label = match files.as_slice() {
            [one] => format!("Import {}", one.name),
            many => format!("Import {} files", many.len()),
        };
        let mut additions = Vec::new();
        for file in files {
            let (track, clip) = (self.fresh_id(), self.fresh_id());
            self.media_files.insert(file.media, file.path.clone());
            additions.push((track, clip, file));
        }
        self.history.apply(&label, None, |project| {
            for (track, clip, file) in additions {
                let start_seconds = seconds_at(project, file.at);
                let seconds = file.frames as f64 / f64::from(file.sample_rate.max(1));
                let end = ticks_at_seconds(project, start_seconds + seconds);
                project.media.push(Media {
                    id: file.media,
                    path: file.path.to_string_lossy().into_owned(),
                    original_path: None,
                    frames: file.frames,
                    channels: file.channels,
                    sample_rate: file.sample_rate,
                    size_bytes: file.size_bytes,
                });
                let color = track_color(project.tracks.len());
                project.tracks.push(Arc::new(Track {
                    id: track,
                    name: file.name.clone(),
                    kind: TrackKind::Audio,
                    color,
                    parent: None,
                    gain_db: 0.0,
                    pan: 0.0,
                    mute: false,
                    solo: false,
                    armed: false,
                    devices: vec![],
                    clips: vec![Arc::new(Clip {
                        id: clip,
                        name: file.name,
                        start: file.at,
                        length: (end - file.at).max(Ticks(1)),
                        muted: false,
                        color: None,
                        locked: false,
                        fade_in: Default::default(),
                        fade_out: Default::default(),
                        loop_length: None,
                        content: ClipContent::Audio {
                            media: file.media,
                            offset: 0,
                            gain_db: 0.0,
                        },
                    })],
                }));
            }
        });
    }
}

/// "Delete track" or "Delete 3 tracks".
/// Where a clip goes: a track and a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipMove {
    pub clip: Id,
    pub track: Id,
    pub start: Ticks,
}

/// Moves each clip in `moves` to its track and start, keeping every
/// track's clips in time order. Locked clips, and moves to tracks that do
/// not exist, are left out, so no clip is ever lost.
fn place_clips(project: &mut Project, moves: &[ClipMove]) {
    let tracks: Vec<Id> = project.tracks.iter().map(|t| t.id).collect();
    let target = |c: &Arc<Clip>| {
        moves
            .iter()
            .find(|m| m.clip == c.id && !c.locked && tracks.contains(&m.track))
            .copied()
    };
    let mut taken = Vec::new();
    for track in &mut project.tracks {
        if track.clips.iter().any(|c| target(c).is_some()) {
            let t = Arc::make_mut(track);
            let (go, stay): (Vec<_>, Vec<_>) = t.clips.drain(..).partition(|c| target(c).is_some());
            t.clips = stay;
            taken.extend(go.into_iter().filter_map(|c| Some((target(&c)?, c))));
        }
    }
    for (m, clip) in taken {
        if let Some(t) = project.tracks.iter_mut().find(|t| t.id == m.track) {
            let mut clip = (*clip).clone();
            clip.start = m.start.max(Ticks::ZERO);
            Arc::make_mut(t).clips.push(Arc::new(clip));
        }
    }
    for track in &mut project.tracks {
        if !track.clips.is_sorted_by_key(|c| c.start) {
            Arc::make_mut(track).clips.sort_by_key(|c| c.start);
        }
    }
}

/// New ids from a document's counter, for edits that make clips.
fn fresh_ids(next: &mut u64) -> impl FnMut() -> Id + '_ {
    move || {
        let id = Id(*next);
        *next += 1;
        id
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_owned()
    } else {
        many.replace("{}", &n.to_string())
    }
}

/// Which tracks are heard: not muted, and, while any track is soloed, only
/// the soloed ones (groups and returns join this with the mixer, M4).
pub fn audible(project: &Project) -> impl Fn(&Track) -> bool + use<> {
    let any_solo = project.tracks.iter().any(|t| t.solo);
    move |t: &Track| !t.mute && (!any_solo || t.solo)
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

/// The highest id used anywhere in `project`.
/// Locators stay in time order (equal times keep their order).
fn sort_markers(markers: &mut [Marker]) {
    markers.sort_by_key(|m| m.at);
}

/// "Locator N", one past the highest number in use.
fn next_marker_name(markers: &[Marker]) -> String {
    let n = markers
        .iter()
        .filter_map(|m| m.name.strip_prefix("Locator ")?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("Locator {}", n + 1)
}

fn highest_id(project: &Project) -> u64 {
    let tracks = project.tracks.iter().flat_map(|t| {
        std::iter::once(t.id.0)
            .chain(t.devices.iter().map(|d| d.id.0))
            .chain(t.clips.iter().map(|c| c.id.0))
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

    fn file(media: Id, name: &str, at: Ticks) -> Imported {
        Imported {
            media,
            path: PathBuf::from(format!("C:/audio/{name}.wav")),
            name: name.into(),
            sample_rate: 48_000,
            channels: 2,
            frames: 96_000,
            size_bytes: 384_044,
            at,
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
        // Ids survive reopening: a new id never repeats a locator's.
        let reopened = Document::new(doc.project().clone()).fresh_id();
        assert!(reopened.0 > a.0.max(b.0));
    }

    #[test]
    fn clips_move_and_copy_across_tracks_as_one_step_and_locked_ones_stay() {
        let mut doc = Document::new(crate::empty_project());
        let (a, b) = (doc.fresh_id(), doc.fresh_id());
        doc.import(vec![
            file(a, "Kick", Ticks::ZERO),
            file(b, "Snare", Ticks::quarters(4)),
        ]);
        let tracks: Vec<Id> = doc.project().tracks.iter().map(|t| t.id).collect();
        let kick = doc.project().tracks[0].clips[0].id;
        // A drag: three frames, one step; the kick ends on the snare's track.
        for q in [1, 2, 8] {
            let to = ClipMove {
                clip: kick,
                track: tracks[1],
                start: Ticks::quarters(q),
            };
            doc.move_clips(&[to], Some("clip drag"));
        }
        let project = doc.project();
        assert!(project.tracks[0].clips.is_empty());
        let starts: Vec<_> = project.tracks[1].clips.iter().map(|c| c.start).collect();
        assert_eq!(
            starts,
            [Ticks::quarters(4), Ticks::quarters(8)],
            "in time order"
        );
        assert_eq!(lumen_project::validate(project), Ok(()));
        doc.undo();
        assert_eq!(
            doc.project().tracks[0].clips[0].start,
            Ticks::ZERO,
            "one step"
        );

        // A copy-drag: twice, still one copy.
        let copy = doc.fresh_id();
        for q in [12, 16] {
            let to = ClipMove {
                clip: kick,
                track: tracks[0],
                start: Ticks::quarters(q),
            };
            doc.copy_clips(&[to], &[copy], Some("clip copy"));
        }
        let kicks: Vec<_> = doc.project().tracks[0]
            .clips
            .iter()
            .map(|c| (c.id, c.start))
            .collect();
        assert_eq!(kicks, [(kick, Ticks::ZERO), (copy, Ticks::quarters(16))]);

        // Locked clips neither move nor go; a move to a missing track is
        // ignored rather than losing the clip.
        doc.apply("lock", None, |p| {
            Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]).locked = true;
        });
        let away = ClipMove {
            clip: kick,
            track: tracks[1],
            start: Ticks::ZERO,
        };
        doc.move_clips(&[away], None);
        doc.delete_clips(&[kick, copy]);
        let left: Vec<_> = doc.project().tracks[0].clips.iter().map(|c| c.id).collect();
        assert_eq!(left, [kick]);
        let lost = ClipMove {
            clip: kick,
            track: Id(999_999),
            start: Ticks::ZERO,
        };
        doc.apply("unlock", None, |p| {
            Arc::make_mut(&mut Arc::make_mut(&mut p.tracks[0]).clips[0]).locked = false;
        });
        doc.move_clips(&[lost], None);
        assert_eq!(doc.project().tracks[0].clips[0].id, kick);
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
        // Two seconds at 120 BPM is four quarter notes.
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

        doc.move_tracks(&[m], 0);
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
        // Ctrl+click adds a solo; a plain click makes it exclusive.
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
    fn deleting_a_group_frees_its_children() {
        let mut doc = Document::new(crate::empty_project());
        let group = doc.add_track(TrackKind::Group, None);
        let child = doc.add_track(TrackKind::Audio, None);
        doc.apply("Group", None, |p| {
            Arc::make_mut(&mut p.tracks[1]).parent = Some(group);
        });
        doc.delete_tracks(&[group]);
        assert_eq!(doc.project().tracks[0].id, child);
        assert_eq!(doc.project().tracks[0].parent, None);
        assert_eq!(lumen_project::validate(doc.project()), Ok(()));
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
