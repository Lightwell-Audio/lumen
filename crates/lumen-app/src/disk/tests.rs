use std::sync::atomic::AtomicUsize;

use lumen_core::time::Ticks;
use lumen_engine::{
    Clip, ClipAudio, ClipShape, Command, Destination, NodeSpec, Region, Routing, Timeline,
    clip_player, engine, mixer,
};

use super::plan::{Horizon, Position, Scene, Want};
use super::*;

const RATE: u32 = 48_000;
const P: i64 = PAGE as i64;

/// The sample a fake file holds at `frame`.
fn value(id: u64, frame: usize) -> f32 {
    (id as f32) + frame as f32 / 1e7
}

/// Pages made up from their place, read after `delay`, counting reads.
struct Fake {
    delay: Duration,
    reads: Arc<AtomicUsize>,
    /// Pages of this file fail.
    broken: Option<u64>,
}

impl Pages for Fake {
    fn read(
        &mut self,
        audio: &StreamAudio,
        page: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        std::thread::sleep(self.delay);
        if self.broken == Some(audio.id()) {
            return Err(MediaError::Io("gone".to_owned()));
        }
        let from = page * PAGE;
        let frames = (audio.frames() as usize).saturating_sub(from).min(PAGE);
        for i in 0..frames {
            left[i] = value(audio.id(), from + i);
            right[i] = -value(audio.id(), from + i);
        }
        Ok(frames)
    }

    fn keep(&mut self, _: &[StreamAudio]) {}
}

fn stream(pool: &Arc<PagePool>, id: u64, pages: usize) -> StreamAudio {
    StreamAudio::new(pool, id, RATE, 2, (pages * PAGE) as u64)
}

/// A clip of `audio` on track 0 at timeline sample `start` (120 BPM: a
/// quarter is 24,000 samples), playing `length` frames from `offset`.
fn clip(audio: &StreamAudio, start: i64, offset: i64, length: i64) -> Clip {
    assert_eq!(start % 24_000, 0, "starts on a quarter");
    Clip {
        envelope: Default::default(),
        id: audio.id(),
        track: 0,
        start: Ticks::quarters(start / 24_000),
        offset,
        length,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Stream(audio.clone())),
        shape: ClipShape::default(),
    }
}

fn scene(clips: &[Clip], looped: Option<(i64, i64)>, points: Vec<i64>) -> Scene {
    Scene::new(clips, &Timeline::default(), RATE, looped, points)
}

fn horizon(near: i64, far: i64) -> Horizon {
    Horizon { near, far }
}

fn planned(scene: &Scene, at: Position, h: Horizon) -> Vec<Want> {
    let mut out = Vec::new();
    scene.plan(at, h, &mut out);
    out
}

fn at(sample: i64, playing: bool) -> Position {
    Position {
        sample,
        playing,
        cue: None,
        marker: None,
    }
}

#[test]
fn pages_are_planned_nearest_first_across_clips_and_each_once() {
    let pool = PagePool::new(1);
    let (a, b) = (stream(&pool, 1, 10), stream(&pool, 2, 10));
    // Two clips side by side, and one more of file 1 later.
    let clips = [
        clip(&a, 0, 0, 10 * P),
        clip(&b, 0, 5 * P, 5 * P),
        clip(&a, 240_000, 0, 10 * P),
    ];
    let scene = scene(&clips, None, vec![]);
    assert_eq!(scene.streams().len(), 2);
    let w = |stream, page| Want { stream, page };
    assert_eq!(
        planned(&scene, at(P / 2, true), horizon(2 * P, 2 * P)),
        [w(0, 0), w(0, 1), w(1, 5), w(1, 6), w(0, 2), w(1, 7)]
    );
}

#[test]
fn memory_clips_and_clips_at_another_rate_are_not_planned() {
    let pool = PagePool::new(1);
    let other = StreamAudio::new(&pool, 3, 44_100, 2, 100_000);
    let memory = Clip {
        audio: Arc::new(ClipAudio::Memory {
            sample_rate: RATE,
            channels: vec![vec![0.0; 1_000]],
        }),
        ..clip(&stream(&pool, 1, 1), 0, 0, 1_000)
    };
    let scene = scene(&[memory, clip(&other, 0, 0, 1_000)], None, vec![]);
    assert!(scene.is_empty());
    assert!(planned(&scene, at(0, true), Horizon::at(RATE)).is_empty());
    assert!(scene.ready(0, 1_000));
}

#[test]
fn a_playing_loop_reads_on_from_its_start_and_a_stopped_one_does_not() {
    let pool = PagePool::new(1);
    let a = stream(&pool, 1, 40);
    let scene = scene(&[clip(&a, 0, 0, 40 * P)], Some((0, 4 * P)), vec![]);
    let pages = |w: Vec<Want>| w.into_iter().map(|w| w.page).collect::<Vec<_>>();
    // Half a page before the loop's end: the end, then the loop's start.
    assert_eq!(
        pages(planned(
            &scene,
            at(4 * P - P / 2, true),
            horizon(2 * P, 2 * P)
        )),
        [3, 0, 1]
    );
    // Stopped, the transport does not wrap.
    assert_eq!(
        pages(planned(
            &scene,
            at(4 * P - P / 2, false),
            horizon(2 * P, 2 * P)
        )),
        [3, 4, 5]
    );
    // A read-ahead longer than the loop reads the loop once.
    assert_eq!(
        pages(planned(&scene, at(0, true), horizon(10 * P, 10 * P))),
        [0, 1, 2, 3]
    );
}

#[test]
fn the_cue_comes_first_then_the_playhead_and_marker_then_the_rest_then_the_points() {
    let pool = PagePool::new(1);
    let a = stream(&pool, 1, 100);
    let scene = scene(&[clip(&a, 0, 0, 100 * P)], None, vec![50 * P]);
    let at = Position {
        sample: 0,
        playing: true,
        cue: Some(80 * P),
        marker: Some(30 * P),
    };
    let pages: Vec<usize> = planned(&scene, at, horizon(P, 3 * P))
        .into_iter()
        .map(|w| w.page)
        .collect();
    // What plays on comes before the places playback might jump to.
    assert_eq!(pages, [80, 0, 30, 1, 2, 50]);
}

#[test]
fn many_points_never_cut_the_read_ahead_at_the_playhead() {
    // Room for the read-ahead (24 pages) and four points of six pages, and
    // twenty points wanted: the read-ahead is read whole and first.
    let pool = PagePool::new(48);
    let a = stream(&pool, 1, 2_000);
    let clips = vec![clip(&a, 0, 0, 2_000 * P)];
    let (handle, _) = player(clips.clone());
    let (pages, _) = fake(Duration::ZERO);
    let disk = Disk::start(&pool, pages, handle.transport_reader());
    let points = (1..=20).map(|i| i * 90 * P).collect();
    disk.set_scene(scene(&clips, None, points));
    let ahead = (2 * RATE as usize).div_ceil(PAGE);
    eventually("the read-ahead is read", || {
        (0..ahead).all(|p| a.resident(p))
    });
    eventually("the pool fills", || a.resident(90) && a.resident(180));
    std::thread::sleep(Duration::from_millis(50));
    assert!((0..ahead).all(|p| a.resident(p)), "the read-ahead stays");
    drop(disk);
}

#[test]
fn ready_asks_for_every_page_the_span_plays() {
    let pool = PagePool::new(4);
    let mut writer = PoolWriter::new(&pool);
    let a = stream(&pool, 1, 10);
    let scene = scene(&[clip(&a, 0, 0, 10 * P)], None, vec![]);
    assert!(!scene.ready(0, P + 1));
    writer.load(&a, 0, &[0.0; PAGE], &[0.0; PAGE]);
    assert!(scene.ready(0, P));
    assert!(!scene.ready(0, P + 1));
    writer.load(&a, 1, &[0.0; PAGE], &[0.0; PAGE]);
    assert!(scene.ready(0, P + 1));
    // Outside every clip there is nothing to wait for.
    assert!(scene.ready(20 * P, P));
}

/// An engine playing `clips` from the pool, at 120 BPM.
fn player(clips: Vec<Clip>) -> (lumen_engine::EngineHandle, impl FnMut(usize)) {
    let (mut clips_handle, source) = clip_player();
    clips_handle.set(clips).unwrap();
    let (mut routing, source) = mixer(source);
    assert!(
        routing
            .set(&Routing {
                nodes: vec![NodeSpec::unity(0, Destination::Hardware(0))],
                cue: None,
                measure: None,
            })
            .unwrap()
    );
    let (handle, mut engine) = engine(Timeline::default(), source, RATE);
    let mut data = vec![0.0; 2 * 128];
    let process = move |blocks: usize| {
        for _ in 0..blocks {
            engine.process(RATE, &mut data, 2, 128);
        }
        // Kept alive with the engine.
        let _ = (&clips_handle, &routing);
    };
    (handle, process)
}

fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let started = Instant::now();
    while !check() {
        assert!(started.elapsed() < Duration::from_secs(20), "{what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn fake(delay: Duration) -> (Box<Fake>, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let fake = Fake {
        delay,
        reads: Arc::clone(&reads),
        broken: None,
    };
    (Box::new(fake), reads)
}

#[test]
fn the_thread_reads_ahead_of_the_playhead_and_the_samples_are_the_files() {
    let pool = PagePool::new(256);
    let a = stream(&pool, 7, 100);
    let clips = vec![clip(&a, 0, 0, 100 * P)];
    let (handle, _) = player(clips.clone());
    let (pages, reads) = fake(Duration::ZERO);
    let disk = Disk::start(&pool, pages, handle.transport_reader());
    let scene = scene(&clips, None, vec![]);
    disk.set_scene(scene);
    let check = Scene::new(&clips, &Timeline::default(), RATE, None, vec![]);
    eventually("two seconds are read", || {
        check.ready(0, 2 * i64::from(RATE))
    });
    let side = a.side(1);
    assert_eq!(side.at(12_345), Some(-value(7, 12_345)));
    // Nothing more than the read-ahead: two seconds, whole pages.
    std::thread::sleep(Duration::from_millis(30));
    let read = reads.load(Ordering::Relaxed);
    assert_eq!(read, (2 * RATE as usize).div_ceil(PAGE));
    assert!(!a.resident(read));
    drop(disk);
}

#[test]
fn a_cue_is_read_before_the_playhead() {
    let pool = PagePool::new(256);
    let a = stream(&pool, 1, 1_000);
    let clips = vec![clip(&a, 0, 0, 1_000 * P)];
    let (handle, _) = player(clips.clone());
    // Slow enough that the order shows.
    let (pages, _) = fake(Duration::from_millis(2));
    let disk = Disk::start(&pool, pages, handle.transport_reader());
    disk.cue(Some(500 * P));
    disk.set_scene(scene(&clips, None, vec![]));
    eventually("the cue is read", || a.resident(500));
    assert!(!a.resident(20), "the far read-ahead at the playhead waits");
}

#[test]
fn slow_reads_cost_underruns_and_never_hold_the_audio_up() {
    let pool = PagePool::new(512);
    let a = stream(&pool, 1, 400);
    let clips = vec![clip(&a, 0, 0, 400 * P)];
    let (mut handle, mut process) = player(clips.clone());
    // 30 ms for a page of 85 ms: one file keeps up in real time, but not
    // rendered as fast as the engine can.
    let (pages, _) = fake(Duration::from_millis(30));
    let disk = Disk::start(&pool, pages, handle.transport_reader());
    disk.set_scene(scene(&clips, None, vec![]));
    handle.send(Command::Play).unwrap();
    let started = Instant::now();
    // Ten seconds of audio as fast as it renders.
    process(10 * RATE as usize / 128);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the audio thread never waited for the disk"
    );
    assert!(pool.underruns() >= 1);
    drop(disk);
}

#[test]
fn a_pool_too_small_for_the_plan_holds_the_nearest_pages() {
    let pool = PagePool::new(8);
    let a = stream(&pool, 1, 100);
    let clips = vec![clip(&a, 0, 0, 100 * P)];
    let (handle, _) = player(clips.clone());
    let (pages, reads) = fake(Duration::ZERO);
    let disk = Disk::start(&pool, pages, handle.transport_reader());
    disk.set_scene(scene(&clips, None, vec![]));
    eventually("the pool fills", || (0..8).all(|p| a.resident(p)));
    std::thread::sleep(Duration::from_millis(50));
    assert!(!a.resident(8));
    assert_eq!(reads.load(Ordering::Relaxed), 8, "no page read twice");
}

#[test]
fn the_pool_follows_a_locate() {
    let pool = PagePool::new(32);
    let a = stream(&pool, 1, 1_000);
    let clips = vec![clip(&a, 0, 0, 1_000 * P)];
    let (mut handle, mut process) = player(clips.clone());
    let (pages, _) = fake(Duration::ZERO);
    let disk = Disk::start(&pool, pages, handle.transport_reader());
    disk.set_scene(scene(&clips, None, vec![]));
    // All two seconds of it, 24 of the 32 pages, so the new place needs room
    // (under a loaded test run, page 0 alone came in first).
    let ahead = (2 * RATE as usize).div_ceil(PAGE);
    eventually("the start is read", || (0..ahead).all(|p| a.resident(p)));
    // 20 quarters: 480,000 samples, page 117.
    handle.send(Command::Locate(Ticks::quarters(20))).unwrap();
    process(1);
    eventually("the new place is read", || a.resident(117));
    eventually("the old one makes room", || !a.resident(0));
    drop(disk);
}

#[test]
fn a_page_that_cannot_be_read_is_counted_once() {
    let pool = PagePool::new(64);
    let a = stream(&pool, 9, 10);
    let clips = vec![clip(&a, 0, 0, 10 * P)];
    let (handle, _) = player(clips.clone());
    let reads = Arc::new(AtomicUsize::new(0));
    let pages = Box::new(Fake {
        delay: Duration::ZERO,
        reads: Arc::clone(&reads),
        broken: Some(9),
    });
    let disk = Disk::start(&pool, pages, handle.transport_reader());
    disk.set_scene(scene(&clips, None, vec![]));
    eventually("every page was tried", || disk.stats().failed() == 10);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(reads.load(Ordering::Relaxed), 10, "not tried again");
}

#[test]
fn a_loop_and_its_points_stay_in_memory_while_playing_elsewhere() {
    // Room for the points and the read-ahead, not for what was played.
    let pool = PagePool::new(48);
    let a = stream(&pool, 1, 1_000);
    let clips = vec![clip(&a, 0, 0, 1_000 * P)];
    let (mut handle, mut process) = player(clips.clone());
    let (pages, _) = fake(Duration::ZERO);
    let disk = Disk::start(&pool, pages, handle.transport_reader());
    // A loop from quarter 40 (960,000 samples, page 234) to 44, and a marker
    // at page 500 (a point).
    handle
        .send(Command::SetLoop(Region::new(
            Ticks::quarters(40),
            Ticks::quarters(44),
            true,
        )))
        .unwrap();
    disk.set_scene(scene(
        &clips,
        Some((960_000, 1_056_000)),
        vec![960_000, 500 * P],
    ));
    process(1);
    eventually("the loop start and the marker are read", || {
        a.resident(234) && a.resident(500)
    });
    handle.send(Command::Play).unwrap();
    process(4 * RATE as usize / 128);
    std::thread::sleep(Duration::from_millis(50));
    assert!(a.resident(234) && a.resident(500));
    drop(disk);
}

#[test]
fn readers_read_pages_of_stream_files_by_their_id() {
    use lumen_media::wav::{Sample, Spec, Writer};

    let path = std::env::temp_dir().join(format!("lumen-disk-files-{}.wav", std::process::id()));
    let frames = PAGE + 100;
    let left: Vec<f32> = (0..frames).map(|i| value(4, i)).collect();
    let right: Vec<f32> = left.iter().map(|v| -v).collect();
    let spec = Spec {
        sample_rate: RATE,
        channels: 2,
        sample: Sample::F32,
    };
    let mut writer = Writer::create(&path, spec).unwrap();
    writer.write(&[&left, &right]).unwrap();
    writer.finish().unwrap();

    let pool = PagePool::new(1);
    let unknown = StreamAudio::new(&pool, u64::MAX, RATE, 2, frames as u64);
    let mut files = Readers::default();
    let (mut l, mut r) = (vec![0.0; PAGE], vec![0.0; PAGE]);
    assert!(
        Pages::read(&mut files, &unknown, 0, &mut l, &mut r).is_err(),
        "not registered"
    );
    let id = crate::media::streams::register(path.clone());
    let audio = StreamAudio::new(&pool, id, RATE, 2, frames as u64);
    assert_eq!(
        Pages::read(&mut files, &audio, 1, &mut l, &mut r).unwrap(),
        100
    );
    assert_eq!((l[99], r[99]), (left[PAGE + 99], right[PAGE + 99]));
    assert_eq!(l[100], 0.0, "past the end is silence");
    // Readers of files the scene no longer has are closed.
    Pages::keep(&mut files, &[]);
    crate::media::streams::forget(id);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn the_thread_stops_while_a_cuer_is_still_held() {
    let pool = PagePool::new(4);
    let (handle, _) = player(vec![]);
    let (pages, _) = fake(Duration::ZERO);
    let disk = Disk::start(&pool, pages, handle.transport_reader());
    let cuer = disk.cuer();
    // Dropping joins the thread: it must end although the cuer keeps the
    // channel open (the transport holds one).
    drop(disk);
    cuer.cue(Some(0));
}
