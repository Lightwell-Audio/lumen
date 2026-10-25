//! Freeze (M7b slice E5), the pure parts. A track's instrument, devices, trim
//! and polarity are rendered into a file; while the track is frozen it plays
//! the file and runs no plugin, and its clips, notes and devices stay exactly
//! as they were, so unfreezing gives them back. Flatten keeps the file and
//! drops everything it was made from.
//!
//! What the owner chose (FR1 to FR4, 2026-10-24):
//! - the file holds the sound only; fader, pan, sends, mute, solo and output
//!   stay live on a frozen track;
//! - a frozen track is read-only ([`violations`], which `History` enforces);
//! - Flatten turns the track into a plain audio track in place;
//! - only audio and MIDI tracks freeze, and not those whose sound depends on
//!   other tracks ([`Refusal`]).
//!
//! The rendering is the app's (it runs on a worker with the track's plugins in
//! hosts of its own); this module says which tracks go, over what span, what
//! the project becomes, and what a frozen track sounds like ([`played`]).

use std::borrow::Cow;
use std::sync::Arc;

use lumen_core::time::Ticks;

use crate::automation::Target;
use crate::model::{
    Clip, ClipContent, Device, Frozen, Id, Media, MidiFrom, Project, Track, TrackKind,
};

/// Why a track cannot be frozen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Groups, returns and the master have no clips or notes to render.
    Kind,
    /// A MIDI track sent to hardware has no sound of its own.
    ToHardware,
    AlreadyFrozen,
    /// Nothing on the track to render.
    Empty,
    /// A device is keyed from another track's sound.
    KeyedFromOthers,
    /// The track plays an output of another track's instrument.
    PlaysAnotherOutput,
    /// Other tracks play outputs of this track's instrument.
    FeedsOthers,
}

impl Refusal {
    /// The reason, as a clause to follow the track's name.
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::Kind => "only audio and MIDI tracks can be frozen",
            Refusal::ToHardware => "it is sent to a MIDI output and has no sound of its own",
            Refusal::AlreadyFrozen => "it is already frozen",
            Refusal::Empty => "it has nothing to render",
            Refusal::KeyedFromOthers => {
                "one of its devices is keyed from another track, which a freeze cannot hear"
            }
            Refusal::PlaysAnotherOutput => {
                "it plays an output of another track's instrument, which a freeze cannot hear"
            }
            Refusal::FeedsOthers => {
                "other tracks play outputs of its instrument, which a freeze would take away"
            }
        }
    }
}

/// One track's freeze.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub track: Id,
    /// The track as it was when the part was planned. A part is applied only
    /// if the track is still exactly this.
    pub planned: Arc<Track>,
    /// From the first clip's start to the last clip's end; the file adds its
    /// tail past that.
    pub start: Ticks,
    pub end: Ticks,
    /// The track plays mono material ([`Track::plays_mono`]).
    pub mono: bool,
}

/// What [`plan`] found.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    pub parts: Vec<Part>,
    pub refused: Vec<(Id, Refusal)>,
}

/// Why `track` cannot be frozen, if it cannot.
pub fn refusal(project: &Project, track: &Track) -> Option<Refusal> {
    match track.kind {
        TrackKind::Audio | TrackKind::Midi => {}
        _ => return Some(Refusal::Kind),
    }
    if track.frozen.is_some() {
        return Some(Refusal::AlreadyFrozen);
    }
    if track.kind == TrackKind::Midi && track.midi_to.is_some() {
        return Some(Refusal::ToHardware);
    }
    if track.clips.is_empty() {
        return Some(Refusal::Empty);
    }
    if track.devices.iter().any(|d| d.sidechain.is_some()) {
        return Some(Refusal::KeyedFromOthers);
    }
    if track.plugin_output.is_some() {
        return Some(Refusal::PlaysAnotherOutput);
    }
    let feeds = project.tracks.iter().any(|other| {
        other
            .plugin_output
            .is_some_and(|o| track.devices.iter().any(|d| d.id == o.device))
    });
    if feeds {
        return Some(Refusal::FeedsOthers);
    }
    None
}

/// The parts for `tracks` (in project order) and the tracks refused.
pub fn plan(project: &Project, tracks: &[Id]) -> Plan {
    let mut plan = Plan::default();
    for track in &project.tracks {
        if !tracks.contains(&track.id) {
            continue;
        }
        if let Some(why) = refusal(project, track) {
            plan.refused.push((track.id, why));
            continue;
        }
        let start = track.clips.iter().map(|c| c.start).min();
        let end = track.clips.iter().map(|c| c.start + c.length).max();
        let (Some(start), Some(end)) = (start, end) else {
            continue;
        };
        plan.parts.push(Part {
            track: track.id,
            planned: Arc::clone(track),
            start,
            end,
            mono: track.plays_mono(&project.media),
        });
    }
    plan
}

/// A part and the file rendered for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Baked {
    pub part: Part,
    pub media: Media,
    /// Where the file ends, with its tail: at least `part.end`.
    pub end: Ticks,
    /// The track's devices with the states the plugins held when the file was
    /// rendered (empty: as the project has them). A frozen track starts no
    /// plugin, so what they hold now would be lost; unfreezing starts them from
    /// these.
    pub devices: Vec<Device>,
}

/// Whether the track of `part` is still exactly as it was planned.
pub fn is_current(project: &Project, part: &Part) -> bool {
    project
        .tracks
        .iter()
        .find(|t| t.id == part.track)
        .is_some_and(|t| Arc::ptr_eq(t, &part.planned) || **t == *part.planned)
}

/// Freezes each track with its file and registers the files as media. Parts
/// whose track changed are skipped. Returns the tracks frozen.
pub fn apply(project: &mut Project, baked: &[Baked], fresh: &mut impl FnMut() -> Id) -> Vec<Id> {
    let mut frozen = Vec::new();
    for Baked {
        part,
        media,
        end,
        devices,
    } in baked
    {
        if !is_current(project, part) {
            continue;
        }
        if !project.media.iter().any(|m| m.id == media.id) {
            project.media.push(media.clone());
        }
        let Some(track) = project.tracks.iter_mut().find(|t| t.id == part.track) else {
            continue;
        };
        let track = Arc::make_mut(track);
        if devices
            .iter()
            .map(|d| d.id)
            .eq(track.devices.iter().map(|d| d.id))
        {
            track.devices.clone_from(devices);
        }
        track.armed = false;
        track.frozen = Some(Frozen {
            id: fresh(),
            media: media.id,
            start: part.start,
            length: (*end - part.start).max(part.end - part.start).max(Ticks(1)),
            mono: part.mono,
        });
        frozen.push(part.track);
    }
    frozen
}

/// Unfreezes `tracks`: the clips, notes and devices play again. The file stays
/// in the project's media. Returns the tracks that were frozen.
pub fn unfreeze(project: &mut Project, tracks: &[Id]) -> Vec<Id> {
    let mut thawed = Vec::new();
    for track in &mut project.tracks {
        if tracks.contains(&track.id) && track.frozen.is_some() {
            Arc::make_mut(track).frozen = None;
            thawed.push(track.id);
        }
    }
    thawed
}

/// Flattens `tracks` (owner's FR3): each frozen track becomes a plain audio
/// track holding the file as one clip. Its name, colour, fader, pan, sends and
/// output stay; its clips, notes, instrument, devices and the lanes for them
/// go. Returns the tracks flattened.
pub fn flatten(project: &mut Project, tracks: &[Id]) -> Vec<Id> {
    let mut flat = Vec::new();
    for track in &mut project.tracks {
        if !tracks.contains(&track.id) {
            continue;
        }
        let Some(frozen) = track.frozen.clone() else {
            continue;
        };
        let held = Arc::make_mut(track);
        let name = held.name.clone();
        let clip = frozen_clip(&frozen, &name);
        held.kind = TrackKind::Audio;
        held.clips = vec![Arc::new(clip)];
        held.devices.clear();
        held.trim_db = 0.0;
        held.invert = false;
        held.midi_to = None;
        held.midi_from = MidiFrom::default();
        held.plugin_output = None;
        held.armed = false;
        held.automation.retain(|l| mixing(l.target));
        held.frozen = None;
        flat.push(held.id);
    }
    flat
}

/// The values that stay live on a frozen track: the mixer's.
fn mixing(target: Target) -> bool {
    matches!(
        target,
        Target::Volume | Target::Pan | Target::Mute | Target::Send { .. }
    )
}

/// The clip that plays a frozen track's file.
fn frozen_clip(frozen: &Frozen, name: &str) -> Clip {
    Clip {
        id: frozen.id,
        name: name.to_owned(),
        start: frozen.start,
        length: frozen.length,
        muted: false,
        color: None,
        locked: false,
        fade_in: Default::default(),
        crossfade: false,
        fade_out: Default::default(),
        loop_length: None,
        loop_phase: Ticks::ZERO,
        audio_phase_seconds: 0.0,
        reversed: false,
        inverted: false,
        envelopes: Default::default(),
        content: ClipContent::Audio {
            media: frozen.media,
            offset: 0,
            gain_db: 0.0,
        },
    }
}

/// The project as it sounds: each frozen track is an audio track that plays
/// its file, with no instrument, devices, trim or polarity, and no lane for
/// them. The live engine, its routing, its MIDI and the plugin hosts are built
/// from this, so a frozen track starts no plugin by construction. The track
/// keeps its `frozen` marker, which [`Track::plays_mono`] reads.
pub fn played(project: &Project) -> Cow<'_, Project> {
    if project.tracks.iter().all(|t| t.frozen.is_none()) {
        return Cow::Borrowed(project);
    }
    let mut copy = project.clone();
    for track in &mut copy.tracks {
        let Some(frozen) = track.frozen.clone() else {
            continue;
        };
        let held = Arc::make_mut(track);
        held.kind = TrackKind::Audio;
        held.clips = vec![Arc::new(frozen_clip(&frozen, &held.name))];
        held.devices.clear();
        held.trim_db = 0.0;
        held.invert = false;
        held.midi_to = None;
        held.plugin_output = None;
        held.armed = false;
        held.automation.retain(|l| mixing(l.target));
    }
    Cow::Owned(copy)
}

/// What an edit may not do: change a frozen track, or the clips of a track
/// being recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Violation {
    /// It changed this frozen track's clips, devices, trim, polarity or the
    /// lanes for them, or armed it.
    Track(Id),
    /// It changed the clips of this track, or removed it, while a recording
    /// is writing a take into it: the next commit would put them back as they
    /// were when recording began.
    Recording(Id),
    /// It changed the tempo or the time signatures, which would leave a frozen
    /// track's file behind the music.
    Timing,
}

/// What `new` does to the frozen tracks of `old` that an edit may not do
/// (owner's FR2). Empty for an edit that leaves them alone, and for freezing,
/// unfreezing and flattening, which end or start the freeze themselves.
pub fn violations(old: &Project, new: &Project) -> Vec<Violation> {
    let mut found = Vec::new();
    let mut any = false;
    for before in old.tracks.iter().filter(|t| t.frozen.is_some()) {
        any = true;
        let Some(after) = new.tracks.iter().find(|t| t.id == before.id) else {
            continue;
        };
        if after.frozen.is_some() && !Arc::ptr_eq(before, after) && changed(before, after) {
            found.push(Violation::Track(before.id));
        }
    }
    if any && (old.tempo != new.tempo || old.meter != new.meter) {
        found.push(Violation::Timing);
    }
    found
}

/// Whether an edit touched what a freeze fixes.
fn changed(before: &Track, after: &Track) -> bool {
    let lanes = |track: &Track| -> Vec<_> {
        track
            .automation
            .iter()
            .filter(|l| !mixing(l.target))
            .cloned()
            .collect()
    };
    before.kind != after.kind
        || before.trim_db.to_bits() != after.trim_db.to_bits()
        || before.invert != after.invert
        || before.midi_to != after.midi_to
        || before.plugin_output != after.plugin_output
        || (!before.armed && after.armed)
        || before.devices != after.devices
        || before.clips.len() != after.clips.len()
        || !before
            .clips
            .iter()
            .zip(&after.clips)
            .all(|(a, b)| Arc::ptr_eq(a, b) || a == b)
        || lanes(before) != lanes(after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Color, Media};
    use crate::testing::project_of;

    fn clip(id: u64, start: i64, length: i64) -> Arc<Clip> {
        Arc::new(Clip {
            start: Ticks::quarters(start),
            length: Ticks::quarters(length),
            ..frozen_clip(
                &Frozen {
                    id: Id(id),
                    media: Id(900),
                    start: Ticks::ZERO,
                    length: Ticks::ZERO,
                    mono: false,
                },
                "Part",
            )
        })
    }

    fn track(id: u64, kind: TrackKind, clips: Vec<Arc<Clip>>) -> Track {
        let mut t = Track::new(Id(id), format!("T{id}"), kind, Color(0));
        t.clips = clips;
        t
    }

    fn media(id: u64, channels: u16) -> Media {
        Media {
            id: Id(id),
            path: "x.wav".into(),
            original_path: None,
            frames: 48_000,
            channels,
            sample_rate: 48_000,
            size_bytes: 0,
        }
    }

    fn project() -> Project {
        let mut p = project_of(
            "freeze",
            vec![
                track(1, TrackKind::Audio, vec![clip(10, 0, 4), clip(11, 8, 4)]),
                track(2, TrackKind::Midi, vec![clip(20, 4, 4)]),
                track(3, TrackKind::Group, Vec::new()),
                track(4, TrackKind::Audio, Vec::new()),
            ],
        );
        p.media = vec![media(900, 1)];
        p
    }

    fn baked(project: &Project, id: u64) -> Baked {
        let plan = plan(project, &[Id(id)]);
        let part = plan.parts[0].clone();
        let end = part.end + Ticks::quarters(1);
        Baked {
            part,
            media: media(950, 2),
            end,
            devices: Vec::new(),
        }
    }

    #[test]
    fn the_plan_spans_the_track_and_says_why_others_are_refused() {
        let p = project();
        let plan = plan(&p, &[Id(1), Id(2), Id(3), Id(4)]);
        assert_eq!(plan.parts.len(), 2);
        assert_eq!(plan.parts[0].start, Ticks::ZERO);
        assert_eq!(plan.parts[0].end, Ticks::quarters(12));
        assert!(plan.parts[0].mono, "its only media is mono");
        assert!(!plan.parts[1].mono);
        assert_eq!(
            plan.refused,
            vec![(Id(3), Refusal::Kind), (Id(4), Refusal::Empty)]
        );
    }

    #[test]
    fn a_track_that_hears_other_tracks_is_refused() {
        use crate::model::{Device, PluginOutput};
        let mut p = project();
        let make = |p: &mut Project, f: &dyn Fn(&mut Track)| {
            f(Arc::make_mut(&mut p.tracks[0]));
        };
        make(&mut p, &|t| {
            t.plugin_output = Some(PluginOutput {
                device: Id(5),
                bus: 1,
            })
        });
        assert_eq!(
            refusal(&p, &p.tracks[0].clone()),
            Some(Refusal::PlaysAnotherOutput)
        );
        make(&mut p, &|t| t.plugin_output = None);
        // Another track plays an output of track 2's instrument.
        let mut synth = Device::synth(Id(7), "Init");
        synth.id = Id(7);
        Arc::make_mut(&mut p.tracks[1]).devices.push(synth);
        Arc::make_mut(&mut p.tracks[0]).plugin_output = Some(PluginOutput {
            device: Id(7),
            bus: 1,
        });
        assert_eq!(
            refusal(&p, &p.tracks[1].clone()),
            Some(Refusal::FeedsOthers)
        );
    }

    #[test]
    fn freezing_plays_one_file_and_unfreezing_gives_the_track_back() {
        let mut p = project();
        let before = p.clone();
        let baked = baked(&p, 1);
        let mut next = 5_000;
        let done = apply(&mut p, &[baked], &mut || {
            next += 1;
            Id(next)
        });
        assert_eq!(done, vec![Id(1)]);
        let track = &p.tracks[0];
        let frozen = track.frozen.as_ref().expect("frozen");
        assert_eq!(frozen.length, Ticks::quarters(13), "with the tail");
        assert!(frozen.mono);
        assert!(p.media.iter().any(|m| m.id == Id(950)));
        // The track still holds its clips; it sounds as one file.
        assert_eq!(track.clips.len(), 2);
        let sounds = played(&p);
        let heard = &sounds.tracks[0];
        assert_eq!(heard.clips.len(), 1);
        assert_eq!(heard.clips[0].id, frozen.id);
        assert!(matches!(
            heard.clips[0].content,
            ClipContent::Audio { media: Id(950), .. }
        ));
        assert!(heard.plays_mono(&sounds.media));
        assert_eq!(unfreeze(&mut p, &[Id(1)]), vec![Id(1)]);
        let mut expected = before;
        expected.media.push(media(950, 2));
        assert_eq!(p, expected, "only the file stays");
    }

    #[test]
    fn nothing_is_borrowed_from_a_project_with_no_frozen_track() {
        let p = project();
        assert!(matches!(played(&p), Cow::Borrowed(_)));
    }

    #[test]
    fn a_frozen_midi_track_sounds_as_audio_without_its_instrument() {
        let mut p = project();
        Arc::make_mut(&mut p.tracks[1]).armed = true;
        Arc::make_mut(&mut p.tracks[1]).trim_db = 3.0;
        let baked = baked(&p, 2);
        apply(&mut p, &[baked], &mut || Id(6_000));
        assert!(!p.tracks[1].armed, "freezing disarms");
        let sounds = played(&p);
        let heard = &sounds.tracks[1];
        assert_eq!(heard.kind, TrackKind::Audio);
        assert_eq!(heard.trim_db, 0.0);
        assert!(heard.devices.is_empty());
    }

    #[test]
    fn a_part_for_a_track_that_changed_is_not_applied() {
        let mut p = project();
        let baked = baked(&p, 1);
        Arc::make_mut(&mut p.tracks[0]).name = "Renamed".into();
        assert!(apply(&mut p, &[baked], &mut || Id(1)).is_empty());
        assert!(p.tracks[0].frozen.is_none());
    }

    #[test]
    fn flatten_leaves_a_plain_audio_track_with_the_file() {
        let mut p = project();
        let baked = baked(&p, 2);
        apply(&mut p, &[baked], &mut || Id(6_000));
        Arc::make_mut(&mut p.tracks[1]).gain_db = -4.0;
        assert_eq!(flatten(&mut p, &[Id(2)]), vec![Id(2)]);
        let track = &p.tracks[1];
        assert_eq!(track.kind, TrackKind::Audio);
        assert!(track.frozen.is_none());
        assert_eq!(track.gain_db, -4.0, "the mix stays");
        assert_eq!(track.name, "T2");
        assert_eq!(track.clips.len(), 1);
        assert_eq!(track.clips[0].id, Id(6_000));
        assert!(violations(&p, &p).is_empty());
    }

    #[test]
    fn a_frozen_track_may_be_mixed_but_not_edited() {
        let mut p = project();
        let baked = baked(&p, 1);
        apply(&mut p, &[baked], &mut || Id(6_000));
        let edit = |p: &Project, f: &dyn Fn(&mut Track)| {
            let mut next = p.clone();
            f(Arc::make_mut(&mut next.tracks[0]));
            violations(p, &next)
        };
        assert!(edit(&p, &|t| t.gain_db = -3.0).is_empty());
        assert!(edit(&p, &|t| t.pan = 0.5).is_empty());
        assert!(edit(&p, &|t| t.mute = true).is_empty());
        assert!(edit(&p, &|t| t.name = "x".into()).is_empty());
        let refused = vec![Violation::Track(Id(1))];
        assert_eq!(edit(&p, &|t| t.clips.clear()), refused);
        assert_eq!(edit(&p, &|t| t.trim_db = 2.0), refused);
        assert_eq!(edit(&p, &|t| t.invert = true), refused);
        assert_eq!(edit(&p, &|t| t.armed = true), refused);
        assert_eq!(
            edit(&p, &|t| Arc::make_mut(&mut t.clips[0]).muted = true),
            refused
        );
    }

    #[test]
    fn the_tempo_does_not_move_under_a_frozen_track() {
        let mut p = project();
        let baked = baked(&p, 1);
        apply(&mut p, &[baked], &mut || Id(6_000));
        let mut next = p.clone();
        next.tempo[0].bpm += 10.0;
        assert_eq!(violations(&p, &next), vec![Violation::Timing]);
        // With nothing frozen, the tempo is free.
        let free = project();
        let mut faster = free.clone();
        faster.tempo[0].bpm += 10.0;
        assert!(violations(&free, &faster).is_empty());
    }

    #[test]
    fn deleting_a_frozen_track_is_not_a_violation() {
        let mut p = project();
        let baked = baked(&p, 1);
        apply(&mut p, &[baked], &mut || Id(6_000));
        let mut next = p.clone();
        next.tracks.remove(0);
        assert!(violations(&p, &next).is_empty());
    }
}
