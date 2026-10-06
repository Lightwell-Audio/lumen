//! Transport behaviour, checked sample by sample: a probe source records the
//! timeline sample of every frame the engine plays.

use lumen_core::meter::Meter;
use lumen_core::time::{TempoMap, TempoPoint, Ticks};
use lumen_engine::{
    Command, Engine, EngineHandle, Output, Region, RenderContext, Source, Timeline, engine,
};

const RATE: u32 = 48_000;

/// Records the timeline sample of each rendered frame, and each locate.
#[derive(Default)]
struct Probe {
    played: Vec<i64>,
    locates: Vec<i64>,
}

impl Source for Probe {
    fn render(&mut self, cx: &RenderContext<'_>, out: &mut Output<'_>) {
        for i in 0..out.frames() {
            self.played.push(cx.start + i as i64);
        }
        out.channel(0).fill(1.0);
    }

    fn locate(&mut self, _: &Timeline, _: u32, sample: i64) {
        self.locates.push(sample);
    }
}

fn setup(timeline: Timeline) -> (EngineHandle, Engine<Probe>) {
    engine(timeline, Probe::default(), RATE)
}

/// Processes `frames` frames in blocks of `block` at `rate`.
fn run(engine: &mut Engine<Probe>, frames: usize, block: usize, rate: u32) {
    let mut data = vec![0.0; 2 * block];
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        engine.process(rate, &mut data[..2 * n], 2, n);
        done += n;
    }
}

fn bars(n: i64) -> Ticks {
    Ticks::quarters(4 * n)
}

#[test]
fn play_renders_consecutive_samples_from_the_marker() {
    let (mut handle, mut engine) = setup(Timeline::default());
    handle.send(Command::Locate(Ticks::quarters(1))).unwrap();
    handle.send(Command::Play).unwrap();
    run(&mut engine, 1_000, 128, RATE);
    let expected: Vec<i64> = (24_000..25_000).collect();
    assert_eq!(engine.source().played, expected);
    assert!(handle.transport().playing);
    assert_eq!(handle.transport().sample, 25_000);
}

#[test]
fn silence_while_stopped_and_audio_while_playing() {
    let (mut handle, mut engine) = setup(Timeline::default());
    let mut data = vec![7.0; 2 * 64];
    engine.process(RATE, &mut data, 2, 64);
    assert!(
        data.iter().all(|&s| s == 0.0),
        "stopped output must be zeroed"
    );
    handle.send(Command::Play).unwrap();
    engine.process(RATE, &mut data, 2, 64);
    assert!(data[..64].iter().all(|&s| s == 1.0));
}

#[test]
fn the_loop_wraps_on_the_exact_sample_for_every_block_size() {
    // 120 BPM: one 4/4 bar is 96,000 samples. Loop bar 2.
    let frames = 400_000;
    let mut expected: Vec<i64> = (0..192_000).collect();
    while expected.len() < frames {
        expected.extend(96_000..192_000);
    }
    expected.truncate(frames);
    for block in [1, 3, 64, 128, 333, 4_096, 10_000] {
        let (mut handle, mut engine) = setup(Timeline::default());
        handle
            .send(Command::SetLoop(Region::new(bars(1), bars(2), true)))
            .unwrap();
        handle.send(Command::Play).unwrap();
        run(&mut engine, frames, block, RATE);
        let played = &engine.source().played;
        let first_difference = played.iter().zip(&expected).position(|(a, b)| a != b);
        assert_eq!(first_difference, None, "block size {block}");
        assert_eq!(played.len(), frames);
    }
}

#[test]
fn an_odd_tempo_loop_has_no_gaps_or_repeats() {
    let timeline = Timeline::constant(97.3, Meter::FOUR_FOUR);
    let (start, end) = (
        timeline.sample_at(bars(1), 44_100),
        timeline.sample_at(bars(2), 44_100),
    );
    let (mut handle, mut engine) = engine(timeline, Probe::default(), 44_100);
    handle
        .send(Command::SetLoop(Region::new(bars(1), bars(2), true)))
        .unwrap();
    handle.send(Command::Locate(bars(1))).unwrap();
    handle.send(Command::Play).unwrap();
    run(&mut engine, 500_000, 512, 44_100);
    for pair in engine.source().played.windows(2) {
        let wrapped = pair[0] == end - 1 && pair[1] == start;
        assert!(pair[1] == pair[0] + 1 || wrapped, "{pair:?}");
    }
    assert!(
        engine
            .source()
            .played
            .iter()
            .all(|s| (start..end).contains(s))
    );
}

#[test]
fn playback_that_starts_after_the_loop_plays_on() {
    let (mut handle, mut engine) = setup(Timeline::default());
    handle
        .send(Command::SetLoop(Region::new(bars(1), bars(2), true)))
        .unwrap();
    handle.send(Command::Locate(bars(3))).unwrap();
    handle.send(Command::Play).unwrap();
    run(&mut engine, 100_000, 256, RATE);
    let played = &engine.source().played;
    assert_eq!(played.first(), Some(&288_000));
    assert_eq!(played.last(), Some(&387_999));
}

#[test]
fn stop_returns_to_the_play_marker_then_to_the_start() {
    let (mut handle, mut engine) = setup(Timeline::default());
    handle.send(Command::Locate(bars(2))).unwrap();
    handle.send(Command::Play).unwrap();
    run(&mut engine, 10_000, 128, RATE);
    assert_eq!(handle.transport().sample, 192_000 + 10_000);

    handle.send(Command::Stop).unwrap();
    run(&mut engine, 128, 128, RATE);
    let t = handle.transport();
    assert!(!t.playing);
    assert_eq!((t.sample, t.ticks), (192_000, bars(2)));

    handle.send(Command::Stop).unwrap();
    run(&mut engine, 128, 128, RATE);
    assert_eq!(handle.transport().sample, 0);
}

#[test]
fn pause_keeps_the_position_and_play_continues_from_it() {
    let (mut handle, mut engine) = setup(Timeline::default());
    handle.send(Command::Play).unwrap();
    run(&mut engine, 1_000, 100, RATE);
    handle.send(Command::Pause).unwrap();
    run(&mut engine, 500, 100, RATE);
    assert_eq!(handle.transport().sample, 1_000);
    assert!(!handle.transport().playing);
    handle.send(Command::Play).unwrap();
    run(&mut engine, 100, 100, RATE);
    let expected: Vec<i64> = (0..1_100).collect();
    assert_eq!(engine.source().played, expected);
}

#[test]
fn locate_while_playing_lands_on_the_exact_sample() {
    let (mut handle, mut engine) = setup(Timeline::default());
    handle.send(Command::Play).unwrap();
    run(&mut engine, 512, 128, RATE);
    let triplet = Ticks::whole_note_fraction(1, 12).unwrap(); // 8,000 samples
    handle.send(Command::Locate(bars(1) + triplet)).unwrap();
    run(&mut engine, 128, 128, RATE);
    let played = &engine.source().played;
    assert_eq!(played[511], 511);
    assert_eq!(played[512], 96_000 + 8_000);
    assert_eq!(engine.source().locates.last(), Some(&104_000));
}

#[test]
fn a_tempo_change_keeps_the_bar_and_beat() {
    let (mut handle, mut engine) = setup(Timeline::default());
    handle.send(Command::Locate(bars(2))).unwrap();
    run(&mut engine, 64, 64, RATE);
    assert_eq!(handle.transport().sample, 192_000);

    handle
        .set_timeline(Timeline::constant(90.0, Meter::FOUR_FOUR))
        .unwrap();
    run(&mut engine, 64, 64, RATE);
    let t = handle.transport();
    assert_eq!(t.ticks, bars(2), "musical position kept");
    assert_eq!(t.sample, 256_000, "two bars at 90 BPM");
    assert_eq!(handle.collect_garbage(), 1, "old timeline handed back");

    // While playing, the position stays continuous in ticks across a swap.
    handle.send(Command::Play).unwrap();
    run(&mut engine, 4_800, 64, RATE);
    let before = handle.transport().ticks;
    handle
        .set_timeline(Timeline::constant(174.0, Meter::FOUR_FOUR))
        .unwrap();
    run(&mut engine, 64, 64, RATE);
    let after = handle.transport().ticks;
    // The position lives on whole samples, so carrying it across the swap
    // can move it by up to half a sample.
    let fast = Timeline::constant(174.0, Meter::FOUR_FOUR);
    let one_block = fast.ticks_at(64, RATE).0;
    let one_sample = fast.ticks_at(1, RATE).0;
    assert!(
        (after.0 - before.0 - one_block).abs() <= one_sample,
        "{before:?} -> {after:?}"
    );
}

#[test]
fn loop_and_punch_follow_a_tempo_change() {
    let (mut handle, mut engine) = setup(Timeline::default());
    handle
        .send(Command::SetLoop(Region::new(bars(1), bars(2), true)))
        .unwrap();
    handle
        .set_timeline(Timeline::new(
            TempoMap::new(vec![
                TempoPoint {
                    at: Ticks::ZERO,
                    bpm: 120.0,
                },
                TempoPoint {
                    at: bars(1),
                    bpm: 60.0,
                },
            ])
            .unwrap(),
            lumen_core::meter::MeterMap::constant(Meter::FOUR_FOUR).unwrap(),
        ))
        .unwrap();
    handle.send(Command::Locate(bars(1))).unwrap();
    handle.send(Command::Play).unwrap();
    // Bar 2 is now 192,000 samples long (60 BPM), from 96,000 to 288,000.
    run(&mut engine, 200_000, 1_000, RATE);
    let played = &engine.source().played;
    assert_eq!(played[191_999], 287_999);
    assert_eq!(played[192_000], 96_000);
}

#[test]
fn a_sample_rate_change_keeps_the_bar_and_beat() {
    let (mut handle, mut engine) = setup(Timeline::default());
    handle
        .send(Command::SetLoop(Region::new(bars(1), bars(2), true)))
        .unwrap();
    handle
        .send(Command::Locate(bars(1) + Ticks::quarters(2)))
        .unwrap();
    run(&mut engine, 64, 64, RATE);
    assert_eq!(handle.transport().sample, 144_000);

    // The device switched to 44.1 kHz (MT-AUDIO-6).
    run(&mut engine, 64, 64, 44_100);
    let t = handle.transport();
    assert_eq!((t.sample_rate, t.sample), (44_100, 132_300));
    assert_eq!(t.ticks, bars(1) + Ticks::quarters(2));

    // The loop end moved with it: bar 3 starts at 176,400 samples.
    handle.send(Command::Play).unwrap();
    run(&mut engine, 50_000, 64, 44_100);
    let played = &engine.source().played;
    assert_eq!(played[44_099], 176_399);
    assert_eq!(played[44_100], 88_200);
}

#[test]
fn punching_is_reported_inside_the_punch_region() {
    let (mut handle, mut engine) = setup(Timeline::default());
    handle
        .send(Command::SetPunch(Region::new(bars(1), bars(2), true)))
        .unwrap();
    handle.send(Command::Play).unwrap();
    run(&mut engine, 95_744, 128, RATE);
    assert!(!handle.transport().punching);
    run(&mut engine, 256, 128, RATE);
    assert!(handle.transport().punching);
    run(&mut engine, 96_000, 128, RATE);
    assert!(!handle.transport().punching);
}

#[test]
fn disabled_or_empty_regions_do_nothing() {
    for region in [
        Region::new(bars(1), bars(2), false),
        Region::new(bars(2), bars(2), true),
        Region::new(bars(2), bars(1), true),
    ] {
        let (mut handle, mut engine) = setup(Timeline::default());
        handle.send(Command::SetLoop(region)).unwrap();
        handle.send(Command::Play).unwrap();
        run(&mut engine, 300_000, 4_096, RATE);
        assert_eq!(engine.source().played.last(), Some(&299_999), "{region:?}");
    }
}
