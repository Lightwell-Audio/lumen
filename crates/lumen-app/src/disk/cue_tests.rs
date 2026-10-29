//! Deterministic tests for the cue adoption handshake.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use lumen_core::time::Ticks;
use lumen_engine::{
    Clip, ClipAudio, ClipShape, Command, Destination, NodeSpec, PagePool, PoolWriter, Routing,
    StreamAudio, Timeline, clip_player, engine, mixer,
};

use super::*;
use crate::transport::hold::Gate;

const RATE: u32 = 48_000;

#[test]
fn a_scene_without_streamed_audio_needs_no_worker_acknowledgement() {
    let (sender, _inbox) = channel();
    let cues = Arc::new(CueState::default());
    let cuer = Cuer(Some(CuePort {
        sender,
        state: cues,
    }));
    cuer.cue(Some(0));
    assert!(!cuer.adopted());
    let gate = Gate {
        scene: Arc::default(),
        cuer,
    };
    assert!(gate.ready(0), "there are no pages to protect");
}

struct PagesForTest;

impl Pages for PagesForTest {
    fn read(
        &mut self,
        audio: &StreamAudio,
        page: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError> {
        let from = page * PAGE;
        let frames = (audio.frames() as usize).saturating_sub(from).min(PAGE);
        left[..frames].fill(0.1);
        right[..frames].fill(-0.1);
        Ok(frames)
    }

    fn keep(&mut self, _: &[StreamAudio]) {}
}

struct StagedPages {
    entered: Sender<usize>,
    release: Receiver<()>,
}

impl Pages for StagedPages {
    fn read(
        &mut self,
        audio: &StreamAudio,
        page: usize,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<usize, MediaError> {
        self.entered.send(page).unwrap();
        self.release.recv().unwrap();
        let from = page * PAGE;
        let frames = (audio.frames() as usize).saturating_sub(from).min(PAGE);
        left[..frames].fill(0.1);
        right[..frames].fill(-0.1);
        Ok(frames)
    }

    fn keep(&mut self, _: &[StreamAudio]) {}
}

fn reader() -> TransportReader {
    let (clips, source) = clip_player();
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
    let (handle, _engine) = engine(Timeline::default(), source, RATE);
    let reader = handle.transport_reader();
    let _ = (&clips, &mut routing);
    reader
}

#[test]
fn ready_waits_until_the_latest_cue_is_adopted_after_an_old_plan_round() {
    let pool = PagePool::new(1);
    let audio = StreamAudio::new(&pool, 1, RATE, 2, (100 * PAGE) as u64);
    let clip = Clip {
        envelope: Default::default(),
        id: 1,
        track: 0,
        start: Ticks::ZERO,
        offset: 0,
        length: (100 * PAGE) as i64,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Stream(audio.clone())),
        shape: ClipShape::default(),
    };
    let scene = Arc::new(Scene::new(
        &[clip],
        &Timeline::default(),
        RATE,
        None,
        Vec::new(),
    ));
    let cues = Arc::new(CueState::default());
    let (sender, inbox) = channel();
    let cuer = Cuer(Some(CuePort {
        sender,
        state: Arc::clone(&cues),
    }));
    let gate = Gate {
        scene: Arc::clone(&scene),
        cuer: cuer.clone(),
    };
    let stats = Arc::new(Stats::default());
    let mut worker = Worker::with_cues(
        &pool,
        Box::new(PagesForTest),
        reader(),
        Arc::clone(&stats),
        cues,
    );
    worker.take(Message::Scene(Arc::clone(&scene)));

    let (left, right) = (vec![0.2; PAGE], vec![-0.2; PAGE]);
    assert!(worker.writer.load(&audio, 10, &left, &right));
    let sample = (10 * PAGE) as i64;
    assert!(scene.ready(sample, 1), "the target is initially resident");
    cuer.cue(Some(sample));
    assert!(scene.ready(sample, 1), "cue submission does not evict it");
    assert!(
        !gate.ready(sample),
        "residency alone cannot release a start before cue adoption"
    );

    // This models a worker round already in progress when the UI submitted
    // the cue: it still uses the old playhead plan and evicts page 10.
    worker.round_for(Duration::MAX);
    assert!(!audio.resident(10), "the old plan evicts the target");
    assert!(!gate.ready(sample), "the unadopted cue remains held");

    // The next worker round consumes the latest cue, makes it the first
    // planned page, and only then publishes adoption.
    while let Ok(message) = inbox.try_recv() {
        worker.take(message);
    }
    worker.round_for(Duration::MAX);
    assert!(audio.resident(10));
    assert!(gate.ready(sample));
    let snapshot = stats.worker_snapshot();
    assert!(snapshot.round >= 2);
    assert_eq!(snapshot.sample, 0);
    assert_eq!(snapshot.cue, Some(sample));
    assert_eq!(snapshot.command_serial, 0);
    assert_eq!(snapshot.read_stream, audio.id());
    assert_eq!(snapshot.read_page, 10);
    let _elapsed = snapshot.read_elapsed_nanos;
}

#[test]
fn cue_adoption_waits_for_the_capacity_limited_prefill_horizon() {
    let pool = PagePool::new(3);
    let audio = StreamAudio::new(&pool, 21, RATE, 2, (400 * PAGE) as u64);
    let clip = Clip {
        envelope: Default::default(),
        id: 21,
        track: 0,
        start: Ticks::ZERO,
        offset: 0,
        length: (400 * PAGE) as i64,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Stream(audio.clone())),
        shape: ClipShape::default(),
    };
    let scene = Arc::new(Scene::new(
        &[clip],
        &Timeline::default(),
        RATE,
        None,
        Vec::new(),
    ));
    let cues = Arc::new(CueState::default());
    let (sender, inbox) = channel();
    let cuer = Cuer(Some(CuePort {
        sender,
        state: Arc::clone(&cues),
    }));
    let gate = Gate {
        scene: Arc::clone(&scene),
        cuer: cuer.clone(),
    };
    let (entered_tx, entered_rx) = channel();
    let (release_tx, release_rx) = channel();
    let stats = Arc::new(Stats::default());
    let mut worker = Worker::with_cues(
        &pool,
        Box::new(StagedPages {
            entered: entered_tx,
            release: release_rx,
        }),
        reader(),
        stats,
        cues,
    );
    worker.take(Message::Scene(Arc::clone(&scene)));

    // The failing hosted cue: 50 ms from 1,488,000 ends before page 364,
    // while the player's 40 ms lookahead reaches it after 21.33 ms.
    // Cache the first page; hold both following reads in a controlled order.
    let sample = 1_488_000;
    let (left, right) = (vec![0.2; PAGE], vec![-0.2; PAGE]);
    assert!(worker.writer.load(&audio, 363, &left, &right));
    assert!(scene.ready(sample, i64::from(RATE) / 20));
    cuer.cue(Some(sample));
    worker.take(inbox.try_recv().unwrap());
    assert!(!gate.ready(sample));

    let join = std::thread::spawn(move || worker.round_for(Duration::MAX));
    assert_eq!(
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        364
    );
    assert!(audio.resident(363));
    assert!(!audio.resident(364));
    assert!(
        !gate.ready(sample),
        "first-page residency is not cue prefill"
    );

    release_tx.send(()).unwrap();
    assert_eq!(
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        365
    );
    assert!(audio.resident(364));
    assert!(!audio.resident(365));
    assert!(
        !gate.ready(sample),
        "the rest of the selected horizon is pending"
    );

    release_tx.send(()).unwrap();
    let _ = join.join().unwrap();
    assert!(audio.resident(365));
    assert!(
        gate.ready(sample),
        "adoption follows completion of the cue prefix"
    );
}

#[test]
fn a_scene_replacement_invalidates_an_adopted_cue_until_the_new_plan_is_wanted() {
    let pool = PagePool::new(2);
    let (old_audio, new_audio) = (
        StreamAudio::new(&pool, 11, RATE, 2, PAGE as u64),
        StreamAudio::new(&pool, 12, RATE, 2, PAGE as u64),
    );
    let mut pages = PoolWriter::new(&pool);
    let (left, right) = (vec![0.1; PAGE], vec![-0.1; PAGE]);
    assert!(pages.load(&old_audio, 0, &left, &right));
    assert!(pages.load(&new_audio, 0, &left, &right));
    let make_scene = |audio: StreamAudio, id| {
        Arc::new(Scene::new(
            &[Clip {
                envelope: Default::default(),
                id,
                track: 0,
                start: Ticks::ZERO,
                offset: 0,
                length: PAGE as i64,
                gain: 1.0,
                audio: Arc::new(ClipAudio::Stream(audio)),
                shape: ClipShape::default(),
            }],
            &Timeline::default(),
            RATE,
            None,
            Vec::new(),
        ))
    };
    let (old_scene, new_scene) = (make_scene(old_audio, 11), make_scene(new_audio, 12));
    let cues = Arc::new(CueState::default());
    let (sender, inbox) = channel();
    let cuer = Cuer(Some(CuePort {
        sender: sender.clone(),
        state: Arc::clone(&cues),
    }));
    let old_gate = Gate {
        scene: Arc::clone(&old_scene),
        cuer: cuer.clone(),
    };
    let new_gate = Gate {
        scene: Arc::clone(&new_scene),
        cuer: cuer.clone(),
    };
    let mut worker = Worker::with_cues(
        &pool,
        Box::new(PagesForTest),
        reader(),
        Arc::new(Stats::default()),
        Arc::clone(&cues),
    );
    worker.take(Message::Scene(Arc::clone(&old_scene)));
    worker.round_for(Duration::MAX);

    cuer.cue(Some(0));
    worker.take(inbox.recv().unwrap());
    worker.round_for(Duration::MAX);
    assert!(cuer.adopted());
    assert!(old_gate.ready(0));

    let generation = cues.request();
    sender
        .send(Message::SceneRequest {
            scene: Arc::clone(&new_scene),
            generation,
        })
        .unwrap();
    assert!(new_scene.ready(0, 1), "the new scene's page is resident");
    assert!(
        !new_gate.ready(0),
        "an old scene's adoption cannot release a held start"
    );

    worker.take(inbox.recv().unwrap());
    assert!(!new_gate.ready(0), "taking the scene is not planning it");
    worker.round_for(Duration::MAX);
    assert!(cuer.adopted());
    assert!(new_gate.ready(0), "the new plan protects its resident page");
}

#[test]
fn a_replaced_cue_ignores_older_generations_and_cancellation_is_adopted() {
    let pool = PagePool::new(4);
    let audio = StreamAudio::new(&pool, 2, RATE, 2, (20 * PAGE) as u64);
    let clip = Clip {
        envelope: Default::default(),
        id: 2,
        track: 0,
        start: Ticks::ZERO,
        offset: 0,
        length: (20 * PAGE) as i64,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Stream(audio)),
        shape: ClipShape::default(),
    };
    let scene = Arc::new(Scene::new(
        &[clip],
        &Timeline::default(),
        RATE,
        None,
        Vec::new(),
    ));
    let cues = Arc::new(CueState::default());
    let (sender, inbox) = channel();
    let cuer = Cuer(Some(CuePort {
        sender,
        state: Arc::clone(&cues),
    }));
    let mut worker = Worker::with_cues(
        &pool,
        Box::new(PagesForTest),
        reader(),
        Arc::new(Stats::default()),
        cues,
    );
    worker.take(Message::Scene(scene));

    cuer.cue(Some((2 * PAGE) as i64));
    let old = inbox.recv().unwrap();
    cuer.cue(Some((8 * PAGE) as i64));
    let newest = inbox.recv().unwrap();
    worker.take(newest);
    worker.take(old);
    worker.round_for(Duration::MAX);
    assert_eq!(worker.cue, Some((8 * PAGE) as i64));
    assert!(cuer.adopted(), "only the newest request is acknowledged");

    cuer.cue(None);
    assert!(!cuer.adopted(), "cancellation has its own generation");
    worker.take(inbox.recv().unwrap());
    worker.round_for(Duration::MAX);
    assert_eq!(worker.cue, None);
    assert!(cuer.adopted(), "the cancellation was adopted");
}

#[test]
fn request_generations_use_wrapping_serial_order() {
    assert!(serial_reached(0, u64::MAX));
    assert!(serial_reached(1, u64::MAX));
    assert!(!serial_reached(u64::MAX, 0));
}

#[test]
fn follow_after_keeps_the_real_cue_until_the_transport_applies_its_command() {
    let pool = PagePool::new(8);
    let audio = StreamAudio::new(&pool, 3, RATE, 2, (20 * PAGE) as u64);
    let clip = Clip {
        envelope: Default::default(),
        id: 3,
        track: 0,
        start: Ticks::ZERO,
        offset: 0,
        length: (20 * PAGE) as i64,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Stream(audio)),
        shape: ClipShape::default(),
    };
    let scene = Arc::new(Scene::new(
        &[clip],
        &Timeline::default(),
        RATE,
        None,
        Vec::new(),
    ));
    let cues = Arc::new(CueState::default());
    let (sender, inbox) = channel();
    let cuer = Cuer(Some(CuePort {
        sender,
        state: Arc::clone(&cues),
    }));
    let (_, source) = clip_player();
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
    let (mut handle, mut engine) = engine(Timeline::default(), source, RATE);
    let mut worker = Worker::with_cues(
        &pool,
        Box::new(PagesForTest),
        handle.transport_reader(),
        Arc::new(Stats::default()),
        cues,
    );
    worker.take(Message::Scene(scene));

    cuer.cue(Some(0));
    worker.take(inbox.recv().unwrap());
    worker.round_for(Duration::MAX);
    assert!(cuer.adopted());
    handle.send(Command::Locate(Ticks::ZERO)).unwrap();
    cuer.follow_after(handle.sent_command());
    worker.take(inbox.recv().unwrap());

    worker.round_for(Duration::MAX);
    assert_eq!(worker.cue, Some(0), "not yet applied by the engine");
    let mut data = vec![0.0; 2 * 128];
    engine.process(RATE, &mut data, 2, 128);
    worker.round_for(Duration::MAX);
    assert_eq!(worker.cue, None, "cleared after the published serial");
}
