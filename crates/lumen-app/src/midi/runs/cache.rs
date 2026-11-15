use std::collections::HashMap;
use std::sync::Arc;

use lumen_core::time::Ticks;
use lumen_engine::{MidiClip, MidiControl, MidiNote, control_list, note_list};
use lumen_project::model::{self, DrumMap, Id};

use super::notes::{loop_fragment, loop_pattern, run_controls, run_notes, run_notes_masked};
use super::{Dest, Member, Suppression, extent, loop_period};

struct Pattern {
    members: Vec<(Arc<model::Clip>, Ticks)>,
    map: Option<Arc<DrumMap>>,
    notes: Arc<[MidiNote]>,
    controls: Arc<[MidiControl]>,
}

impl Pattern {
    fn matches(&self, run: &[Member<'_>], map: Option<&DrumMap>) -> bool {
        self.map.as_deref() == map
            && self.members.len() == run.len()
            && self.members.iter().zip(run).all(|((clip, length), m)| {
                *length == m.length && (Arc::ptr_eq(clip, m.clip) || **clip == **m.clip)
            })
    }
}

struct Cached {
    members: Vec<(Arc<model::Clip>, Ticks)>,
    map: Option<Arc<DrumMap>>,
    suppression: Option<Suppression>,
    clips: Vec<MidiClip>,
}

impl Cached {
    fn matches(
        &self,
        run: &[Member<'_>],
        map: Option<&DrumMap>,
        suppression: Option<Suppression>,
    ) -> bool {
        self.suppression == suppression
            && self.map.as_deref() == map
            && self.members.len() == run.len()
            && self.members.iter().zip(run).all(|((clip, length), m)| {
                *length == m.length && (Arc::ptr_eq(clip, m.clip) || **clip == **m.clip)
            })
    }
}

#[derive(Default)]
pub struct Cache {
    runs: HashMap<Key, Cached>,
    patterns: HashMap<Key, Pattern>,
    maps: HashMap<Id, Arc<DrumMap>>,
}

type Key = (Id, Dest);

impl Cache {
    pub(super) fn shared(&mut self, map: Option<&DrumMap>) -> Option<Arc<DrumMap>> {
        let map = map?;
        if let Some(known) = self.maps.get(&map.id)
            && **known == *map
        {
            return Some(Arc::clone(known));
        }
        let fresh = Arc::new(map.clone());
        self.maps.insert(map.id, Arc::clone(&fresh));
        Some(fresh)
    }

    pub(crate) fn clip_with_suppression(
        &mut self,
        run: &[Member<'_>],
        suppression: Option<Suppression>,
    ) -> &[MidiClip] {
        let head = run[0].clip;
        let key = (head.id, run[0].dest);
        let map = self.shared(run[0].map);
        let cached_matches = self
            .runs
            .get(&key)
            .is_some_and(|c| c.matches(run, map.as_deref(), suppression));

        if !cached_matches {
            let period = loop_period(run);
            let pattern = match self.patterns.get(&key) {
                Some(cached) if cached.matches(run, map.as_deref()) => {
                    (Arc::clone(&cached.notes), Arc::clone(&cached.controls))
                }
                _ => {
                    let source = Member::own(head, head.length, run[0].map).sending(run[0].dest);
                    let source_run = if period.is_some() {
                        std::slice::from_ref(&source)
                    } else {
                        run
                    };
                    let (notes, controls) = if let Some(period) = period {
                        loop_pattern(head, period, run[0].map, run[0].dest)
                    } else {
                        (
                            note_list(run_notes(source_run)),
                            control_list(run_controls(source_run, None)),
                        )
                    };
                    self.patterns.insert(
                        key,
                        Pattern {
                            members: run.iter().map(|m| (Arc::clone(m.clip), m.length)).collect(),
                            map: map.clone(),
                            notes: Arc::clone(&notes),
                            controls: Arc::clone(&controls),
                        },
                    );
                    (notes, controls)
                }
            };

            let (start, total_length, _) = extent(run);
            let split_loop = period.is_some()
                && suppression.is_some_and(|s| {
                    s.from < s.until && s.from < start + total_length && start < s.until
                });
            let suppression_applies = suppression.is_some_and(|s| {
                run.iter()
                    .any(|m| s.from < m.clip.start + m.clip.length && m.clip.start < s.until)
            });
            let seams: Vec<Ticks> = run[1..].iter().map(|m| m.clip.start - head.start).collect();
            let mut clips = Vec::new();
            if split_loop {
                let suppression = suppression.expect("split loop has a bounded suppression window");
                let period = period.expect("split loop has a positive period");
                let clip_end = start + total_length;
                let mask_from = suppression.from.max(start);
                let mask_until = suppression.until.min(clip_end);
                clips.extend(loop_segments(
                    head,
                    LoopSpan {
                        start,
                        elapsed: Ticks::ZERO,
                        length: mask_from - start,
                        period,
                    },
                    &pattern.0,
                    &pattern.1,
                    &seams,
                    run[0].map,
                    run[0].dest,
                ));
                let suffix_elapsed = mask_until - start;
                clips.extend(loop_segments(
                    head,
                    LoopSpan {
                        start: mask_until,
                        elapsed: suffix_elapsed,
                        length: clip_end - mask_until,
                        period,
                    },
                    &pattern.0,
                    &pattern.1,
                    &seams,
                    run[0].map,
                    run[0].dest,
                ));
            } else if let Some(period) = period.filter(|p| head.loop_phase.0.rem_euclid(p.0) != 0) {
                let (start, length, _) = extent(run);
                clips.extend(loop_segments(
                    head,
                    LoopSpan {
                        start,
                        elapsed: Ticks::ZERO,
                        length,
                        period,
                    },
                    &pattern.0,
                    &pattern.1,
                    &seams,
                    run[0].map,
                    run[0].dest,
                ));
            } else {
                let (start, length, period) = extent(run);
                let (notes, controls) = if !suppression_applies {
                    pattern
                } else {
                    (
                        note_list(run_notes_masked(run, suppression)),
                        control_list(run_controls(run, suppression)),
                    )
                };
                clips.push(engine_clip(
                    head.id,
                    start,
                    length,
                    period,
                    notes,
                    controls,
                    run[1..].iter().map(|m| m.clip.start - head.start).collect(),
                ));
            }
            self.runs.insert(
                key,
                Cached {
                    members: run.iter().map(|m| (Arc::clone(m.clip), m.length)).collect(),
                    map: map.clone(),
                    suppression,
                    clips: clips.clone(),
                },
            );
        }
        self.runs
            .get(&key)
            .map(|cached| cached.clips.as_slice())
            .expect("the MIDI run cache was filled above")
    }

    pub(crate) fn retain(&mut self, project: &model::Project) {
        let live: std::collections::HashSet<Id> = project
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        if self.runs.len() > live.len() {
            self.runs.retain(|key, _| live.contains(&key.0));
        }
        if self.patterns.len() > live.len() {
            self.patterns.retain(|key, _| live.contains(&key.0));
        }
        if self.maps.len() > project.drum_maps.len() {
            self.maps
                .retain(|id, _| project.drum_maps.iter().any(|map| map.id == *id));
        }
    }
}

fn engine_clip(
    id: Id,
    start: Ticks,
    length: Ticks,
    period: Option<Ticks>,
    notes: Arc<[MidiNote]>,
    controls: Arc<[MidiControl]>,
    seams: Arc<[Ticks]>,
) -> MidiClip {
    MidiClip {
        id: id.0,
        start,
        length,
        period,
        notes,
        controls,
        seams,
    }
}

struct LoopSpan {
    start: Ticks,
    elapsed: Ticks,
    length: Ticks,
    period: Ticks,
}

fn loop_segments(
    clip: &model::Clip,
    span: LoopSpan,
    notes: &Arc<[MidiNote]>,
    controls: &Arc<[MidiControl]>,
    seams: &[Ticks],
    map: Option<&DrumMap>,
    dest: Dest,
) -> Vec<MidiClip> {
    let LoopSpan {
        start,
        elapsed,
        length,
        period,
    } = span;
    if length <= Ticks::ZERO {
        return Vec::new();
    }
    let phase = Ticks((clip.loop_phase.0 + elapsed.0).rem_euclid(period.0));
    if phase == Ticks::ZERO {
        return vec![engine_clip(
            clip.id,
            start,
            length,
            Some(period),
            Arc::clone(notes),
            Arc::clone(controls),
            segment_seams(elapsed, length, seams),
        )];
    }
    let first = (period - phase).min(length);
    let (fragment_notes, fragment_controls) =
        loop_fragment(clip, elapsed, first, period, map, dest);
    let mut segments = vec![engine_clip(
        clip.id,
        start,
        first,
        None,
        note_list(fragment_notes),
        control_list(fragment_controls),
        segment_seams(elapsed, first, seams),
    )];
    if first < length {
        segments.push(engine_clip(
            clip.id,
            start + first,
            length - first,
            Some(period),
            Arc::clone(notes),
            Arc::clone(controls),
            segment_seams(elapsed + first, length - first, seams),
        ));
    }
    segments
}

fn segment_seams(elapsed: Ticks, length: Ticks, seams: &[Ticks]) -> Arc<[Ticks]> {
    let end = elapsed + length;
    seams
        .iter()
        .filter(|&&seam| elapsed < seam && seam < end)
        .map(|&seam| seam - elapsed)
        .collect::<Vec<_>>()
        .into()
}
