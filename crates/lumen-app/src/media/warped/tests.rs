use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use lumen_core::time::{TempoMap, TempoPoint, Ticks};
use lumen_core::warp::Marker;
use lumen_engine::{
    ClipAudio, Command, Destination, NodeSpec, PAGE, PagePool, Routing, StreamAudio, Timeline,
    clip_player, engine, mixer,
};
use lumen_project::model::{Id, Project, Warp};

use super::{Request, WarpedStreams};
use crate::document::{Document, DropInto, Imported};
use crate::lanes::engine_clips_with;
use crate::media::streams::{self, Readers, Unread};

const RATE: u32 = 48_000;
/// Ten seconds: long enough for the tests' clips, short enough to stay fast.
const FRAMES: usize = 480_000;
/// The file's tone, in cycles per frame (about 960 Hz).
const TONE: f64 = 0.02;

fn tone(speed: f64, from: f64, k: usize) -> f32 {
    (std::f64::consts::TAU * TONE * (from + k as f64 * speed)).sin() as f32
}

fn file() -> Arc<ClipAudio> {
    let left: Vec<f32> = (0..FRAMES).map(|i| tone(1.0, 0.0, i)).collect();
    let right: Vec<f32> = left.iter().map(|v| v * 0.5).collect();
    Arc::new(ClipAudio::Memory {
        sample_rate: RATE,
        channels: vec![left, right],
    })
}

fn constant(bpm: f64) -> TempoMap {
    TempoMap::constant(bpm).unwrap()
}

fn request<'a>(
    warp: &'a Warp,
    source: &'a Arc<ClipAudio>,
    tempo: &'a TempoMap,
    frames: i64,
) -> Request<'a> {
    Request {
        warp,
        source,
        tempo,
        rate: RATE,
        origin: Ticks::ZERO,
        reach: frames,
    }
}

/// The frames `from..from + n` of a warped stream, read as the disk thread
/// does: one page at a time on a fresh reader.
fn read(audio: &ClipAudio, from: usize, n: usize) -> Vec<f32> {
    let stream = audio.stream().expect("a warped clip plays as a stream");
    let mut readers = Readers::default();
    let mut out = Vec::new();
    let (mut left, mut right) = (vec![0.0; PAGE], vec![0.0; PAGE]);
    let mut at = from;
    while at < from + n {
        let page = at / PAGE;
        let got = readers
            .read(stream, (page * PAGE) as u64, &mut left, &mut right)
            .unwrap();
        let wanted = n - out.len();
        out.extend(left[at % PAGE..got.min(PAGE)].iter().take(wanted));
        at = (page + 1) * PAGE;
    }
    out
}

#[test]
fn a_clip_that_plays_as_it_was_made_is_the_file_bit_for_bit() {
    let source = file();
    let tempo = constant(120.0);
    let warp = Warp::new(120.0, 0.0);
    let mut warps = WarpedStreams::new(&PagePool::new(1));
    let stream = warps
        .stream(&request(&warp, &source, &tempo, 96_000))
        .unwrap();
    let file = source.memory().unwrap();
    assert_eq!(read(&stream, 0, 96_000), file[0][..96_000]);
    // A page in the middle, read alone, is the same.
    assert_eq!(
        read(&stream, 5 * PAGE + 7, 3_000),
        file[0][5 * PAGE + 7..][..3_000]
    );
}

#[test]
fn a_file_made_at_a_slower_tempo_runs_faster_in_the_song() {
    // Made at 100 BPM, played in a 120 BPM song: 1.2 times the speed.
    let source = file();
    let tempo = constant(120.0);
    let warp = Warp::new(100.0, 0.0);
    let mut warps = WarpedStreams::new(&PagePool::new(1));
    let stream = warps
        .stream(&request(&warp, &source, &tempo, 192_000))
        .unwrap();
    let played = read(&stream, 0, 20_000);
    for (k, v) in played.iter().enumerate().skip(100) {
        let want = tone(1.2, 0.0, k);
        assert!((v - want).abs() < 2.0e-3, "frame {k}: {v} against {want}");
    }
}

#[test]
fn a_pages_frames_do_not_depend_on_what_was_read_before() {
    let source = file();
    let tempo = TempoMap::new(vec![
        TempoPoint::step(Ticks::ZERO, 120.0),
        TempoPoint {
            at: Ticks::quarters(4),
            bpm: 90.0,
            ramp: true,
        },
        TempoPoint::step(Ticks::quarters(12), 140.0),
    ])
    .unwrap();
    let mut warp = Warp::new(110.0, 0.5);
    warp.markers.push(Marker::new(3.0, Ticks::quarters(6)));
    warp.markers.push(Marker::new(4.4, Ticks::quarters(9)));
    let mut warps = WarpedStreams::new(&PagePool::new(1));
    let stream = warps
        .stream(&request(&warp, &source, &tempo, 400_000))
        .unwrap();
    let id = stream.stream().unwrap().id();
    // The whole run, page after page, then each page alone, backwards, on
    // readers that have read nothing.
    let whole = read(&stream, 0, 12 * PAGE);
    assert!(whole.iter().any(|v| v.abs() > 0.5));
    for page in (0..12).rev() {
        let alone = read(&stream, page * PAGE, PAGE);
        assert_eq!(alone, whole[page * PAGE..(page + 1) * PAGE], "page {page}");
    }
    // The ids are the registry's: forgetting one ends it.
    assert!(streams::warped(id).is_some());
    drop(warps);
    assert!(streams::warped(id).is_none());
}

#[test]
fn the_position_follows_the_map_through_a_ramp() {
    let source = file();
    let tempo = TempoMap::new(vec![
        TempoPoint {
            at: Ticks::ZERO,
            bpm: 80.0,
            ramp: true,
        },
        TempoPoint::step(Ticks::quarters(8), 160.0),
    ])
    .unwrap();
    let mut warp = Warp::new(120.0, 0.25);
    warp.markers.push(Marker::new(2.0, Ticks::quarters(4)));
    let mut warps = WarpedStreams::new(&PagePool::new(1));
    let stream = warps
        .stream(&request(&warp, &source, &tempo, 300_000))
        .unwrap();
    let warped = streams::warped(stream.stream().unwrap().id()).unwrap();
    let mut last = f64::MIN;
    for k in (0..250_000).step_by(997) {
        let p = warped.position(k);
        assert!(p > last, "the file runs forward at frame {k}");
        last = p;
    }
    // At a tick, the position is the map's second there, in frames.
    for quarter in [0, 2, 4, 6] {
        let at = Ticks::quarters(quarter);
        let k = tempo.sample_at(at, RATE);
        let second = lumen_core::warp::source_at(&warp.markers, 120.0, at);
        let got = warped.position(k) / f64::from(RATE);
        assert!(
            (got - second).abs() < 1.0e-4,
            "beat {quarter}: {got} against {second}"
        );
    }
}

#[test]
fn an_unchanged_request_finds_its_stream_and_a_changed_one_makes_another() {
    let source = file();
    let tempo = constant(120.0);
    let warp = Warp::new(100.0, 0.0);
    let mut warps = WarpedStreams::new(&PagePool::new(1));
    let a = warps
        .stream(&request(&warp, &source, &tempo, 96_000))
        .unwrap();
    let again = warps
        .stream(&request(&warp, &source, &tempo, 96_000))
        .unwrap();
    assert!(Arc::ptr_eq(&a, &again), "the prepared pages are kept");
    assert_eq!(warps.len(), 1);

    let mut moved = warp.clone();
    moved.markers.push(Marker::new(2.0, Ticks::quarters(3)));
    let b = warps
        .stream(&request(&moved, &source, &tempo, 96_000))
        .unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
    let faster = constant(121.0);
    let c = warps
        .stream(&request(&warp, &source, &faster, 96_000))
        .unwrap();
    assert!(!Arc::ptr_eq(&a, &c));
    assert_eq!(warps.len(), 3);

    // Only what the engine plays is kept.
    let (ia, ic) = (a.stream().unwrap().id(), c.stream().unwrap().id());
    warps.retain(&HashSet::from([ic]));
    assert_eq!(warps.len(), 1);
    assert!(streams::warped(ia).is_none());
    assert!(streams::warped(ic).is_some());
}

#[test]
fn a_file_at_another_rate_or_an_empty_stream_is_not_warped() {
    let source = file();
    let tempo = constant(120.0);
    let warp = Warp::new(120.0, 0.0);
    let mut warps = WarpedStreams::new(&PagePool::new(1));
    assert!(warps.stream(&request(&warp, &source, &tempo, 0)).is_none());
    let mut other = request(&warp, &source, &tempo, 1_000);
    other.rate = 44_100;
    assert!(
        warps.stream(&other).is_none(),
        "the device is at another rate"
    );
    assert_eq!(warps.len(), 0);
}

#[test]
fn a_stream_file_is_warped_from_disk() {
    use lumen_media::wav::{Sample, Spec, Writer};
    let dir = lumen_project::testing::TempDir::new("warped-from-disk");
    let path = dir.path().join("tone.wav");
    let data: Vec<f32> = (0..FRAMES).map(|i| tone(1.0, 0.0, i)).collect();
    let mut writer = Writer::create(
        &path,
        Spec {
            sample_rate: RATE,
            channels: 1,
            sample: Sample::F32,
        },
    )
    .unwrap();
    writer.write(&[&data]).unwrap();
    writer.finish().unwrap();
    let file_id = streams::register(path);
    let pool = PagePool::new(1);
    let stream = StreamAudio::new(&pool, file_id, RATE, 1, FRAMES as u64);
    let source = Arc::new(ClipAudio::Stream(stream));

    let tempo = constant(120.0);
    let warp = Warp::new(150.0, 0.0);
    let mut warps = WarpedStreams::new(&pool);
    let warped = warps
        .stream(&request(&warp, &source, &tempo, 192_000))
        .unwrap();
    let played = read(&warped, 0, 9_000);
    for (k, v) in played.iter().enumerate().skip(100) {
        let want = tone(0.8, 0.0, k);
        assert!((v - want).abs() < 2.0e-3, "frame {k}");
    }
    streams::forget(file_id);
}

/// A project with one clip of the file, two seconds at 120 BPM, warped from a
/// file made at `made_at` BPM, and the engine's render of it offline.
fn project(made_at: f64, tempo: Vec<TempoPoint>, looped: bool) -> Document {
    project_at(made_at, tempo, looped, Ticks::ZERO)
}

/// [`project`] with the clip starting at `start`.
fn project_at(made_at: f64, tempo: Vec<TempoPoint>, looped: bool, start: Ticks) -> Document {
    let mut doc = Document::new(crate::empty_project());
    doc.import(vec![Imported {
        media: Id(1000),
        path: PathBuf::from("C:/a/a.wav"),
        name: "A".into(),
        sample_rate: RATE,
        channels: 2,
        frames: FRAMES as u64,
        size_bytes: 0,
        at: start,
        into: DropInto::NewTracks {
            before: None,
            group: None,
        },
    }]);
    doc.apply("prepare", None, |project| {
        project.tempo = tempo;
        let clip = Arc::make_mut(&mut Arc::make_mut(&mut project.tracks[0]).clips[0]);
        clip.length = Ticks::quarters(4);
        if looped {
            clip.loop_length = Some(Ticks::quarters(1));
        }
    });
    let id = doc.project().tracks[0].clips[0].id;
    doc.warp_clips(&[id], made_at);
    doc
}

fn render(project: &Project, frames: usize, block: usize, warped: bool) -> Vec<f32> {
    render_with(&file(), project, frames, block, warped)
}

fn render_with(
    source: &Arc<ClipAudio>,
    project: &Project,
    frames: usize,
    block: usize,
    warped: bool,
) -> Vec<f32> {
    let timeline = Timeline::new(
        TempoMap::new(project.tempo.clone()).unwrap(),
        project.meter_map(),
    );
    let mut warps = WarpedStreams::new(&PagePool::new(1));
    let list = engine_clips_with(
        project,
        RATE,
        |_| Some((Arc::clone(source), RATE)),
        &mut |request| warped.then(|| warps.stream(request)).flatten(),
    );
    // As an export plays them: the streams load their own pages.
    let list = streams::offline(list, &Unread::default());
    let (mut clips, player) = clip_player();
    clips.set(list).unwrap();
    let (mut routing, source) = mixer(player);
    routing
        .set(&Routing {
            nodes: vec![NodeSpec::unity(
                project.tracks[0].id.0,
                Destination::Hardware(0),
            )],
            cue: None,
            measure: None,
        })
        .unwrap();
    let (mut handle, mut engine) = engine(timeline, source, RATE);
    handle.send(Command::Play).unwrap();
    let mut data = vec![0.0; block * 2];
    let mut out = Vec::new();
    for _ in 0..frames.div_ceil(block) {
        engine.process(RATE, &mut data, 2, block);
        // Channel-major: the left channel is the first `block` samples.
        out.extend_from_slice(&data[..block]);
    }
    out.truncate(frames);
    out
}

fn step(bpm: f64) -> Vec<TempoPoint> {
    vec![TempoPoint::step(Ticks::ZERO, bpm)]
}

#[test]
fn a_warped_clip_at_its_own_tempo_renders_as_an_unwarped_one_does() {
    let doc = project(120.0, step(120.0), false);
    let plain = render(doc.project(), 96_000, 256, false);
    let warped = render(doc.project(), 96_000, 256, true);
    assert!(plain.iter().any(|v| v.abs() > 0.5));
    assert_eq!(warped, plain, "the same bits");
}

#[test]
fn a_warped_clip_is_heard_at_the_speed_its_map_gives_and_whatever_the_block() {
    let doc = project(100.0, step(120.0), false);
    let a = render(doc.project(), 96_000, 128, true);
    let b = render(doc.project(), 96_000, 1_000, true);
    assert_eq!(a, b, "offline equals real time at any block size");
    // 1.2 times the speed against the plain render, away from the clip's
    // edge fades.
    for (k, v) in a.iter().enumerate().skip(2_000).take(90_000) {
        let want = tone(1.2, 0.0, k);
        assert!((v - want).abs() < 3.0e-3, "frame {k}: {v} against {want}");
    }
}

#[test]
fn a_looped_warped_clip_repeats_its_period() {
    let doc = project(100.0, step(120.0), true);
    let out = render(doc.project(), 96_000, 256, true);
    // One quarter at 120 BPM is 24,000 frames.
    for k in 4_000..20_000 {
        assert_eq!(out[k], out[k + 24_000], "frame {k}");
    }
}

#[test]
fn every_repeat_of_a_loop_follows_the_tempo() {
    // 120 BPM, then 60 BPM from the third beat. A one-beat loop of a file made
    // at 120 BPM plays the same beat of the file each time: at its own speed
    // while the song is at 120, and at half speed (and twice as long, 48,000
    // frames) once it is at 60.
    let tempo = vec![
        TempoPoint::step(Ticks::ZERO, 120.0),
        TempoPoint::step(Ticks::quarters(2), 60.0),
    ];
    let doc = project(120.0, tempo, true);
    let out = render(doc.project(), 190_000, 256, true);
    for beat in 0..2 {
        let at = beat * 24_000;
        for j in 2_000..22_000 {
            let want = tone(1.0, 0.0, j);
            assert!(
                (out[at + j] - want).abs() < 3.0e-3,
                "beat {beat}, frame {j}"
            );
        }
    }
    for beat in 0..2 {
        let at = 48_000 + beat * 48_000;
        for j in 2_000..46_000 {
            let want = tone(0.5, 0.0, j);
            assert!(
                (out[at + j] - want).abs() < 3.0e-3,
                "beat {}, frame {j}",
                beat + 2
            );
        }
    }
}

#[test]
fn a_clip_off_the_sample_grid_at_its_own_tempo_is_the_file_bit_for_bit() {
    // At 123 BPM a beat is 23,414.6 frames, so a clip at the second beat
    // starts between two samples; it still plays the frames a plain clip does.
    let doc = project_at(123.0, step(123.0), false, Ticks::quarters(1));
    let plain = render(doc.project(), 120_000, 256, false);
    let warped = render(doc.project(), 120_000, 256, true);
    assert!(plain.iter().any(|v| v.abs() > 0.5));
    assert_eq!(warped, plain);
}

#[test]
fn every_repeat_of_a_loop_off_the_sample_grid_is_the_file_bit_for_bit_away_from_its_joins() {
    // At 123 BPM the repeats begin between samples. Each is its own clip with
    // its own anchor, so the 2nd, 3rd and 4th are as exact as the 1st; only
    // the 5 ms joins differ from the engine's own loop.
    let doc = project(123.0, step(123.0), true);
    let plain = render(doc.project(), 96_000, 256, false);
    let warped = render(doc.project(), 96_000, 256, true);
    assert!(plain.iter().any(|v| v.abs() > 0.5));
    let beat = 60.0 / 123.0 * f64::from(RATE);
    for n in 0..3 {
        let from = (beat * f64::from(n)).round() as usize + 300;
        let to = (beat * f64::from(n + 1)).round() as usize - 300;
        assert_eq!(warped[from..to], plain[from..to], "repeat {n}");
    }
}

fn constant_file(value: f32) -> Arc<ClipAudio> {
    Arc::new(ClipAudio::Memory {
        sample_rate: RATE,
        channels: vec![vec![value; FRAMES]; 2],
    })
}

#[test]
fn the_pieces_of_a_split_warped_clip_share_a_stream_and_play_on_without_a_dip() {
    let mut doc = project_at(110.0, step(120.0), false, Ticks::quarters(1));
    let id = doc.project().tracks[0].clips[0].id;
    doc.split_clips(&[id], &[Ticks::quarters(3)]);
    let source = constant_file(0.5);
    let mut warps = WarpedStreams::new(&PagePool::new(1));
    let list = engine_clips_with(
        doc.project(),
        RATE,
        |_| Some((Arc::clone(&source), RATE)),
        &mut |request| warps.stream(request),
    );
    assert_eq!(list.len(), 2);
    assert!(Arc::ptr_eq(&list[0].audio, &list[1].audio), "one stream");
    assert_eq!(list[0].offset + list[0].length, list[1].offset);
    assert_eq!(warps.len(), 1);

    let out = render_with(&source, doc.project(), 120_000, 256, true);
    // The clips run from the second beat to the sixth: frames 24,000 to
    // 120,000. Away from the outer fades, nothing dips at the cut (72,000).
    for (k, v) in out.iter().enumerate().take(118_000).skip(26_000) {
        assert!((v - 0.5).abs() < 1.0e-4, "frame {k}: {v}");
    }
}

#[test]
fn a_real_time_export_reads_each_stream_once_and_only_what_its_clips_play() {
    let mut doc = project_at(110.0, step(120.0), false, Ticks::quarters(1));
    let id = doc.project().tracks[0].clips[0].id;
    doc.split_clips(&[id], &[Ticks::quarters(3)]);
    let source = file();
    let mut warps = WarpedStreams::new(&PagePool::new(1));
    let list = engine_clips_with(
        doc.project(),
        RATE,
        |_| Some((Arc::clone(&source), RATE)),
        &mut |request| warps.stream(request),
    );
    let (first, second) = (list[0].clone(), list[1].clone());
    let held = streams::in_memory(list).unwrap();
    assert!(Arc::ptr_eq(&held[0].audio, &held[1].audio), "read once");
    let frames = held[0].audio.frames() as i64;
    assert_eq!(
        frames,
        first.length + second.length,
        "only the frames the clips play, not the song before them"
    );
    assert_eq!(held[0].offset, 0);
    assert_eq!(held[1].offset, first.length);
    // What was read is what the stream renders.
    let live = first.audio.stream().unwrap();
    let again = read(
        &ClipAudio::Stream(live.clone()),
        first.offset as usize,
        3_000,
    );
    assert_eq!(held[0].audio.memory().unwrap()[0][..3_000], again);
}

#[test]
fn a_warped_clip_follows_a_tempo_change() {
    // 120 BPM, then 60 BPM from the second beat: made at 120, the clip plays
    // as made at first and at half speed after.
    let tempo = vec![
        TempoPoint::step(Ticks::ZERO, 120.0),
        TempoPoint::step(Ticks::quarters(2), 60.0),
    ];
    let doc = project(120.0, tempo, false);
    let out = render(doc.project(), 120_000, 256, true);
    for (k, v) in out.iter().enumerate().take(44_000).skip(4_000) {
        assert!((v - tone(1.0, 0.0, k)).abs() < 3.0e-3, "before, frame {k}");
    }
    // The change is at frame 48,000; the file has played 48,000 frames.
    for (k, v) in out.iter().enumerate().take(110_000).skip(52_000) {
        let want = tone(0.5, 48_000.0, k - 48_000);
        assert!((v - want).abs() < 3.0e-3, "after, frame {k}");
    }
}

/// A clip list holding the one clip that plays `stream`, from the start.
fn clip_of(stream: &Arc<ClipAudio>, frames: i64) -> lumen_engine::Clip {
    lumen_engine::Clip {
        id: 7,
        track: 1,
        start: Ticks::ZERO,
        offset: 0,
        length: frames,
        gain: 1.0,
        audio: Arc::clone(stream),
        shape: Default::default(),
        envelope: Default::default(),
    }
}

#[test]
fn an_edit_made_while_playing_waits_for_the_audio_it_plays() {
    use lumen_engine::PoolWriter;
    let pool = PagePool::new(16);
    let source = file();
    let tempo = constant(120.0);
    let warp = Warp::new(100.0, 0.0);
    let mut warps = WarpedStreams::new(&pool);
    let stream = warps
        .stream(&request(&warp, &source, &tempo, 96_000))
        .unwrap();
    let list = vec![clip_of(&stream, 96_000)];
    let project = crate::empty_project();
    let mut lanes = crate::lanes::Lanes::default();

    // Stopped, nothing waits: the transport's own start waits for the audio.
    assert!(!lanes.hold_for_warps(&list, &project, RATE));
    // Playing, with nothing of the new stream in memory, the list waits and
    // the disk thread is told what to prepare.
    lanes.set_playhead(Some(0));
    assert!(lanes.hold_for_warps(&list, &project, RATE));
    assert_eq!(lanes.streamed().1.len(), 1, "the disk thread plans it");

    // Its next quarter second (12,000 frames: three pages) in memory: it plays.
    let mut writer = PoolWriter::new(&pool);
    let live = stream.stream().unwrap();
    let (left, right) = (vec![0.0; PAGE], vec![0.0; PAGE]);
    for page in 0..2 {
        assert!(writer.load(live, page, &left, &right));
    }
    assert!(
        lanes.hold_for_warps(&list, &project, RATE),
        "one page short"
    );
    assert!(writer.load(live, 2, &left, &right));
    assert!(!lanes.hold_for_warps(&list, &project, RATE));
}

#[test]
fn a_list_is_not_held_for_longer_than_the_limit() {
    let pool = PagePool::new(4);
    let source = file();
    let tempo = constant(120.0);
    let warp = Warp::new(100.0, 0.0);
    let mut warps = WarpedStreams::new(&pool);
    let stream = warps
        .stream(&request(&warp, &source, &tempo, 96_000))
        .unwrap();
    let list = vec![clip_of(&stream, 96_000)];
    let project = crate::empty_project();
    let mut lanes = crate::lanes::Lanes::default();
    lanes.set_playhead(Some(0));
    assert!(lanes.hold_for_warps(&list, &project, RATE));
    lanes.age_hold(std::time::Duration::from_secs(3));
    assert!(
        !lanes.hold_for_warps(&list, &project, RATE),
        "a disk thread that never answers does not freeze the edit"
    );
}

#[test]
fn a_second_edit_while_held_tells_the_disk_thread_of_its_own_stream() {
    let pool = PagePool::new(16);
    let source = file();
    let tempo = constant(120.0);
    let first = Warp::new(100.0, 0.0);
    let mut second = first.clone();
    second.original_bpm = 110.0;
    let mut warps = WarpedStreams::new(&pool);
    let a = warps
        .stream(&request(&first, &source, &tempo, 96_000))
        .unwrap();
    let b = warps
        .stream(&request(&second, &source, &tempo, 96_000))
        .unwrap();
    let project = crate::empty_project();
    let mut lanes = crate::lanes::Lanes::default();
    lanes.set_playhead(Some(0));
    // The same clip (7) twice, each time with the stream its edit made.
    assert!(lanes.hold_for_warps(&[clip_of(&a, 96_000)], &project, RATE));
    assert!(lanes.hold_for_warps(&[clip_of(&b, 96_000)], &project, RATE));
    let planned: Vec<u64> = lanes
        .streamed()
        .1
        .iter()
        .filter_map(|c| c.audio.stream().map(|s| s.id()))
        .collect();
    assert!(
        planned.contains(&b.stream().unwrap().id()),
        "the disk thread prepares the stream being waited for"
    );
}

#[test]
fn each_repeat_of_a_warped_loop_is_crossfaded_like_abutting_clips() {
    // The repeats are real clips (ADR-0131), so the engine joins them as it
    // joins any two clips that abut: an equal-power crossfade over its 5 ms
    // overlap. On a constant that shows as a bump to 0.5 * sqrt(2) in the
    // middle of the overlap, and as the file's own level everywhere else.
    let doc = project(120.0, step(120.0), true);
    let source = constant_file(0.5);
    let out = render_with(&source, doc.project(), 96_000, 256, true);
    let peak = 0.5 * std::f32::consts::SQRT_2;
    for seam in [24_000, 48_000, 72_000] {
        assert!((out[seam - 1] - 0.5).abs() < 1.0e-4, "before {seam}");
        assert!(
            (out[seam + 120] - peak).abs() < 1.0e-3,
            "middle of the overlap at {seam}"
        );
        assert!(
            (out[seam + 400] - 0.5).abs() < 1.0e-4,
            "back to 0.5 after {seam}"
        );
    }
}

#[test]
fn a_split_through_a_repeat_of_a_warped_loop_plays_on_without_a_dip() {
    let mut doc = project(110.0, step(120.0), true);
    let id = doc.project().tracks[0].clips[0].id;
    // Two and a half beats in, the middle of the third repeat (48,000 to 72,000).
    doc.split_clips(
        &[id],
        &[Ticks::quarters(2) + Ticks(lumen_core::time::PPQ / 2)],
    );
    let source = constant_file(0.5);
    let out = render_with(&source, doc.project(), 96_000, 256, true);
    for (k, v) in out.iter().enumerate().take(60_400).skip(59_600) {
        assert!((v - 0.5).abs() < 1.0e-4, "frame {k}: {v}");
    }
}

#[test]
fn a_real_time_export_keeps_pieces_far_apart_in_a_stream_apart() {
    let doc = project_at(110.0, step(120.0), false, Ticks::ZERO);
    let source = file();
    let mut warps = WarpedStreams::new(&PagePool::new(1));
    let list = engine_clips_with(
        doc.project(),
        RATE,
        |_| Some((Arc::clone(&source), RATE)),
        &mut |request| warps.stream(request),
    );
    let near = list[0].clone();
    let mut far = near.clone();
    far.id += 1;
    far.offset += 900_000;
    far.length = 1_000;
    let held = streams::in_memory(vec![near.clone(), far]).unwrap();
    assert!(!Arc::ptr_eq(&held[0].audio, &held[1].audio));
    assert_eq!(held[0].audio.frames() as i64, near.length);
    assert_eq!(held[1].audio.frames(), 1_000, "not the 900,000 between");
    assert_eq!((held[0].offset, held[1].offset), (0, 0));
}
