//! The metronome, the test tone and panic (M2 slice 7).

use lumen_core::meter::Meter;
use lumen_core::time::Ticks;
use lumen_engine::{
    ClickSound, Command, Engine, EngineHandle, MetronomeSettings, Region, Timeline, engine,
};

const RATE: u32 = 48_000;
/// One beat at 120 BPM.
const BEAT: usize = 24_000;

fn metronome(timeline: Timeline, volume: f32, sound: ClickSound) -> (EngineHandle, Engine<()>) {
    let (mut handle, engine) = engine(timeline, (), RATE);
    handle
        .send(Command::SetMetronome(MetronomeSettings {
            enabled: true,
            volume,
            sound,
            channel: 0,
        }))
        .unwrap();
    (handle, engine)
}

/// Renders `frames` (left channel) in blocks of `block`.
fn run(engine: &mut Engine<()>, frames: usize, block: usize) -> Vec<f32> {
    let mut data = vec![0.0; 2 * block];
    let mut out = Vec::with_capacity(frames);
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        engine.process(RATE, &mut data[..2 * n], 2, n);
        out.extend_from_slice(&data[..n]);
        done += n;
    }
    out
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |m, s| m.max(s.abs()))
}

/// Where each click begins: the first sound after at least `gap` silent
/// frames. (The beep starts at sin(0) = 0, so it is heard from the frame
/// after the beat.)
fn onsets(out: &[f32], gap: usize) -> Vec<usize> {
    let mut quiet = gap;
    let mut found = Vec::new();
    for (i, &s) in out.iter().enumerate() {
        if s == 0.0 {
            quiet += 1;
        } else {
            if quiet >= gap {
                found.push(i);
            }
            quiet = 0;
        }
    }
    found
}

#[test]
fn clicks_start_on_the_exact_beat_at_every_block_size() {
    for block in [1, 64, 333, 4_096] {
        let (mut handle, mut engine) = metronome(Timeline::default(), 0.5, ClickSound::Beep);
        handle.send(Command::Play).unwrap();
        let out = run(&mut engine, 5 * BEAT, block);
        let expected: Vec<usize> = (0..5).map(|b| b * BEAT + 1).collect();
        assert_eq!(onsets(&out, 1_000), expected, "block {block}");
    }
}

#[test]
fn the_downbeat_is_accented_and_volume_scales() {
    let (mut handle, mut engine) = metronome(Timeline::default(), 0.5, ClickSound::Beep);
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 5 * BEAT, 512);
    let click = |beat: usize| peak(&out[beat * BEAT..beat * BEAT + 2_400]);
    assert!(click(0) > click(1) * 1.5, "{} vs {}", click(0), click(1));
    assert!((click(1) - click(2)).abs() < 1e-3);
    assert!((click(4) - click(0)).abs() < 1e-3, "bar 2 is accented too");

    let (mut handle, mut engine) = metronome(Timeline::default(), 0.25, ClickSound::Beep);
    handle.send(Command::Play).unwrap();
    let quiet = run(&mut engine, BEAT, 512);
    let ratio = peak(&quiet[..2_400]) / click(0);
    assert!((ratio - 0.5).abs() < 0.01, "{ratio}");
}

#[test]
fn accents_follow_the_time_signature() {
    // 7/8 at 120 BPM: a click every eighth (12,000 samples), accented every 7.
    let seven_eight = Meter {
        numerator: 7,
        denominator: 8,
    };
    let (mut handle, mut engine) = metronome(
        Timeline::constant(120.0, seven_eight),
        0.5,
        ClickSound::Wood,
    );
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 15 * 12_000, 256);
    let loud: Vec<usize> = (0..15)
        .filter(|&b| peak(&out[b * 12_000..b * 12_000 + 2_400]) > 0.4)
        .collect();
    assert_eq!(loud, vec![0, 7, 14]);
}

#[test]
fn clicks_stay_on_the_beat_across_a_loop_wrap() {
    let (mut handle, mut engine) = metronome(Timeline::default(), 0.5, ClickSound::Beep);
    // Loop the first bar and a half: the wrap lands mid-bar, between beats.
    handle
        .send(Command::SetLoop(Region::new(
            Ticks::ZERO,
            Ticks::quarters(6),
            true,
        )))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let out = run(&mut engine, 12 * BEAT, 300);
    // Beats 0–5, then the wrap at 6 beats starts beat 0 again, and so on.
    let expected: Vec<usize> = (0..12).map(|b| b * BEAT + 1).collect();
    assert_eq!(onsets(&out, 1_000), expected);
}

#[test]
fn a_ringing_click_is_not_cut_off_by_stop() {
    let (mut handle, mut engine) = metronome(Timeline::default(), 0.5, ClickSound::Beep);
    handle.send(Command::Play).unwrap();
    run(&mut engine, 100, 100);
    handle.send(Command::Stop).unwrap();
    let after = run(&mut engine, 2_400, 100);
    assert!(peak(&after[..200]) > 0.05, "the click kept ringing");
    assert!(peak(&after[2_300..]) < 1e-3, "and then decayed");
}

#[test]
fn the_metronome_off_is_silent() {
    let (mut handle, mut engine) = engine(Timeline::default(), (), RATE);
    handle.send(Command::Play).unwrap();
    assert_eq!(peak(&run(&mut engine, 3 * BEAT, 512)), 0.0);
}

#[test]
fn the_test_tone_is_1_khz_at_minus_20_dbfs_even_when_stopped() {
    let (mut handle, mut engine) = engine(Timeline::default(), (), RATE);
    handle.send(Command::TestTone(true)).unwrap();
    let out = run(&mut engine, RATE as usize, 480);
    let settled = &out[RATE as usize / 10..];
    assert!((peak(settled) - 0.1).abs() < 1e-3, "{}", peak(settled));
    let rising = settled
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    let hz = rising as f64 / (settled.len() as f64 / f64::from(RATE));
    assert!((hz - 1_000.0).abs() < 2.0, "{hz} Hz");
    // It ramps in over 5 ms rather than starting with a click.
    assert!(peak(&out[..24]) < 0.02);

    handle.send(Command::TestTone(false)).unwrap();
    let off = run(&mut engine, 4_800, 480);
    assert!(peak(&off[..48]) > 0.0, "ramps out");
    assert_eq!(peak(&off[480..]), 0.0);
}

#[test]
fn panic_silences_everything_at_once() {
    let (mut handle, mut engine) = metronome(Timeline::default(), 0.5, ClickSound::Beep);
    handle.send(Command::TestTone(true)).unwrap();
    handle.send(Command::Play).unwrap();
    run(&mut engine, BEAT + 500, 256);
    handle.send(Command::Panic).unwrap();
    let after = run(&mut engine, 1_024, 256);
    assert_eq!(peak(&after), 0.0);
    assert!(!handle.transport().playing);
}

#[test]
fn the_metronome_plays_on_the_cue_pair_when_asked_and_the_main_one_otherwise() {
    // M4 slice 4 (the owner's D6): Main or Cue; never through the master.
    let (mut handle, mut player) = engine(Timeline::default(), (), RATE);
    handle
        .send(Command::SetMetronome(MetronomeSettings {
            enabled: true,
            volume: 0.5,
            sound: ClickSound::Beep,
            channel: 2,
        }))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let frames = 4_800;
    let mut data = vec![0.0; 4 * frames];
    player.process(RATE, &mut data, 4, frames);
    let channel = |c: usize| peak(&data[c * frames..(c + 1) * frames]);
    assert!(channel(2) > 0.1 && channel(3) > 0.1, "on the cue pair");
    assert_eq!((channel(0), channel(1)), (0.0, 0.0), "not on the main pair");
    // A device with only the main pair: it falls back there.
    let (mut handle, mut player) = engine(Timeline::default(), (), RATE);
    handle
        .send(Command::SetMetronome(MetronomeSettings {
            enabled: true,
            volume: 0.5,
            sound: ClickSound::Beep,
            channel: 2,
        }))
        .unwrap();
    handle.send(Command::Play).unwrap();
    let mut data = vec![0.0; 2 * frames];
    player.process(RATE, &mut data, 2, frames);
    assert!(peak(&data[..frames]) > 0.1);
}
