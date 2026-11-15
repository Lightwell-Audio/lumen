use std::collections::HashMap;

use lumen_core::time::Ticks;
use lumen_engine::{Clip, ClipAudio};
use lumen_project::model::{ClipContent, Id, Project};

use crate::media::warped::{Identity, Part, merge_pitch};

fn unlooped_warps(project: &Project) -> std::borrow::Cow<'_, Project> {
    use std::borrow::Cow;
    let loops: Vec<Id> = project
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .filter(|clip| clip.warp.is_some() && clip.loop_length.is_some())
        .map(|clip| clip.id)
        .collect();
    if loops.is_empty() {
        return Cow::Borrowed(project);
    }
    let mut copy = project.clone();
    for track in &mut copy.tracks {
        if track
            .clips
            .iter()
            .any(|clip| loops.contains(&clip.id) && clip.locked)
        {
            for clip in &mut std::sync::Arc::make_mut(track).clips {
                if loops.contains(&clip.id) {
                    std::sync::Arc::make_mut(clip).locked = false;
                }
            }
        }
    }
    for id in loops {
        let mut made = 0_u64;
        let mut fresh = || {
            made += 1;
            Id((1 << 63) | ((id.0 & 0x7FFF_FFFF_FFFF) << 16) | made)
        };
        lumen_project::edit::clip_ops::unloop(&mut copy, &[id], &mut fresh);
    }
    Cow::Owned(copy)
}
#[cfg(test)]
pub fn engine_clips(
    project: &Project,
    rate: u32,
    audio: impl Fn(Id) -> Option<(std::sync::Arc<lumen_engine::ClipAudio>, u32)>,
) -> Vec<Clip> {
    engine_clips_with(project, rate, audio, &mut |_| None)
}

pub fn engine_clips_with(
    project: &Project,
    rate: u32,
    audio: impl Fn(Id) -> Option<(std::sync::Arc<lumen_engine::ClipAudio>, u32)>,
    warped: &mut dyn FnMut(&crate::media::warped::Request<'_>) -> Option<std::sync::Arc<ClipAudio>>,
) -> Vec<Clip> {
    let unlooped = unlooped_warps(project);
    let project = &*unlooped;
    let tempo = lumen_core::time::TempoMap::new(project.tempo.clone()).ok();
    let mut list = Vec::new();
    let played = crate::crossfades::played(project);
    let pitch = pitch_curves(project, &played);
    let reach = tempo.as_ref().map_or(0, |tempo| {
        let end = project
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .map(|model| {
                let clip = played.get(&model.id).map_or(&**model, |p| &p.clip);
                clip.start + clip.length
            })
            .max();
        end.map_or(0, |end| tempo.sample_at(end, rate))
    });
    for track in &project.tracks {
        for model in &track.clips {
            let (clip, blend) = played
                .get(&model.id)
                .map_or((&**model, [false; 2]), |p| (&p.clip, p.blend));
            let ClipContent::Audio {
                media,
                offset,
                gain_db,
            } = &clip.content
            else {
                continue;
            };
            let key = if clip.reversed {
                Id(crate::media::reversed_key(media.0))
            } else {
                *media
            };
            let Some((samples, native)) = audio(key) else {
                continue;
            };
            let base_offset = (*offset as f64 * f64::from(samples.sample_rate())
                / f64::from(native.max(1))) as i64;
            let source_origin = if clip.loop_length.is_some() {
                clip.start - clip.loop_phase
            } else {
                clip.start
            };
            let start_sample = tempo.as_ref().map(|t| t.sample_at(clip.start, rate));
            let warped_audio = clip
                .warp
                .as_ref()
                .zip(tempo.as_ref())
                .filter(|_| clip.loop_length.is_none() && start_sample.is_some_and(|s| s >= 0))
                .and_then(|(warp, tempo)| {
                    warped(&crate::media::warped::Request {
                        warp,
                        source: &samples,
                        tempo,
                        rate,
                        origin: source_origin,
                        reach,
                        pitch: pitch.of(clip.id),
                    })
                });
            let (samples, streamed_warp) = match warped_audio {
                Some(stream) => (stream, true),
                None => (samples, false),
            };
            let offset = if streamed_warp {
                start_sample.unwrap_or(0)
            } else {
                tempo.as_ref().map_or(base_offset, |tempo| {
                    let timeline_sample = tempo.sample_at(source_origin, rate);
                    let source_anchor = ((tempo.seconds_at(source_origin)
                        - clip.audio_phase_seconds)
                        * f64::from(rate))
                    .round() as i64;
                    base_offset + timeline_sample - source_anchor
                })
            };
            let frames = |a: Ticks, b: Ticks| {
                tempo
                    .as_ref()
                    .map_or(0, |t| t.sample_at(b, rate) - t.sample_at(a, rate))
            };
            let end = clip.start + clip.length;
            let region = if tempo.is_some() {
                frames(clip.start, end)
            } else {
                i64::MAX
            };
            let length = if clip.loop_length.is_some() {
                region
            } else {
                (samples.frames() as i64 - offset).min(region)
            };
            let shape = lumen_engine::ClipShape {
                fade_in: if clip.crossfade && !blend[0] {
                    Default::default()
                } else {
                    fade_frames(clip, tempo.as_ref(), rate, true, blend[0])
                },
                fade_out: fade_frames(clip, tempo.as_ref(), rate, false, blend[1]),
                period: clip
                    .loop_length
                    .map(|period| {
                        let origin = clip.start - clip.loop_phase;
                        frames(origin, origin + period)
                    })
                    .filter(|p| *p > 0),
                phase: clip.loop_length.map_or(0, |period| {
                    let origin = clip.start - clip.loop_phase;
                    let period_frames = frames(origin, origin + period);
                    if period_frames > 0 {
                        frames(origin, clip.start).rem_euclid(period_frames)
                    } else {
                        0
                    }
                }),
            };
            if length > 0 {
                list.push(Clip {
                    id: clip.id.0,
                    track: track.id.0,
                    start: clip.start,
                    offset,
                    length,
                    gain: if clip.muted {
                        0.0
                    } else if clip.inverted {
                        -crate::routing::db_to_gain(*gain_db)
                    } else {
                        crate::routing::db_to_gain(*gain_db)
                    },
                    envelope: envelope_of(clip, project, tempo.as_ref(), rate, samples.channels()),
                    audio: samples,
                    shape,
                });
            }
        }
    }
    list
}

#[derive(Default)]
struct PitchPlan {
    curves: HashMap<Identity, lumen_core::automation::Lane>,
    streams: HashMap<Id, Identity>,
}

impl PitchPlan {
    fn of(&self, clip: Id) -> Option<&lumen_core::automation::Lane> {
        self.curves.get(self.streams.get(&clip)?)
    }
}

fn pitch_curves(project: &Project, played: &HashMap<Id, crate::crossfades::Played>) -> PitchPlan {
    let mut groups: HashMap<Identity, Vec<(Id, usize, Part<'_>)>> = HashMap::new();
    for (track_at, track) in project.tracks.iter().enumerate() {
        for model in &track.clips {
            let clip = played.get(&model.id).map_or(&**model, |p| &p.clip);
            let (Some(warp), ClipContent::Audio { media, .. }) = (&clip.warp, &clip.content) else {
                continue;
            };
            if clip.loop_length.is_some() || !warp.mode.transposes() {
                continue;
            }
            let key = if clip.reversed {
                Id(crate::media::reversed_key(media.0))
            } else {
                *media
            };
            groups
                .entry(Identity::of(warp, key, clip.start))
                .or_default()
                .push((clip.id, track_at, Part::of(clip)));
        }
    }
    let mut plan = PitchPlan::default();
    for (identity, parts) in groups {
        let mut layered: std::collections::HashSet<Id> = std::collections::HashSet::new();
        for (i, (x, track_x, a)) in parts.iter().enumerate() {
            for (y, track_y, b) in &parts[i + 1..] {
                let crossfade =
                    track_x == track_y && played.contains_key(x) && played.contains_key(y);
                if a.start < b.start + b.length
                    && b.start < a.start + a.length
                    && (a.lane != b.lane || a.start != b.start)
                    && !crossfade
                {
                    layered.insert(*x);
                    layered.insert(*y);
                }
            }
        }
        let (alone, shared): (Vec<_>, Vec<_>) = parts
            .into_iter()
            .partition(|(id, _, _)| layered.contains(id));
        for (id, _, part) in alone {
            let own = identity.alone(id);
            plan.streams.insert(id, own);
            if let Some(lane) = merge_pitch(std::slice::from_ref(&part)) {
                plan.curves.insert(own, lane);
            }
        }
        let parts: Vec<_> = shared
            .into_iter()
            .map(|(id, _, part)| {
                plan.streams.insert(id, identity);
                part
            })
            .collect();
        if let Some(lane) = merge_pitch(&parts) {
            plan.curves.insert(identity, lane);
        }
    }
    plan
}

pub(super) fn reversed_media(project: &Project) -> Vec<u64> {
    let mut wanted: Vec<u64> = project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .filter(|c| c.reversed)
        .filter_map(|c| match &c.content {
            ClipContent::Audio { media, .. } => Some(media.0),
            ClipContent::Midi { .. } => None,
        })
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    wanted
}

fn fade_frames(
    clip: &lumen_project::model::Clip,
    tempo: Option<&lumen_core::time::TempoMap>,
    rate: u32,
    fade_in: bool,
    blend: bool,
) -> lumen_engine::FadeFrames {
    let fade = if fade_in { clip.fade_in } else { clip.fade_out };
    let end = clip.start + clip.length;
    let frames = |a, b| {
        tempo.map_or(0, |tempo| {
            tempo.sample_at(b, rate) - tempo.sample_at(a, rate)
        })
    };
    let visible = if fade_in {
        frames(clip.start, clip.start + fade.length)
    } else {
        frames(end - fade.length, end)
    };
    let (offset, span) = if fade.span == Ticks::ZERO {
        (0, 0)
    } else if fade_in {
        let origin = clip.start - fade.offset;
        (
            frames(origin, clip.start),
            frames(origin, origin + fade.span),
        )
    } else {
        let origin_end = end + fade.offset;
        (
            frames(end, origin_end),
            frames(origin_end - fade.span, origin_end),
        )
    };
    lumen_engine::FadeFrames {
        frames: visible,
        offset,
        span,
        curve: fade.curve,
        blend,
    }
}

fn envelope_of(
    clip: &lumen_project::model::Clip,
    project: &Project,
    tempo: Option<&lumen_core::time::TempoMap>,
    rate: u32,
    channels: usize,
) -> lumen_engine::ClipEnvelope {
    let has_gain_or_pan = clip.envelopes.gain.is_some() || clip.envelopes.pan.is_some();
    let Some(tempo) = tempo.filter(|_| has_gain_or_pan) else {
        return Default::default();
    };
    let origin = if clip.loop_length.is_some() {
        clip.start - clip.loop_phase
    } else {
        clip.start
    };
    let bake = |kind| {
        let lane = clip.envelopes.get(kind)?;
        crate::lane_bake::bake_from(lane, origin, tempo, rate)
    };
    let law = if channels == 1 {
        project.pan_law
    } else {
        lumen_core::pan::PanLaw::Balance
    };
    lumen_engine::ClipEnvelope {
        gain: bake(lumen_project::envelope::Kind::Gain),
        pan: bake(lumen_project::envelope::Kind::Pan).map(|lane| (lane, law)),
    }
}

pub(super) fn audio_replacing(
    project: &Project,
    inputs: &crate::midi::play::Inputs,
) -> Vec<crate::midi::play::Replacing> {
    inputs
        .replacing
        .iter()
        .filter(|r| {
            project
                .tracks
                .iter()
                .any(|t| t.id == r.track && t.kind == lumen_project::model::TrackKind::Audio)
        })
        .cloned()
        .collect()
}

pub(super) fn without_replaced<'a>(
    project: std::borrow::Cow<'a, Project>,
    replacing: &[crate::midi::play::Replacing],
) -> std::borrow::Cow<'a, Project> {
    if replacing.is_empty() {
        return project;
    }
    let mut copy = project.into_owned();
    let mut next = u64::MAX;
    let mut fresh = || {
        next -= 1;
        lumen_project::model::Id(next)
    };
    for r in replacing {
        let until = r.until.unwrap_or(Ticks(i64::MAX / 8));
        lumen_project::edit::clear(&mut copy, &[r.track], r.from, until, &mut fresh);
    }
    std::borrow::Cow::Owned(copy)
}
