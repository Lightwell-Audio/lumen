use super::*;

/// Distinct samples for page `page` of a file: left positive, right negative.
fn page_of(page: usize, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let left: Vec<f32> = (0..frames)
        .map(|i| (page * PAGE + i + 1) as f32 / 1e7)
        .collect();
    let right = left.iter().map(|v| -v).collect();
    (left, right)
}

fn fill(writer: &mut PoolWriter, audio: &StreamAudio, page: usize) -> bool {
    let frames = (audio.frames() as usize - page * PAGE).min(PAGE);
    let (left, right) = page_of(page, frames);
    writer.load(audio, page, &left, &right)
}

#[test]
fn a_loaded_page_reads_back_exactly_and_a_missing_one_reads_nothing() {
    let pool = PagePool::new(4);
    let mut writer = PoolWriter::new(&pool);
    // Two and a half pages.
    let audio = StreamAudio::new(&pool, 1, 48_000, 2, (2 * PAGE + PAGE / 2) as u64);
    assert_eq!(audio.pages(), 3);
    assert!(fill(&mut writer, &audio, 0));
    assert!(fill(&mut writer, &audio, 2));
    let (left, right) = (audio.side(0), audio.side(1));
    for frame in [0, 1, PAGE - 1] {
        let want = (frame + 1) as f32 / 1e7;
        assert_eq!(left.at(frame), Some(want));
        assert_eq!(right.at(frame), Some(-want));
    }
    // Page 1 is not in memory.
    assert!(!audio.resident(1));
    assert_eq!(left.at(PAGE), None);
    assert_eq!(left.at(2 * PAGE - 1), None);
    // The last, short page; past the end there is nothing.
    let last = 2 * PAGE + PAGE / 2 - 1;
    assert_eq!(left.at(last), Some((last + 1) as f32 / 1e7));
    assert_eq!(left.at(last + 1), None);
    assert_eq!(writer.used(), 2);
    assert_eq!(writer.free(), 2);
}

#[test]
fn a_mono_file_reads_its_one_channel_on_both_sides() {
    let pool = PagePool::new(1);
    let mut writer = PoolWriter::new(&pool);
    let audio = StreamAudio::new(&pool, 1, 48_000, 1, 100);
    assert_eq!(audio.channels(), 1);
    let left: Vec<f32> = (0..100).map(|i| i as f32).collect();
    assert!(writer.load(&audio, 0, &left, &[]));
    assert_eq!(audio.side(0).at(42), Some(42.0));
    assert_eq!(audio.side(1).at(42), Some(42.0));
}

#[test]
fn loading_a_page_in_memory_again_takes_no_page() {
    let pool = PagePool::new(2);
    let mut writer = PoolWriter::new(&pool);
    let audio = StreamAudio::new(&pool, 1, 48_000, 2, PAGE as u64);
    assert!(fill(&mut writer, &audio, 0));
    assert!(fill(&mut writer, &audio, 0));
    assert_eq!(writer.used(), 1);
    // A page past the end of the file is nothing to load.
    assert!(writer.load(&audio, 5, &[], &[]));
    assert_eq!(writer.used(), 1);
}

#[test]
fn a_full_pool_refuses_until_the_least_recently_wanted_page_is_evicted() {
    let pool = PagePool::new(2);
    let mut writer = PoolWriter::new(&pool);
    let audio = StreamAudio::new(&pool, 1, 48_000, 2, (4 * PAGE) as u64);
    assert!(fill(&mut writer, &audio, 0));
    writer.tick();
    assert!(fill(&mut writer, &audio, 1));
    assert!(!fill(&mut writer, &audio, 2), "no page is free");
    // A new round wants page 1 again: page 0 goes first.
    writer.tick();
    assert!(writer.want(&audio, 1));
    assert!(!writer.want(&audio, 2), "page 2 is not in memory");
    assert_eq!(writer.evict(2), 1, "a page wanted this round stays");
    assert!(!audio.resident(0));
    assert!(audio.resident(1));
    assert!(fill(&mut writer, &audio, 2));
    assert_eq!(
        audio.side(0).at(2 * PAGE),
        Some((2 * PAGE + 1) as f32 / 1e7)
    );
}

#[test]
fn a_page_taken_out_during_a_block_is_reused_only_after_it_ends() {
    let pool = PagePool::new(1);
    let mut writer = PoolWriter::new(&pool);
    let audio = StreamAudio::new(&pool, 1, 48_000, 2, (2 * PAGE) as u64);
    assert!(fill(&mut writer, &audio, 0));
    // The audio thread is in a block that may have looked page 0 up.
    pool.enter();
    let side = audio.side(0);
    assert_eq!(side.at(0), Some(1e-7));
    writer.tick();
    assert_eq!(writer.evict(1), 1);
    assert!(!audio.resident(0));
    // Still in the block: the page is not free, so its samples hold.
    assert_eq!(writer.reclaim(), 0);
    assert!(!fill(&mut writer, &audio, 1));
    assert_eq!(side.at(0), Some(1e-7), "the block still reads what it saw");
    pool.leave();
    assert_eq!(writer.reclaim(), 1);
    assert!(fill(&mut writer, &audio, 1));
}

#[test]
fn a_page_taken_out_between_blocks_is_free_at_once() {
    let pool = PagePool::new(1);
    let mut writer = PoolWriter::new(&pool);
    let audio = StreamAudio::new(&pool, 1, 48_000, 2, (2 * PAGE) as u64);
    assert!(fill(&mut writer, &audio, 0));
    pool.enter();
    pool.leave();
    writer.tick();
    assert_eq!(writer.evict(1), 1);
    assert_eq!(writer.reclaim(), 1);
    // A block that starts after reads the table again and finds nothing.
    pool.enter();
    assert_eq!(audio.side(0).at(0), None);
    pool.leave();
}

#[test]
fn a_block_that_began_before_and_one_that_began_after_both_hold_the_page() {
    // Entered, page taken out, left and entered again before the writer
    // looks: the epoch has moved, so the page is free (the new block read
    // the table after it was cleared).
    let pool = PagePool::new(1);
    let mut writer = PoolWriter::new(&pool);
    let audio = StreamAudio::new(&pool, 1, 48_000, 2, (2 * PAGE) as u64);
    assert!(fill(&mut writer, &audio, 0));
    pool.enter();
    writer.tick();
    writer.evict(1);
    pool.leave();
    pool.enter();
    assert_eq!(audio.side(0).at(0), None);
    assert_eq!(writer.reclaim(), 1);
    pool.leave();
}

#[test]
fn the_pages_of_a_file_no_clip_holds_are_freed() {
    let pool = PagePool::new(2);
    let mut writer = PoolWriter::new(&pool);
    let audio = StreamAudio::new(&pool, 1, 48_000, 2, (2 * PAGE) as u64);
    assert!(fill(&mut writer, &audio, 0));
    assert!(fill(&mut writer, &audio, 1));
    assert_eq!(writer.free(), 0);
    drop(audio);
    assert_eq!(writer.reclaim(), 2);
    assert_eq!(writer.used(), 0);
}

#[test]
fn dropping_a_stream_takes_out_every_page() {
    let pool = PagePool::new(3);
    let mut writer = PoolWriter::new(&pool);
    let audio = StreamAudio::new(&pool, 1, 48_000, 2, (3 * PAGE) as u64);
    for page in 0..3 {
        assert!(fill(&mut writer, &audio, page));
    }
    writer.drop_stream(&audio);
    assert!((0..3).all(|p| !audio.resident(p)));
    assert_eq!(writer.reclaim(), 3);
}

#[test]
fn underruns_are_counted() {
    let pool = PagePool::new(1);
    assert_eq!(pool.underruns(), 0);
    pool.underrun();
    pool.underrun();
    assert_eq!(pool.underruns(), 2);
    assert_eq!(pool.memory(), PAGE * STRIDE * 4);
}

#[test]
#[should_panic(expected = "its own pool")]
fn a_writer_refuses_another_pools_stream() {
    let (one, two) = (PagePool::new(1), PagePool::new(1));
    let mut writer = PoolWriter::new(&one);
    let audio = StreamAudio::new(&two, 1, 48_000, 2, 10);
    writer.load(&audio, 0, &[0.0; 10], &[0.0; 10]);
}
