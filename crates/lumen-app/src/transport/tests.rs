//! What a start held for the disk does when another command comes (M9
//! slice R6, the owner's L A; review of the slice).

use std::sync::Arc;

use lumen_engine::{Clip, ClipAudio, PAGE, PagePool, PoolWriter, StreamAudio};

use super::hold::Gate;
use super::*;
use crate::disk::Cuer;
use crate::disk::plan::Scene;

const RATE: u32 = 48_000;

/// A transport playing from disk, with the first two pages of a 40 s file in
/// memory and nothing after them: a start at the song start is ready, one
/// 20 s in is held. Keep the pool alive as long as the transport.
fn gated() -> (Transport, Arc<PagePool>) {
    let mut transport = Transport::new(&crate::empty_project(), &Choice::test_device());
    let pool = PagePool::new(8);
    let frames = 40 * u64::from(RATE);
    let audio = StreamAudio::new(&pool, 1, RATE, 2, frames);
    let mut writer = PoolWriter::new(&pool);
    let (left, right) = (vec![0.1; PAGE], vec![-0.1; PAGE]);
    for page in 0..2 {
        assert!(writer.load(&audio, page, &left, &right));
    }
    let clips = [Clip {
        envelope: Default::default(),
        id: 1,
        track: 0,
        start: Ticks::ZERO,
        offset: 0,
        length: frames as i64,
        gain: 1.0,
        audio: Arc::new(ClipAudio::Stream(audio)),
        shape: Default::default(),
    }];
    let scene = Arc::new(Scene::new(
        &clips,
        &Timeline::default(),
        RATE,
        None,
        Vec::new(),
    ));
    transport.gate = Some(Gate {
        scene,
        cuer: Cuer::idle(),
    });
    (transport, pool)
}

/// 20 s in at the default 120 BPM: far from what is in memory.
const FAR: Ticks = Ticks::quarters(40);

#[test]
fn a_ready_start_replaces_one_still_held_so_the_old_one_never_fires() {
    let (mut transport, _pool) = gated();
    transport.start(Command::Locate(FAR), FAR);
    assert!(transport.held.is_some(), "far from what is in memory: held");
    // A second locate, to a place that is ready, goes at once; the first
    // must not jump back there when its 150 ms are up.
    transport.start(Command::Locate(Ticks::ZERO), Ticks::ZERO);
    assert!(transport.held.is_none());
}

#[test]
fn play_pressed_again_or_stop_drops_a_held_start() {
    let (mut transport, _pool) = gated();
    transport.start(Command::Play, FAR);
    assert!(transport.held.is_some());
    transport.play_stop();
    assert!(
        transport.held.is_none(),
        "Play again while held: it never starts"
    );

    transport.start(Command::Play, FAR);
    transport.stop();
    assert!(transport.held.is_none());

    transport.start(Command::Play, FAR);
    transport.panic();
    assert!(transport.held.is_none());
}
