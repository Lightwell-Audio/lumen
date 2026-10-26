//! What Export MIDI writes (M5 slice 7, ADR-0062): what plays, as
//! `midi_runs` gives it to the engine. Only notes in a clip's window sound,
//! muted notes do not, a note held across a seam sounds through it, and a
//! looping clip's repeats are baked into real notes, each cut where its
//! repeat ends, as the engine cuts them.

use std::sync::Arc;

use lumen_core::meter::MeterPoint;
use lumen_core::time::{TempoMap, TempoPoint, Ticks};
use lumen_project::edit::in_window;
use lumen_project::model::{Clip, ClipContent, ClipEvent, Id, Note, Project, TrackKind};
use lumen_project::smf::{FileTrack, MidiFile};

use crate::midi_runs::{Member, loop_fragment, loop_period, run_notes, runs};

/// The notes and events `clips` (one track's, in start order) play, in
/// arrangement time.
fn heard(clips: &[&Arc<Clip>]) -> (Vec<Note>, Vec<ClipEvent>) {
    let (mut notes, mut events) = (Vec::new(), Vec::new());
    let members = clips.iter().map(|clip| Member {
        clip,
        length: clip.length,
    });
    for run in runs(members) {
        let head = run[0].clip;
        if let Some(period) = loop_period(&run) {
            let length = run.last().expect("run is non-empty").clip.start
                + run.last().expect("run is non-empty").length
                - head.start;
            bake_loop(head, length, period, &mut notes, &mut events);
            continue;
        }
        notes.extend(
            run_notes(&run)
                .into_iter()
                .map(|n| Note::new(head.start + n.start, n.length, n.pitch, n.velocity)),
        );
        for m in &run {
            if let ClipContent::Midi { events: e, .. } = &m.clip.content {
                events.extend(
                    e.iter()
                        .filter(|e| in_window(m.clip, e.at))
                        .map(|e| ClipEvent {
                            at: m.clip.start + e.at,
                            kind: e.kind,
                        }),
                );
            }
        }
    }
    notes.sort_by_key(|n| (n.start(), n.pitch()));
    events.sort_by_key(|e| e.at);
    (notes, events)
}

/// A looping clip's repeats as real notes and events.
fn bake_loop(
    clip: &Clip,
    length: Ticks,
    period: Ticks,
    notes: &mut Vec<Note>,
    events: &mut Vec<ClipEvent>,
) {
    let ClipContent::Midi {
        events: controls, ..
    } = &clip.content
    else {
        return;
    };
    let mut elapsed = Ticks::ZERO;
    while elapsed < length {
        let phase = Ticks((clip.loop_phase.0 + elapsed.0).rem_euclid(period.0));
        let span = if phase == Ticks::ZERO {
            period
        } else {
            period - phase
        }
        .min(length - elapsed);
        let (fragment, _) = loop_fragment(clip, elapsed, span, period);
        notes.extend(fragment.into_iter().map(|n| {
            Note::new(
                clip.start + elapsed + n.start,
                n.length,
                n.pitch,
                n.velocity,
            )
        }));
        for e in controls
            .iter()
            .filter(|e| e.at >= Ticks::ZERO && e.at < period)
        {
            if e.at < Ticks::ZERO || e.at >= period {
                continue;
            }
            let at = Ticks((e.at.0 - phase.0).rem_euclid(period.0));
            if at < span {
                events.push(ClipEvent {
                    at: clip.start + elapsed + at,
                    kind: e.kind,
                });
            }
        }
        elapsed = elapsed + span;
    }
}

/// The tempo map from `from` on, moved to start at zero.
fn tempo_from(project: &Project, from: Ticks) -> Vec<TempoPoint> {
    let Ok(map) = TempoMap::new(project.tempo.clone()) else {
        return project.tempo.clone();
    };
    // A file holds steps only: a ramp comes out as a run of short steps.
    let points = map.stepped();
    let first = points
        .iter()
        .rev()
        .find(|p| p.at <= from)
        .map_or(120.0, |p| p.bpm);
    let mut out = vec![TempoPoint::step(Ticks::ZERO, first)];
    out.extend(
        points
            .iter()
            .filter(|p| p.at > from)
            .map(|p| TempoPoint::step(p.at - from, p.bpm)),
    );
    out
}

/// The time-signature changes from `from` on, moved to start at zero.
fn meter_from(project: &Project, from: Ticks) -> Vec<MeterPoint> {
    let mut out = vec![MeterPoint {
        at: Ticks::ZERO,
        meter: project.meter_map().meter_at(from),
    }];
    out.extend(
        project
            .meter
            .iter()
            .filter(|p| p.at > from)
            .map(|p| MeterPoint {
                at: p.at - from,
                meter: p.meter,
            }),
    );
    out
}

/// The selected MIDI clips `ids` as one part (Export MIDI clip…, type 0):
/// what they play, from the first one's start, named after it, with the
/// tempo from there on. `None` when no MIDI clip is among them.
pub fn clips_file(project: &Project, ids: &[Id]) -> Option<MidiFile> {
    let mut chosen: Vec<(usize, &Arc<Clip>)> = Vec::new();
    for (index, track) in project.tracks.iter().enumerate() {
        for clip in &track.clips {
            if ids.contains(&clip.id) && matches!(clip.content, ClipContent::Midi { .. }) {
                chosen.push((index, clip));
            }
        }
    }
    let from = chosen.iter().map(|(_, c)| c.start).min()?;
    let name = chosen
        .iter()
        .min_by_key(|(t, c)| (c.start, *t))
        .map(|(_, c)| c.name.clone());
    let (mut notes, mut events) = (Vec::new(), Vec::new());
    for (index, _) in project.tracks.iter().enumerate() {
        let clips: Vec<&Arc<Clip>> = chosen
            .iter()
            .filter(|(t, _)| *t == index)
            .map(|(_, c)| *c)
            .collect();
        let (n, e) = heard(&clips);
        notes.extend(n);
        events.extend(e);
    }
    let shift = |t: Ticks| t - from;
    notes = notes
        .into_iter()
        .map(|n| Note::new(shift(n.start()), n.length(), n.pitch(), n.velocity()))
        .collect();
    notes.sort_by_key(|n| (n.start(), n.pitch()));
    events = events
        .into_iter()
        .map(|e| ClipEvent {
            at: shift(e.at),
            kind: e.kind,
        })
        .collect();
    events.sort_by_key(|e| e.at);
    Some(MidiFile {
        tracks: vec![FileTrack {
            name,
            channel: 0,
            source: 0,
            notes,
            events,
        }],
        tempo: tempo_from(project, from),
        meter: meter_from(project, from),
    })
}

/// The project's MIDI tracks (Export MIDI file…, type 1): a part per track
/// that plays something, named after it, on its MIDI To channel (channel 1
/// for the synth), with the tempo map and time signature. Muted clips are
/// left out. `None` when no MIDI track plays anything.
pub fn project_file(project: &Project) -> Option<MidiFile> {
    let tracks: Vec<FileTrack> = project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Midi)
        .filter_map(|track| {
            let clips: Vec<&Arc<Clip>> = track.clips.iter().filter(|c| !c.muted).collect();
            let (notes, events) = heard(&clips);
            (!notes.is_empty() || !events.is_empty()).then(|| FileTrack {
                name: Some(track.name.clone()),
                channel: track.midi_to.as_ref().map_or(0, |out| out.channel),
                source: 0,
                notes,
                events,
            })
        })
        .collect();
    (!tracks.is_empty()).then(|| MidiFile {
        tracks,
        tempo: project.tempo.clone(),
        meter: project.meter.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::model::{EventKind, MidiTo, Track};

    fn q(n: i64) -> Ticks {
        Ticks::quarters(n)
    }

    fn clip(id: u64, start: i64, length: i64, notes: Vec<Note>) -> Arc<Clip> {
        Arc::new(Clip {
            id: Id(id),
            name: format!("Clip {id}"),
            start: q(start),
            length: q(length),
            muted: false,
            color: None,
            locked: false,
            crossfade: false,
            fade_in: Default::default(),
            fade_out: Default::default(),
            loop_length: None,
            loop_phase: Default::default(),
            envelopes: Default::default(),
            audio_phase_seconds: 0.0,
            reversed: false,
            inverted: false,
            takes: None,
            content: ClipContent::Midi {
                notes,
                events: Vec::new(),
            },
        })
    }

    fn project(tracks: Vec<Track>) -> Project {
        let mut project = crate::empty_project();
        project.tracks = tracks.into_iter().map(Arc::new).collect();
        project
    }

    fn midi(id: u64, name: &str, clips: Vec<Arc<Clip>>) -> Track {
        Track {
            clips,
            ..Track::new(
                Id(id),
                name,
                TrackKind::Midi,
                lumen_project::model::Color(0),
            )
        }
    }

    fn starts(notes: &[Note]) -> Vec<(Ticks, Ticks, u8)> {
        notes
            .iter()
            .map(|n| (n.start(), n.length(), n.pitch()))
            .collect()
    }

    #[test]
    fn a_looping_clip_is_baked_and_hidden_and_muted_notes_are_left_out() {
        let mut muted = Note::new(q(1), q(1), 61, 100);
        muted.4 = true;
        let mut looped = (*clip(
            1,
            4,
            10,
            vec![
                Note::new(q(0), q(1), 60, 100),
                muted,
                // Longer than the loop: cut where each repeat ends.
                Note::new(q(3), q(4), 64, 90),
                // Past the loop's end, hidden.
                Note::new(q(5), q(1), 72, 90),
                // Before the clip's start, hidden.
                Note::new(Ticks(-5), q(1), 48, 90),
            ],
        ))
        .clone();
        looped.loop_length = Some(q(4));
        if let ClipContent::Midi { events, .. } = &mut looped.content {
            events.push(ClipEvent {
                at: q(2),
                kind: EventKind::Controller {
                    number: 1,
                    value: 64,
                },
            });
        }
        let p = project(vec![midi(10, "Keys", vec![Arc::new(looped)])]);
        let file = clips_file(&p, &[Id(1)]).unwrap();
        let part = &file.tracks[0];
        assert_eq!(part.name.as_deref(), Some("Clip 1"));
        assert_eq!(
            starts(&part.notes),
            [
                (q(0), q(1), 60),
                (q(3), q(1), 64),
                (q(4), q(1), 60),
                (q(7), q(1), 64),
                // The last repeat is cut by the clip's end at 10.
                (q(8), q(1), 60),
            ]
        );
        let at: Vec<Ticks> = part.events.iter().map(|e| e.at).collect();
        assert_eq!(at, [q(2), q(6)], "the third repeat ends before its event");
    }

    #[test]
    fn a_split_loop_exports_from_its_source_phase() {
        let mut looped = (*clip(
            1,
            4,
            8,
            vec![
                Note::new(q(0), q(1), 60, 100),
                Note::new(q(2), q(2), 62, 100),
            ],
        ))
        .clone();
        looped.loop_length = Some(q(4));
        looped.loop_phase = q(3);
        let p = project(vec![midi(10, "Keys", vec![Arc::new(looped)])]);
        let file = clips_file(&p, &[Id(1)]).unwrap();
        assert_eq!(
            starts(&file.tracks[0].notes),
            [
                (q(0), q(1), 62),
                (q(1), q(1), 60),
                (q(3), q(2), 62),
                (q(5), q(1), 60),
                (q(7), q(1), 62),
            ]
        );
    }

    #[test]
    fn a_note_held_across_a_seam_is_one_note_as_it_plays() {
        // Clip 1 holds a note past its end; clip 2 begins there with the
        // same note, carried on from before its start.
        let p = project(vec![midi(
            10,
            "Keys",
            vec![
                clip(1, 0, 4, vec![Note::new(q(2), q(4), 60, 100)]),
                clip(2, 4, 4, vec![Note::new(q(-2), q(4), 60, 100)]),
            ],
        )]);
        let file = clips_file(&p, &[Id(1), Id(2)]).unwrap();
        assert_eq!(starts(&file.tracks[0].notes), [(q(2), q(4), 60)]);
    }

    #[test]
    fn selected_clips_start_at_zero_with_the_tempo_from_there() {
        let mut p = project(vec![
            midi(
                10,
                "Keys",
                vec![clip(1, 8, 4, vec![Note::new(q(1), q(1), 60, 100)])],
            ),
            midi(
                20,
                "Bass",
                vec![clip(2, 12, 4, vec![Note::new(q(0), q(1), 36, 100)])],
            ),
        ]);
        p.tempo = vec![
            TempoPoint {
                at: q(0),
                bpm: 100.0,
                ramp: false,
            },
            TempoPoint {
                at: q(4),
                bpm: 110.0,
                ramp: false,
            },
            TempoPoint {
                at: q(12),
                bpm: 90.0,
                ramp: false,
            },
        ];
        let file = clips_file(&p, &[Id(1), Id(2), Id(99)]).unwrap();
        assert_eq!(
            starts(&file.tracks[0].notes),
            [(q(1), q(1), 60), (q(4), q(1), 36)]
        );
        assert_eq!(
            file.tempo,
            [
                TempoPoint {
                    at: q(0),
                    bpm: 110.0,
                    ramp: false
                },
                TempoPoint {
                    at: q(4),
                    bpm: 90.0,
                    ramp: false
                },
            ]
        );
        assert_eq!(clips_file(&p, &[Id(99)]), None);
    }

    #[test]
    fn the_project_file_has_a_part_per_playing_midi_track_on_its_channel() {
        let mut hardware = midi(
            20,
            "Hardware",
            vec![clip(2, 0, 4, vec![Note::new(q(0), q(1), 40, 100)])],
        );
        hardware.midi_to = Some(MidiTo {
            port: "Synth".into(),
            channel: 4,
        });
        let mut silent = (*clip(3, 0, 4, vec![Note::new(q(0), q(1), 50, 100)])).clone();
        silent.muted = true;
        let p = project(vec![
            midi(
                10,
                "Keys",
                vec![clip(1, 0, 4, vec![Note::new(q(2), q(1), 60, 100)])],
            ),
            hardware,
            midi(30, "Muted", vec![Arc::new(silent)]),
            Track::new(
                Id(40),
                "Audio",
                TrackKind::Audio,
                lumen_project::model::Color(0),
            ),
        ]);
        let file = project_file(&p).unwrap();
        let parts: Vec<(Option<&str>, u8)> = file
            .tracks
            .iter()
            .map(|t| (t.name.as_deref(), t.channel))
            .collect();
        assert_eq!(parts, [(Some("Keys"), 0), (Some("Hardware"), 4)]);
        assert_eq!(file.tempo, p.tempo);
        assert_eq!(file.meter, p.meter);
        assert_eq!(project_file(&project(Vec::new())), None);
    }
}
