//! One track's part of a block: reading its clips, handing a hosted
//! instrument its block early, and rendering the instrument into the track's
//! bus.

use std::time::{Duration, Instant};

use lumen_core::midi::Event;
use lumen_devices::ParamBlock;

use super::hold::{self, Clock};
use super::player::Out;
use super::{MidiTrack, Target};
use crate::mixer::{Buses, CHUNK, fill_param_lane};
use crate::source::{Mode, RenderContext};

use super::InstrumentSpec;

/// What a block with no lane to play says to a hosted instrument.
static NO_PARAMS: ParamBlock = ParamBlock::none();

/// A part's first half for one track: reads its clips into the track's event
/// queue, in order.
pub(super) fn gather_track(
    track: &mut MidiTrack,
    cx: &RenderContext<'_>,
    frames: usize,
    clock: Clock<'_>,
) {
    let channel = track.channel();
    if cx.mode == Mode::Play && track.sounds() {
        let from = cx.start;
        hold::gather(
            &track.clips,
            &mut track.state,
            channel,
            clock,
            (from, from + frames as i64),
            track.kit.as_deref(),
        );
    }
    hold::sort_events(&mut track.state.events);
    // What the track's instrument is told for this block: the values the
    // project holds for a built-in instrument's knobs (M8 slice B5), and, while
    // the song plays, the points its lanes give them. An instrument keeps the
    // block it was last given, so a block with nothing to say says so
    // (`NO_PARAMS`).
    //
    // The values are given in **every** mode (M8 slice B5, review H3): a knob
    // turned while the transport is stopped, or with a note held on live input,
    // must reach the instrument with the next block, exactly as an effect's do.
    // Only the lanes are gated on Play, because a lane taken away (or a stopped
    // song) must not go on setting its last value. B4 (Flare) reads the same
    // block for the synth's parameters, so keep this one gate.
    // The tempo a synced part of it follows goes with it (M8 slice B4).
    if let Some(spec) = track.instrument.as_mut() {
        let bpm = cx
            .timeline
            .tempo
            .bpm_at(cx.timeline.ticks_at(cx.start, cx.sample_rate));
        spec.instrument.set_tempo(bpm as f32);
        let playing = cx.mode == Mode::Play;
        let has_lanes = playing && !track.automation.is_empty();
        match track.params.as_deref_mut() {
            Some(block) if !track.values.is_empty() || has_lanes => {
                block.start(frames);
                for &(id, value) in &track.values {
                    if block.open(id) {
                        block.point(0, f64::from(value));
                    }
                }
                if playing {
                    for lane in &track.automation {
                        fill_param_lane(block, lane, cx.start, frames);
                    }
                }
                spec.instrument.automate(block);
            }
            _ => spec.instrument.automate(&NO_PARAMS),
        }
    }
}

/// Hands a hosted instrument its block now, so its host works while the
/// engine renders the clips (ADR-0069). A built-in instrument is left to
/// [`play_track`].
pub(super) fn start_host(track: &mut MidiTrack, cx: &RenderContext<'_>, frames: usize) {
    let events: &[Event] = match track.target {
        Target::Instrument => &track.state.events,
        Target::Port { .. } => &[],
    };
    if let Some(spec) = track.instrument.as_mut()
        && spec.instrument.hosted()
    {
        track.begun = spec.instrument.begin(cx.sample_rate, frames, events);
    }
}

/// A part's second half for one track: sends hardware events out, and renders
/// the instrument into the track's bus (or collects it, when it began early).
pub(super) fn play_track(
    track: &mut MidiTrack,
    cx: &RenderContext<'_>,
    buses: &mut Buses<'_>,
    out: &mut Out,
) {
    let frames = buses.frames();
    let id = track.id;
    let sizes = (frames, cx.sample_rate);
    match track.target {
        Target::Port { .. } => {
            out.forward(track, cx.stream);
            render_instrument(track, id, sizes, cx.deadline, buses, false);
        }
        Target::Instrument => render_instrument(track, id, sizes, cx.deadline, buses, true),
    }
}

/// Renders the track's instrument into its bus: with the queued events when
/// `events`, otherwise only what still rings. An instrument that began early
/// is collected, waiting for its host until `deadline`.
pub(super) fn render_instrument(
    track: &mut MidiTrack,
    id: u64,
    (frames, cx_rate): (usize, u32),
    deadline: Option<Instant>,
    buses: &mut Buses<'_>,
    events: bool,
) {
    ring_out(track, id, (frames, cx_rate), buses);
    if std::mem::take(&mut track.begun)
        && let Some(spec) = track.instrument.as_mut()
    {
        let mut left = [0.0f32; CHUNK];
        let mut right = [0.0f32; CHUNK];
        let deadline = deadline.unwrap_or_else(|| {
            Instant::now()
                + Duration::from_micros(frames as u64 * 750_000 / u64::from(cx_rate.max(1)))
        });
        spec.instrument
            .finish(deadline, &mut left[..frames], &mut right[..frames]);
        if let Some(mut bus) = buses.track(id) {
            for (side, samples) in [(0, &left), (1, &right)] {
                for (to, from) in bus.channel(side).iter_mut().zip(samples) {
                    *to += from;
                }
            }
        }
        play_outputs(&track.outputs, spec, frames, buses);
        track.state.events.clear();
        return;
    }
    let queued: &[Event] = if events { &track.state.events } else { &[] };
    if let Some(spec) = track.instrument.as_mut()
        && (!queued.is_empty() || spec.instrument.active())
    {
        let mut left = [0.0f32; CHUNK];
        let mut right = [0.0f32; CHUNK];
        spec.instrument.render_by(
            cx_rate,
            queued,
            &mut left[..frames],
            &mut right[..frames],
            deadline,
        );
        if let Some(mut bus) = buses.track(id) {
            for (side, samples) in [(0, &left), (1, &right)] {
                for (to, from) in bus.channel(side).iter_mut().zip(samples) {
                    *to += from;
                }
            }
        }
        play_outputs(&track.outputs, spec, frames, buses);
    }
    if events {
        track.state.events.clear();
    }
}

/// Plays the release of an instrument the track no longer has into its bus,
/// while anything of it still sounds.
fn ring_out(track: &mut MidiTrack, id: u64, (frames, rate): (usize, u32), buses: &mut Buses<'_>) {
    let Some(spec) = track.leaving.as_mut().filter(|s| s.instrument.active()) else {
        return;
    };
    let mut left = [0.0f32; CHUNK];
    let mut right = [0.0f32; CHUNK];
    spec.instrument
        .render(rate, &[], &mut left[..frames], &mut right[..frames]);
    if let Some(mut bus) = buses.track(id) {
        for (side, samples) in [(0, &left), (1, &right)] {
            for (to, from) in bus.channel(side).iter_mut().zip(samples) {
                *to += from;
            }
        }
    }
}

/// Adds each of the instrument's other outputs that is on to the bus of the
/// track that plays it, for the block just rendered. A track the routing does
/// not have hears nothing.
pub(super) fn play_outputs(
    outputs: &[(u32, u64)],
    spec: &mut InstrumentSpec,
    frames: usize,
    buses: &mut Buses<'_>,
) {
    for &(pair, target) in outputs {
        let mut left = [0.0f32; CHUNK];
        let mut right = [0.0f32; CHUNK];
        if !spec
            .instrument
            .output(pair as usize, &mut left[..frames], &mut right[..frames])
        {
            continue;
        }
        if let Some(mut bus) = buses.track(target) {
            for (side, samples) in [(0, &left), (1, &right)] {
                for (to, from) in bus.channel(side).iter_mut().zip(samples) {
                    *to += from;
                }
            }
        }
    }
}

/// Adds the next frames of what the track's instrument sounds now to its
/// bus, without moving the instrument on.
pub(super) fn preview_instrument(
    track: &mut MidiTrack,
    id: u64,
    (frames, rate): (usize, u32),
    buses: &mut Buses<'_>,
) {
    let Some(spec) = track.instrument.as_mut().filter(|s| s.instrument.active()) else {
        return;
    };
    let mut left = [0.0f32; CHUNK];
    let mut right = [0.0f32; CHUNK];
    spec.instrument
        .preview(rate, &mut left[..frames], &mut right[..frames]);
    if let Some(mut bus) = buses.track(id) {
        for (side, samples) in [(0, &left), (1, &right)] {
            for (to, from) in bus.channel(side).iter_mut().zip(samples) {
                *to += from;
            }
        }
    }
}
