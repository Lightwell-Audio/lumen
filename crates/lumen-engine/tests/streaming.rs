//! Clips played from disk (M9 slice R6): with its pages in memory a streamed
//! clip plays exactly what the same clip in memory plays; with a page
//! missing it fades out before it and counts one underrun.

use std::sync::Arc;

use lumen_core::fade::FadeShape;
use lumen_core::time::Ticks;
use lumen_engine::{
    Clip, ClipAudio, ClipPlayer, ClipShape, ClipsHandle, Command, Destination, Engine,
    EngineHandle, FadeFrames, Mixer, NodeSpec, PAGE, PagePool, PoolWriter, Region, Routing,
    StreamAudio, Timeline, clip_player, engine, mixer,
};

const RATE: u32 = 48_000;

/// Every sample distinct: left `(i + 1) / 1e7`, right negated.
fn ramp(frames: usize) -> (Vec<f32>, Vec<f32>) {
    let left: Vec<f32> = (0..frames).map(|i| (i + 1) as f32 / 1e7).collect();
    let right = left.iter().map(|v| -v).collect();
    (left, right)
}

fn in_memory(frames: usize) -> Arc<ClipAudio> {
    let (left, right) = ramp(frames);
    Arc::new(ClipAudio::Memory {
        sample_rate: RATE,
        channels: vec![left, right],
    })
}

/// The same audio as a stream in its own pool, with `pages` of it loaded.
fn streamed(
    frames: usize,
    pages: std::ops::Range<usize>,
) -> (Arc<ClipAudio>, StreamAudio, PoolWriter) {
    let pool = PagePool::new(frames.div_ceil(PAGE) as u32);
    let mut writer = PoolWriter::new(&pool);
    let stream = StreamAudio::new(&pool, 1, RATE, 2, frames as u64);
    for page in pages {
        load(&mut writer, &stream, page);
    }
    (Arc::new(ClipAudio::Stream(stream.clone())), stream, writer)
}

fn load(writer: &mut PoolWriter, stream: &StreamAudio, page: usize) {
    let (left, right) = ramp(stream.frames() as usize);
    let to = ((page + 1) * PAGE).min(left.len());
    assert!(writer.load(
        stream,
        page,
        &left[page * PAGE..to],
        &right[page * PAGE..to]
    ));
}

type Player = Engine<Mixer<ClipPlayer>>;

fn setup(clips: Vec<Clip>) -> (EngineHandle, ClipsHandle, Player) {
    let (mut clips_handle, player) = clip_player();
    clips_handle.set(clips).unwrap();
    let (mut routing, source) = mixer(player);
    assert!(
        routing
            .set(&Routing {
                nodes: vec![NodeSpec::unity(0, Destination::Hardware(0))],
                cue: None,
                measure: None,
            })
            .unwrap()
    );
    let (handle, engine) = engine(Timeline::default(), source, RATE);
    (handle, clips_handle, engine)
}

fn run(engine: &mut Player, frames: usize, block: usize) -> Vec<(f32, f32)> {
    let mut data = vec![0.0; 2 * block];
    let mut out = Vec::with_capacity(frames);
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        engine.process(RATE, &mut data[..2 * n], 2, n);
        out.extend((0..n).map(|i| (data[i], data[n + i])));
        done += n;
    }
    out
}

fn clip(audio: &Arc<ClipAudio>, shape: ClipShape) -> Clip {
    Clip {
        envelope: Default::default(),
        id: 1,
        track: 0,
        start: Ticks::quarters(1), // sample 24,000 at 120 BPM
        offset: 1_000,
        length: 40_000,
        gain: 0.8,
        audio: Arc::clone(audio),
        shape,
    }
}

#[test]
fn a_streamed_clip_in_memory_plays_exactly_what_the_clip_in_memory_plays() {
    let frames = 30_000;
    let memory = in_memory(frames);
    let (stream, _, _writer) = streamed(frames, 0..frames.div_ceil(PAGE));
    let fade = |frames| FadeFrames {
        frames,
        span: frames,
        curve: FadeShape::SCurve.curve(),
        ..Default::default()
    };
    let shapes = [
        ClipShape::default(),
        // Fades, and a loop of 9,000 frames that wraps across pages.
        ClipShape {
            fade_in: fade(2_000),
            fade_out: fade(5_000),
            period: Some(9_000),
            phase: 300,
        },
    ];
    for shape in shapes {
        for block in [1, 64, 128, 333, 4_096] {
            let (mut handle, _, mut engine) = setup(vec![clip(&memory, shape)]);
            handle.send(Command::Play).unwrap();
            let want = run(&mut engine, 70_000, block);
            let (mut handle, _, mut engine) = setup(vec![clip(&stream, shape)]);
            handle.send(Command::Play).unwrap();
            let got = run(&mut engine, 70_000, block);
            assert!(want.iter().any(|s| s.0 != 0.0), "the clip is heard");
            assert!(got == want, "block {block}, {shape:?}");
        }
    }
    let ClipAudio::Stream(s) = &*stream else {
        unreachable!()
    };
    assert_eq!(s.pool().underruns(), 0);
}

#[test]
fn a_missing_page_fades_the_clip_out_before_it_and_counts_one_underrun() {
    let frames = 40 * PAGE;
    let memory = in_memory(frames);
    // Pages 0 to 4 are in memory; 5 on are not.
    let (stream, table, mut writer) = streamed(frames, 0..5);
    let plain = |audio: &Arc<ClipAudio>| Clip {
        start: Ticks::ZERO,
        offset: 0,
        length: frames as i64,
        gain: 1.0,
        ..clip(audio, ClipShape::default())
    };
    let (mut handle, _, mut reference) = setup(vec![plain(&memory)]);
    handle.send(Command::Play).unwrap();
    let want = run(&mut reference, 10 * PAGE, 128);
    let (mut handle, _, mut engine) = setup(vec![plain(&stream)]);
    handle.send(Command::Play).unwrap();
    let got = run(&mut engine, 10 * PAGE, 128);
    let missing = 5 * PAGE;
    // Plays exactly until the look-ahead (40 ms) and a block reach the gap.
    let exact = missing - RATE as usize / 25 - 128;
    assert!(got[..exact] == want[..exact]);
    // Glides out in the 40 ms before it: below what it was, then silent.
    let before = missing - 1;
    assert!(got[before].0.abs() < want[before].0.abs() / 10.0);
    assert!(got[missing..].iter().all(|&s| s == (0.0, 0.0)));
    assert_eq!(
        table.pool().underruns(),
        1,
        "one underrun, not one per block"
    );

    // The pages arrive: the clip glides back in, and plays exactly once it
    // settles.
    for page in 5..40 {
        load(&mut writer, &table, page);
    }
    let later = run(&mut engine, 4 * PAGE, 128);
    let settled = 10 * PAGE + RATE as usize / 50 + 128;
    let reference_later = run(&mut reference, 4 * PAGE, 128);
    assert!(later[0].0.abs() < reference_later[0].0.abs());
    let offset = settled - 10 * PAGE;
    assert!(later[offset..] == reference_later[offset..]);
    assert_eq!(table.pool().underruns(), 1);

    // A second gap is a second underrun.
    writer.tick();
    writer.drop_stream(&table);
    writer.reclaim();
    let gone = run(&mut engine, PAGE, 128);
    assert!(gone[PAGE - 1] == (0.0, 0.0));
    assert_eq!(table.pool().underruns(), 2);
}

#[test]
fn a_clip_whose_pages_are_missing_from_its_start_stays_silent() {
    let frames = 4 * PAGE;
    let (stream, table, _writer) = streamed(frames, 0..0);
    let (mut handle, _, mut engine) = setup(vec![Clip {
        start: Ticks::ZERO,
        offset: 0,
        length: frames as i64,
        ..clip(&stream, ClipShape::default())
    }]);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, frames, 256);
    assert!(out.iter().all(|&s| s == (0.0, 0.0)));
    assert_eq!(table.pool().underruns(), 1);
}

#[test]
fn a_looped_transport_counts_no_underrun_while_its_pages_are_there() {
    let frames = 20 * PAGE;
    let (stream, table, _writer) = streamed(frames, 0..20);
    let (mut handle, _, mut engine) = setup(vec![Clip {
        start: Ticks::ZERO,
        offset: 0,
        length: frames as i64,
        ..clip(&stream, ClipShape::default())
    }]);
    // A one-beat loop (24,000 frames), played round many times.
    handle
        .send(Command::SetLoop(Region::new(
            Ticks::ZERO,
            Ticks::quarters(1),
            true,
        )))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 200_000, 128);
    assert!(out.iter().any(|s| s.0 != 0.0));
    assert_eq!(table.pool().underruns(), 0);
}

#[test]
fn an_offline_render_loads_what_it_reads_and_equals_the_render_from_memory() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use lumen_engine::{ClipRender, Finished, OfflineLoader, render_clips};

    let frames = 30 * PAGE;
    let memory = in_memory(frames);
    // Eight pages for a file of thirty: the render evicts as it goes.
    let pool = PagePool::new(8);
    let loads = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&loads);
    let (left, right) = ramp(frames);
    let loader = OfflineLoader::new(
        &pool,
        Box::new(move |_, page, l, r| {
            counted.fetch_add(1, Ordering::Relaxed);
            let to = ((page + 1) * PAGE).min(frames);
            let n = to - page * PAGE;
            l[..n].copy_from_slice(&left[page * PAGE..to]);
            r[..n].copy_from_slice(&right[page * PAGE..to]);
            Some(n)
        }),
    );
    // The live stream it stands for: only its shape and id are taken.
    let live = StreamAudio::new(&PagePool::new(1), 1, RATE, 2, frames as u64);
    let stream = loader.stream(&live);
    assert!(stream.is_offline() && !live.is_offline());
    let streamed = Arc::new(ClipAudio::Stream(stream));
    let shape = ClipShape {
        period: Some(9 * PAGE as i64 + 77),
        ..ClipShape::default()
    };
    let render = |audio: &Arc<ClipAudio>| {
        let mut out = Vec::new();
        let ended = render_clips(
            ClipRender {
                clips: vec![Clip {
                    start: Ticks::ZERO,
                    offset: 500,
                    length: 25 * PAGE as i64,
                    ..clip(audio, shape)
                }],
                timeline: Timeline::default(),
                sample_rate: RATE,
                from: 0,
                frames: 26 * PAGE as u64,
                bare_ends: false,
                block: 256,
            },
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
            &mut |l, r| out.extend(l.iter().zip(r).map(|(a, b)| (*a, *b))),
        );
        assert_eq!(ended, Finished::Done);
        out
    };
    let want = render(&memory);
    let got = render(&streamed);
    assert!(want.iter().any(|s| s.0 != 0.0));
    assert!(
        got == want,
        "an offline render plays exactly what memory plays"
    );
    assert!(loads.load(Ordering::Relaxed) > 8);
    let ClipAudio::Stream(s) = &*streamed else {
        unreachable!()
    };
    assert_eq!(s.pool().underruns(), 0);
}
