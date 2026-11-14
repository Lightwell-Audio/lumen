//! Parallax (the stereo delay) and Afterglow (the reverb), M8 slice B3: what
//! they do to the audio, that a synced echo lands on the beat and follows a
//! tempo change, that the reverb's decay is the time it says, that the same
//! audio comes out at every block size, and that nothing allocates while they
//! run.

use lumen_devices::delay::{self, Delay};
use lumen_devices::params;
use lumen_devices::reverb::{self, Reverb};
use lumen_devices::{Block, Effect, ParamBlock, Stereo};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;

/// The normalised value of `id` at `value` in its own units.
fn at(table: &'static [params::ParamInfo], id: u32, value: f32) -> f32 {
    params::find(table, id)
        .expect("a parameter of the table")
        .normalised(value)
}

/// The note choice's value (0 is Free): index / 6.
fn note(index: usize) -> f32 {
    index as f32 / 6.0
}

/// A block: every parameter at rest, `set`'s values (normalised) on top, and
/// the lanes' points.
#[derive(Clone)]
struct BlockBuilder {
    frames: usize,
    bpm: f32,
    points: Vec<(u32, Vec<(u32, f64)>)>,
}

impl BlockBuilder {
    fn new(frames: usize) -> Self {
        Self {
            frames,
            bpm: 120.0,
            points: Vec::new(),
        }
    }

    fn bpm(mut self, bpm: f32) -> Self {
        self.bpm = bpm;
        self
    }

    fn held(mut self, id: u32, value: f32) -> Self {
        self.points.push((id, vec![(0, f64::from(value))]));
        self
    }

    fn lane(mut self, id: u32, points: &[(u32, f32)]) -> Self {
        self.points.push((
            id,
            points
                .iter()
                .map(|&(at, value)| (at, f64::from(value)))
                .collect(),
        ));
        self
    }

    fn build(&self) -> ParamBlock {
        let mut events = ParamBlock::new();
        events.start(self.frames);
        for (id, points) in &self.points {
            events.open(*id);
            for &(at, value) in points {
                events.point(at, value);
            }
        }
        events
    }
}

/// The values of every parameter of `table`, `set`'s (normalised) on top.
fn table_block(
    table: &'static [params::ParamInfo],
    frames: usize,
    set: &[(u32, f32)],
) -> BlockBuilder {
    let mut block = BlockBuilder::new(frames);
    for info in table {
        let value = set
            .iter()
            .find(|(id, _)| *id == info.id)
            .map_or_else(|| info.normalised(info.default), |&(_, value)| value);
        block = block.held(info.id, value);
    }
    block
}

/// Runs `device` over `lead` frames of silence, then `signal` (one sample per
/// frame, both sides), a block at a time. `bpm_at` gives the tempo at a
/// timeline sample, `lanes` the block's parameters. Returns left and right.
fn run_device(
    device: &mut dyn Effect,
    lead: usize,
    signal: &[f32],
    block: usize,
    bpm_at: &dyn Fn(usize) -> f32,
    lanes: &dyn Fn(usize, usize) -> BlockBuilder,
) -> (Vec<f32>, Vec<f32>) {
    let block = block.clamp(1, 256);
    let total = lead + signal.len();
    let mut left = vec![0.0; total];
    let mut right = vec![0.0; total];
    left[lead..].copy_from_slice(signal);
    right[lead..].copy_from_slice(signal);
    let mut done = 0;
    while done < total {
        let n = block.min(total - done);
        let bpm = bpm_at(done);
        let events = lanes(done, n).bpm(bpm).build();
        let block = Block {
            rate: RATE,
            start: done as i64,
            bpm,
            law: lumen_core::pan::PanLaw::Balance,
            events: &events,
        };
        device.process(
            &block,
            Stereo {
                left: &mut left[done..done + n],
                right: &mut right[done..done + n],
            },
            None,
        );
        done += n;
    }
    (left, right)
}

/// 120 BPM everywhere: the reference tempo.
fn steady_120(_: usize) -> f32 {
    120.0
}

/// A delay run with `set`, over silence for `lead` frames, then `signal`.
fn delay_run(set: &[(u32, f32)], lead: usize, signal: &[f32], bpm: f32, block: usize) -> Vec<f32> {
    let set = set.to_vec();
    let mut device = Delay::new();
    let lanes = move |_: usize, n: usize| table_block(delay::TABLE, n, &set);
    let bpm_at = move |_: usize| bpm;
    run_device(&mut device, lead, signal, block, &bpm_at, &lanes).0
}

/// A reverb run with `set`, over the signal.
fn reverb_run(set: &[(u32, f32)], signal: &[f32], block: usize) -> (Vec<f32>, Vec<f32>) {
    let set = set.to_vec();
    let mut device = Reverb::new();
    let lanes = move |_: usize, n: usize| table_block(reverb::TABLE, n, &set);
    run_device(&mut device, 0, signal, block, &steady_120, &lanes)
}

/// Where and how loud the loudest frame of `samples[from..]` is.
fn loudest(samples: &[f32], from: usize) -> (usize, f32) {
    let mut at = from;
    let mut level = 0.0f32;
    for (k, sample) in samples.iter().enumerate().skip(from) {
        if sample.abs() > level {
            level = sample.abs();
            at = k;
        }
    }
    (at, level)
}

/// The RT60 of an impulse response: the slope of its energy decay between
/// −5 dB and −35 dB, turned into the time of 60 dB (the standard estimate).
fn measured_rt60(response: &[f32], rate: u32) -> f32 {
    let mut energy = 0.0f64;
    let mut decay = vec![0.0f64; response.len()];
    for k in (0..response.len()).rev() {
        energy += f64::from(response[k]).powi(2);
        decay[k] = energy;
    }
    let total = decay[0];
    assert!(total > 0.0, "the reverb made no sound");
    let level = |db: f64| total * 10f64.powf(db / 10.0);
    let mut points = Vec::new();
    for (k, &e) in decay.iter().enumerate() {
        if points.is_empty() && e <= level(-5.0) {
            points.push(k);
        }
        if e <= level(-35.0) {
            points.push(k);
            break;
        }
    }
    assert_eq!(points.len(), 2, "the response was too short to measure");
    let seconds = (points[1] - points[0]) as f32 / rate as f32;
    seconds * 60.0 / 30.0
}

#[test]
fn the_two_tables_are_pinned_and_every_id_is_stable_and_unique() {
    // Ids and defaults are frozen: a change here is a change to what saved
    // projects play, and is deliberate.
    let delay_table: Vec<(u32, &str, f32)> = delay::TABLE
        .iter()
        .map(|p| (p.id, p.name, p.normalised(p.default)))
        .collect();
    assert_eq!(
        delay_table,
        [
            (1, "Time", at(delay::TABLE, delay::TIME, 375.0)),
            (2, "Note", 4.0 / 6.0),
            (3, "Repeats", 0.35),
            (4, "Ping", 0.0),
            (5, "Low cut", 0.0),
            (6, "High cut", 1.0),
            (7, "Mix", 0.3),
            (8, "Freeze", 0.0),
        ]
    );
    let reverb_table: Vec<(u32, &str, f32)> = reverb::TABLE
        .iter()
        .map(|p| (p.id, p.name, p.normalised(p.default)))
        .collect();
    assert_eq!(reverb_table.len(), 7);
    assert_eq!(reverb_table[0].0, reverb::PREDELAY);
    assert_eq!(
        reverb_table[2],
        (3, "Decay", at(reverb::TABLE, reverb::DECAY, 2.0))
    );
    assert_eq!(reverb_table[6].1, "Mix");
    for table in [delay::TABLE, reverb::TABLE] {
        let ids: std::collections::HashSet<u32> = table.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), table.len(), "an id is reused");
        assert!(!ids.contains(&0), "zero is not a parameter");
        assert!(table.len() <= lumen_devices::MOST_QUEUES);
        for info in table {
            assert!(info.default >= info.low && info.default <= info.high);
        }
    }
    // The notes the delay offers, in order: Free first, then the tempo's.
    assert_eq!(
        delay::TABLE[1].choices,
        ["Free", "1/32", "1/16", "1/8", "1/4", "1/2", "1/1"]
    );
}

#[test]
fn a_value_reads_in_its_own_words_and_is_read_back() {
    let time = &delay::TABLE[0];
    assert_eq!(time.text_of(at(delay::TABLE, delay::TIME, 375.0)), "375 ms");
    assert_eq!(
        time.text_of(at(delay::TABLE, delay::TIME, 1200.0)),
        "1.20 s"
    );
    let note_info = &delay::TABLE[1];
    assert_eq!(
        note_info.text_of(at(delay::TABLE, delay::NOTE, 0.0)),
        "Free"
    );
    assert_eq!(note_info.value_of("1/4"), Some(note(4)));
    assert_eq!(note_info.value_of("quarter"), Some(note(4)));
    assert_eq!(note_info.text_of(note_info.value_of("1/8").unwrap()), "1/8");
    assert!(note_info.value_of("nine").is_none());
    let high = &delay::TABLE[5];
    assert_eq!(
        high.text_of(at(delay::TABLE, delay::HIGH_CUT, 20_000.0)),
        "Off"
    );
    assert_eq!(high.value_of("off"), Some(1.0));
    assert_eq!(
        high.text_of(at(delay::TABLE, delay::HIGH_CUT, 8000.0)),
        "8.0 kHz"
    );
    assert_eq!(
        high.value_of("8 kHz"),
        Some(at(delay::TABLE, delay::HIGH_CUT, 8000.0))
    );
    let decay = &reverb::TABLE[2];
    assert_eq!(
        decay.text_of(at(reverb::TABLE, reverb::DECAY, 2.0)),
        "2.00 s"
    );
    assert!(decay.value_of("2.4 s").is_some());
    assert_eq!(
        delay::TABLE[6].text_of(at(delay::TABLE, delay::MIX, 30.0)),
        "30 %"
    );
}

#[test]
fn every_description_reports_the_tail_its_running_instance_does() {
    // At its defaults: a fresh instance's tempo and rate are the reference the
    // description is worked out at.
    for (name, table) in [(delay::NAME, delay::TABLE), (reverb::NAME, reverb::TABLE)] {
        let defaults: Vec<(u32, f32)> = table
            .iter()
            .map(|p| (p.id, p.normalised(p.default)))
            .collect();
        let description = lumen_devices::effect_description(name).unwrap();
        let fresh = lumen_devices::effect(name).unwrap();
        assert_eq!((description.tail)(&defaults), fresh.tail(), "{name}");
        assert_eq!((description.latency)(&defaults), fresh.latency(), "{name}");
    }
    // Its tails move with its settings: a longer time, more feedback, a longer
    // decay and the mix all show in it.
    let delay_tail = |set: &[(u32, f32)]| {
        let values: Vec<(u32, f32)> = delay::TABLE
            .iter()
            .map(|p| {
                (
                    p.id,
                    set.iter()
                        .find(|(id, _)| *id == p.id)
                        .map_or_else(|| p.normalised(p.default), |&(_, v)| v),
                )
            })
            .collect();
        delay::tail_frames(&values, 120.0, 48_000)
    };
    let quarter = note(4);
    let eighth = note(3);
    let more = at(delay::TABLE, delay::FEEDBACK, 80.0);
    let less = at(delay::TABLE, delay::FEEDBACK, 10.0);
    let wet = at(delay::TABLE, delay::MIX, 100.0);
    let dry = at(delay::TABLE, delay::MIX, 0.0);
    let frozen = at(delay::TABLE, delay::FREEZE, 1.0);
    let long = [
        (delay::NOTE, quarter),
        (delay::MIX, wet),
        (delay::FEEDBACK, more),
    ];
    assert!(
        delay_tail(&long)
            > delay_tail(&[
                (delay::NOTE, eighth),
                (delay::MIX, wet),
                (delay::FEEDBACK, more)
            ])
    );
    assert!(
        delay_tail(&long)
            > delay_tail(&[
                (delay::NOTE, quarter),
                (delay::MIX, wet),
                (delay::FEEDBACK, less)
            ])
    );
    assert_eq!(
        delay_tail(&[(delay::MIX, dry)]),
        0,
        "a dry delay has no tail"
    );
    assert_eq!(
        delay_tail(&[(delay::MIX, wet), (delay::FREEZE, frozen)]),
        (10.0 * 48_000.0) as u32,
        "a frozen loop is capped at ten seconds, not for ever"
    );
    let reverb_tail = |decay: f32, mix: f32| {
        let values: Vec<(u32, f32)> = reverb::TABLE
            .iter()
            .map(|p| match p.id {
                reverb::DECAY => (p.id, at(reverb::TABLE, reverb::DECAY, decay)),
                reverb::MIX => (p.id, at(reverb::TABLE, reverb::MIX, mix)),
                _ => (p.id, p.normalised(p.default)),
            })
            .collect();
        reverb::tail_frames(&values, 48_000)
    };
    assert!(reverb_tail(4.0, 50.0) > reverb_tail(0.5, 50.0));
    assert_eq!(reverb_tail(4.0, 0.0), 0);
    // And a running instance's tail follows the tempo it has played at: a
    // quarter note at 60 BPM is twice the tail of one at 120.
    let mut slow = Delay::new();
    let set = [
        (delay::NOTE, quarter),
        (delay::MIX, wet),
        (delay::FEEDBACK, less),
    ];
    let signal = vec![0.0; 4_000];
    let lanes = move |_: usize, n: usize| table_block(delay::TABLE, n, &set);
    run_device(&mut slow, 0, &signal, 128, &|_| 60.0, &lanes);
    let mut fast = Delay::new();
    run_device(&mut fast, 0, &signal, 128, &|_| 120.0, &lanes);
    assert!(
        slow.tail() > fast.tail(),
        "{} against {}",
        slow.tail(),
        fast.tail()
    );
}

#[test]
fn a_synced_echo_lands_on_the_beat_to_the_sample() {
    // Four at 120 BPM is 24000 frames at 48 kHz, exactly.
    let mut signal = vec![0.0; 60_000];
    signal[0] = 1.0;
    let heard = delay_run(
        &[
            (delay::NOTE, note(4)),
            (delay::MIX, at(delay::TABLE, delay::MIX, 100.0)),
            (delay::FEEDBACK, 0.0),
        ],
        0,
        &signal,
        120.0,
        128,
    );
    let (at_frame, level) = loudest(&heard, 1);
    assert_eq!(at_frame, 24_000, "the echo is on the beat: {level}");
    assert!((level - 1.0).abs() < 1e-4, "and the whole impulse: {level}");
    // An eighth of a note is 12000 frames: half a quarter at 120 BPM.
    let mut signal = vec![0.0; 20_000];
    signal[0] = 1.0;
    let heard = delay_run(
        &[
            (delay::NOTE, note(3)),
            (delay::MIX, 1.0),
            (delay::FEEDBACK, 0.0),
        ],
        0,
        &signal,
        120.0,
        97,
    );
    assert_eq!(loudest(&heard, 1).0, 12_000);
}

#[test]
fn a_synced_echo_follows_a_tempo_change() {
    // A quarter note, 120 BPM until sample 96000, then 60: the sound at the
    // change lands one new beat later, sample for sample.
    let bpm_at = |sample: usize| if sample < 96_000 { 120.0 } else { 60.0 };
    let mut signal = vec![0.0; 160_000];
    signal[0] = 1.0;
    signal[96_000] = 1.0;
    let set = vec![
        (delay::NOTE, note(4)),
        (delay::MIX, 1.0),
        (delay::FEEDBACK, 0.0),
    ];
    let mut device = Delay::new();
    let lanes = {
        let set = set.clone();
        move |_: usize, n: usize| table_block(delay::TABLE, n, &set)
    };
    let (heard, _) = run_device(&mut device, 0, &signal, 256, &bpm_at, &lanes);
    assert_eq!(loudest(&heard, 1).0, 24_000, "the first echo, at 120");
    let (at_frame, level) = loudest(&heard, 96_001);
    assert_eq!(at_frame, 144_000, "the echo after the change: {level}");
    assert!((level - 1.0).abs() < 1e-3, "{level}");
    // No click: the output never steps between neighbouring frames by more
    // than the impulse itself (the glide slides the read point).
    let worst = heard
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 1.2, "a step of {worst}");
}

#[test]
fn the_feedback_decays_the_repeats_by_its_percent() {
    // 100 ms, half the feedback: 1, 0.5, 0.25 at 4800-frame intervals.
    let lead = 2_000;
    let mut signal = vec![0.0; 40_000];
    signal[0] = 1.0;
    let heard = delay_run(
        &[
            (delay::TIME, at(delay::TABLE, delay::TIME, 100.0)),
            (delay::NOTE, 0.0),
            (delay::FEEDBACK, at(delay::TABLE, delay::FEEDBACK, 50.0)),
            (delay::MIX, at(delay::TABLE, delay::MIX, 100.0)),
        ],
        lead,
        &signal,
        120.0,
        128,
    );
    let first = lead + 4_800;
    assert!((heard[first] - 1.0).abs() < 1e-3, "{}", heard[first]);
    assert!((heard[first + 4_800] - 0.5).abs() < 1e-3);
    assert!((heard[first + 9_600] - 0.25).abs() < 1e-3);
}

#[test]
fn the_loop_s_high_cut_darkens_the_repeats() {
    // An impulse, 100 ms a repeat, a strong feedback: the tenth echo has been
    // through the loop's high cut nine times, so at 500 Hz it is gone where an
    // open loop still has it.
    let lead = 2_000;
    let mut signal = vec![0.0; 60_000];
    signal[0] = 1.0;
    let echo = |high_cut: f32| {
        let heard = delay_run(
            &[
                (delay::TIME, at(delay::TABLE, delay::TIME, 100.0)),
                (delay::NOTE, 0.0),
                (delay::FEEDBACK, at(delay::TABLE, delay::FEEDBACK, 90.0)),
                (delay::LOW_CUT, 0.0),
                (delay::HIGH_CUT, at(delay::TABLE, delay::HIGH_CUT, high_cut)),
                (delay::MIX, at(delay::TABLE, delay::MIX, 100.0)),
            ],
            lead,
            &signal,
            120.0,
            128,
        );
        let at = lead + 9 * 4_800;
        heard[at..at + 4_800]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()))
    };
    let open = echo(20_000.0);
    let dark = echo(500.0);
    assert!(open > 0.2, "the open loop still has the tenth echo: {open}");
    assert!(
        dark < open * 0.1,
        "the high cut keeps its light: {dark} against {open}"
    );
}

#[test]
fn freeze_holds_the_loop_without_new_sound() {
    // Feed 220 Hz, freeze, then feed nothing: the loop still rings, and the
    // tail it reports is the ten-second cap rather than for ever.
    let mut signal: Vec<f32> = (0..40_000)
        .map(|k| (std::f32::consts::TAU * 220.0 * k as f32 / RATE as f32).sin() * 0.4)
        .collect();
    for sample in &mut signal[20_000..] {
        *sample = 0.0;
    }
    let mut device = Delay::new();
    let set = [
        (delay::TIME, at(delay::TABLE, delay::TIME, 200.0)),
        (delay::NOTE, 0.0),
        (delay::FEEDBACK, 0.0),
        (delay::MIX, at(delay::TABLE, delay::MIX, 100.0)),
        (delay::FREEZE, at(delay::TABLE, delay::FREEZE, 1.0)),
    ];
    let lanes = |_: usize, n: usize| table_block(delay::TABLE, n, &set);
    let (heard, _) = run_device(&mut device, 0, &signal, 256, &steady_120, &lanes);
    let level = heard[30_000..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(level > 0.1, "the loop is held: {level}");
    assert!(device.tail() >= 9 * 48_000, "capped, {}", device.tail());
    assert!(device.tail() <= 10 * 48_000);
}

#[test]
fn the_reverb_s_decay_is_the_time_it_says() {
    for decay in [0.5f32, 1.5, 4.0] {
        let frames = ((decay * 1.6 + 0.3) * RATE as f32) as usize;
        let mut signal = vec![0.0; frames];
        signal[0] = 1.0;
        let heard = reverb_run(
            &[
                (reverb::PREDELAY, 0.0),
                (reverb::SIZE, at(reverb::TABLE, reverb::SIZE, 60.0)),
                (reverb::DECAY, at(reverb::TABLE, reverb::DECAY, decay)),
                (reverb::DAMPING, 0.0),
                (reverb::WIDTH, at(reverb::TABLE, reverb::WIDTH, 100.0)),
                (reverb::EARLY, 0.0),
                (reverb::MIX, at(reverb::TABLE, reverb::MIX, 100.0)),
            ],
            &signal,
            128,
        );
        let measured = measured_rt60(&heard.0, RATE);
        assert!(
            (measured - decay).abs() / decay < 0.10,
            "decay {decay}: measured {measured}"
        );
    }
}

#[test]
fn a_reverb_makes_a_stereo_tail_and_width_moves_it() {
    let mut signal = vec![0.0; 20_000];
    signal[0] = 1.0;
    let run = |width: f32| {
        reverb_run(
            &[
                (reverb::EARLY, at(reverb::TABLE, reverb::EARLY, 60.0)),
                (reverb::WIDTH, at(reverb::TABLE, reverb::WIDTH, width)),
            ],
            &signal,
            128,
        )
    };
    let (left, right) = run(100.0);
    let tail = 5_000;
    let side = left[tail..]
        .iter()
        .zip(&right[tail..])
        .map(|(l, r)| (f64::from(*l) - f64::from(*r)).abs())
        .sum::<f64>();
    assert!(side > 0.0, "the tail has two sides");
    let (left, right) = run(0.0);
    let side = left[tail..]
        .iter()
        .zip(&right[tail..])
        .map(|(l, r)| (l - r).abs())
        .fold(0.0f32, f32::max);
    assert!(side < 1e-5, "at zero width the tail is mono: {side}");
}

/// A lane's value at an absolute frame: straight between its points.
fn curve_at(points: &[(u32, f32)], frame: usize) -> f32 {
    let after = points.partition_point(|p| p.0 as usize <= frame);
    let Some(before) = after.checked_sub(1) else {
        return points[0].1;
    };
    let (at, value) = points[before];
    match points.get(after) {
        Some(&(next, next_value)) if next as usize > frame => {
            let span = (next - at) as f32;
            let into = (frame - at as usize) as f32;
            value + (next_value - value) * (into / span)
        }
        _ => value,
    }
}

/// The points a lane gives one block, as the engine fills them: where it is at
/// the block's first frame, each of its points inside the block, and where it
/// has got to at its last.
fn window(points: &[(u32, f32)], done: usize, n: usize) -> Vec<(u32, f32)> {
    let mut entries: Vec<(usize, f32)> = vec![(0, curve_at(points, done))];
    for &(at, value) in points {
        let at = at as usize;
        if at > done && at < done + n {
            entries.push((at - done, value));
        }
    }
    if n > 1 && !entries.iter().any(|&(offset, _)| offset == n - 1) {
        entries.push((n - 1, curve_at(points, done + n - 1)));
    }
    entries
        .into_iter()
        .map(|(offset, value)| (offset as u32, value))
        .collect()
}

#[test]
fn both_devices_play_the_same_audio_at_every_block_size() {
    // A tone with a moving parameter lane: the same frames at 64, 97, 128, 512
    // and 1024.
    let signal: Vec<f32> = (0..12_000)
        .map(|k| {
            let t = k as f32 / RATE as f32;
            (std::f32::consts::TAU * 440.0 * t).sin() * 0.3
                + (std::f32::consts::TAU * 3_100.0 * t).sin() * 0.1
        })
        .collect();
    let points = [(0u32, 0.4f32), (4_000, 0.4), (4_001, 0.85), (8_000, 0.1)];
    let lane = move |done: usize, n: usize| {
        table_block(delay::TABLE, n, &[]).lane(delay::FEEDBACK, &window(&points, done, n))
    };
    let mut whole_device = Delay::new();
    let whole = run_device(&mut whole_device, 0, &signal, 64, &|_| 110.0, &lane).0;
    for block in [97usize, 128, 512, 1_024] {
        let mut device = Delay::new();
        let parts = run_device(&mut device, 0, &signal, block, &|_| 110.0, &lane).0;
        for (frame, (a, b)) in whole.iter().zip(&parts).enumerate() {
            assert!(
                (a - b).abs() < 1e-5,
                "delay, block {block}, frame {frame}: {a} against {b}"
            );
        }
    }
    let points = [(0u32, 0.3f32), (5_000, 0.3), (5_001, 0.6), (9_000, 0.2)];
    let lane = move |done: usize, n: usize| {
        table_block(reverb::TABLE, n, &[]).lane(reverb::DECAY, &window(&points, done, n))
    };
    let mut whole_device = Reverb::new();
    let whole = run_device(&mut whole_device, 0, &signal, 64, &steady_120, &lane).0;
    for block in [97usize, 128, 512, 1_024] {
        let mut device = Reverb::new();
        let parts = run_device(&mut device, 0, &signal, block, &steady_120, &lane).0;
        for (frame, (a, b)) in whole.iter().zip(&parts).enumerate() {
            assert!(
                (a - b).abs() < 1e-5,
                "reverb, block {block}, frame {frame}: {a} against {b}"
            );
        }
    }
}

#[test]
fn a_block_with_automation_never_touches_the_heap() {
    use lumen_rt::{count_rt_violations, guard_is_active};
    assert!(guard_is_active(), "the guarded allocator is installed");
    let mut left = vec![0.25; 4_096];
    let mut right = vec![0.25; 4_096];
    let mut delay_device = Delay::new();
    let mut reverb_device = Reverb::new();
    // The blocks are built before the guard: what is guarded is the devices'
    // own work while they run.
    let delay_events = table_block(delay::TABLE, 256, &[])
        .lane(delay::FEEDBACK, &[(0, 0.2), (120, 0.9)])
        .build();
    let reverb_events = table_block(reverb::TABLE, 256, &[])
        .lane(reverb::DECAY, &[(0, 0.2), (120, 0.9)])
        .build();
    let ((), heap) = count_rt_violations(|| {
        for (device, events) in [
            (&mut delay_device as &mut dyn Effect, &delay_events),
            (&mut reverb_device as &mut dyn Effect, &reverb_events),
        ] {
            for k in 0..16 {
                let at = k * 256;
                let block = Block {
                    rate: RATE,
                    start: at as i64,
                    bpm: 120.0,
                    law: lumen_core::pan::PanLaw::Balance,
                    events,
                };
                device.process(
                    &block,
                    Stereo {
                        left: &mut left[at..at + 256],
                        right: &mut right[at..at + 256],
                    },
                    None,
                );
            }
        }
    });
    assert_eq!(heap, 0, "a device allocated on the audio thread");
}

#[test]
fn a_new_device_reports_its_description_s_tail_before_any_block() {
    // The unit the engine reads the running instance's tail in after a Stop:
    // before a block it is the reference tempo and rate, as the description.
    for name in [delay::NAME, reverb::NAME] {
        let table = lumen_devices::effect_params(name).unwrap();
        let defaults: Vec<(u32, f32)> = table
            .iter()
            .map(|p| (p.id, p.normalised(p.default)))
            .collect();
        let device = lumen_devices::effect(name).unwrap();
        assert_eq!(
            device.tail(),
            lumen_devices::effect_tail_at(name, &defaults, 120.0, 48_000).unwrap()
        );
        assert!(device.tail() > 0, "{name} has a tail at its defaults");
    }
}
