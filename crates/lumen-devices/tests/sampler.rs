//! Shutter, the built-in sampler (M8 slice B5): what it plays at a sample's
//! root key, how far a pitched render is from an independent reference, that
//! it plays the same frames at every block size, and that it allocates
//! nothing and frees nothing while it runs.

use std::sync::Arc;

use lumen_core::midi::{Event, Msg};
use lumen_devices::sampler::{
    self, ATTACK, CUTOFF, FILTER, Face, NAME, PITCH, RELEASE, RESONANCE, Sample, Sampler, Sound,
    TABLE, VELOCITY,
};
use lumen_devices::{Instrument, ParamBlock, params};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;

/// The most frames one call renders: the engine's chunk.
const MOST: usize = 256;

fn at(id: u32, value: f32) -> f32 {
    params::find(TABLE, id)
        .expect("a parameter of the table")
        .normalised(value)
}

/// A block naming each parameter at rest at `set`'s values, everything else
/// at its default.
fn block(frames: usize, set: &[(u32, f32)]) -> ParamBlock {
    let mut events = ParamBlock::new();
    events.start(frames);
    for info in TABLE {
        let value = set
            .iter()
            .find(|(id, _)| *id == info.id)
            .map_or_else(|| info.normalised(info.default), |&(_, value)| value);
        events.open(info.id);
        events.point(0, f64::from(value));
    }
    events
}

fn note_on(key: u8, velocity: u8, offset: u32) -> Event {
    Event {
        offset,
        msg: Msg::note_on(0, key, velocity),
    }
}

fn note_off(key: u8, offset: u32) -> Event {
    Event {
        offset,
        msg: Msg::note_off(0, key),
    }
}

/// A sample that is its own index, so a render can be read frame by frame:
/// frame `k` holds `k * step`.
fn ramp(frames: usize, step: f32) -> Arc<Sample> {
    Arc::new(Sample::mono(
        RATE,
        (0..frames).map(|k| k as f32 * step).collect(),
    ))
}

/// A sound whose One shot face plays `sample` on `root`.
fn one_shot(sample: Arc<Sample>, root: u8) -> Arc<Sound> {
    Arc::new(Sound {
        face: Face::OneShot,
        one_shot: Some(sampler::Region::whole(sample, root)),
        ..Sound::default()
    })
}

/// Renders `frames` in blocks of `size` (at most the engine's chunk),
/// striking `key` at the first frame and holding it to the end.
fn render(
    device: &mut Sampler,
    frames: usize,
    size: usize,
    key: u8,
    velocity: u8,
    set: &[(u32, f32)],
) -> (Vec<f32>, Vec<f32>) {
    let size = size.clamp(1, MOST);
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut at_frame = 0;
    while at_frame < frames {
        let n = size.min(frames - at_frame);
        let events: Vec<Event> = if at_frame == 0 {
            vec![note_on(key, velocity, 0)]
        } else {
            Vec::new()
        };
        device.automate(&block(n, set));
        device.render(
            RATE,
            &events,
            &mut left[at_frame..at_frame + n],
            &mut right[at_frame..at_frame + n],
        );
        at_frame += n;
    }
    (left, right)
}

/// Renders `blocks` blocks with no events.
fn settle(device: &mut Sampler, blocks: usize, set: &[(u32, f32)]) -> Vec<f32> {
    let mut left = vec![0.0; MOST];
    let mut right = vec![0.0; MOST];
    let mut heard = Vec::new();
    for _ in 0..blocks {
        device.automate(&block(MOST, set));
        device.render(RATE, &[], &mut left, &mut right);
        heard.extend_from_slice(&left);
    }
    heard
}

/// A sample played at its root key is the file's own frames, to the bit.
#[test]
fn a_sample_at_its_root_key_is_the_file_sample_for_sample() {
    let file = ramp(480, 0.001);
    let mut device = Sampler::new(one_shot(file.clone(), 60));
    let (left, right) = render(&mut device, 480, 128, 60, 100, &[]);
    for frame in 0..480 {
        assert_eq!(
            left[frame],
            file.at(0, frame as isize),
            "frame {frame} is the file's"
        );
        assert_eq!(right[frame], left[frame], "a mono file on both sides");
    }
}

/// The same for a stereo file, whose sides stay apart.
#[test]
fn a_stereo_file_keeps_its_two_sides() {
    let left: Vec<f32> = (0..256).map(|k| k as f32 / 512.0).collect();
    let right: Vec<f32> = (0..256).map(|k| -(k as f32) / 731.0).collect();
    let mut data = left.clone();
    data.extend_from_slice(&right);
    let sample = Arc::new(Sample::new(RATE, 2, 256, data).expect("two sides"));
    let mut device = Sampler::new(one_shot(sample, 60));
    let (l, r) = render(&mut device, 256, 256, 60, 127, &[]);
    assert_eq!(l, left);
    assert_eq!(r, right);
}

/// A tone of `frames` frames at `rate`, quiet enough to read frame by frame.
fn tone(frames: usize, rate: u32) -> Arc<Sample> {
    Arc::new(Sample::mono(
        rate,
        (0..frames)
            .map(|k| {
                let x = k as f64 * 0.01;
                // A tone well under Nyquist at either pitch: it is the reading
                // that is compared, not the aliasing above it.
                (x.sin() * 0.5) as f32
            })
            .collect(),
    ))
}

/// The reference the pitched renders are compared with: the same 4-point
/// Catmull-Rom in `f64` over `sample`, written independently of the device's
/// own reading. `start` and `end` are the region, `reversed` reads it
/// backwards, and `ratio` is frames of the region per frame of output.
struct Reference {
    sample: Arc<Sample>,
    start: usize,
    end: usize,
    reversed: bool,
    ratio: f64,
}

impl Reference {
    /// The value at output frame `frame`, the way a person would write it.
    fn at(&self, frame: usize) -> f64 {
        // A reversed region runs backwards through its own frames.
        let at = frame as f64 * self.ratio;
        let index = if self.reversed {
            (self.end as f64 - 1.0) - at
        } else {
            self.start as f64 + at
        };
        let base = index.floor();
        let into = index - base;
        let read = |k: isize| f64::from(self.sample.at(0, k));
        let (p0, p1, p2, p3) = (
            read(base as isize - 1),
            read(base as isize),
            read(base as isize + 1),
            read(base as isize + 2),
        );
        0.5 * ((2.0 * p1)
            + (-p0 + p2) * into
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * into * into
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * into * into * into)
    }
}

/// A pitched render is within the resampler's tested error of an independent
/// `f64` reference: the same sample read at the same rate, worked out here
/// without the device's code.
#[test]
fn a_pitched_render_follows_the_resampler_s_reference() {
    let slices = [(4_800, RATE)];
    for (frames, rate) in slices {
        let sample = tone(frames, rate);
        let end = sample.frames();
        for semitones in [1, 5, 7, 12, -5] {
            let mut device = Sampler::new(one_shot(sample.clone(), 60));
            let key = (60 + semitones) as u8;
            let (left, _) = render(&mut device, 3_000, 256, key, 100, &[]);
            let reference = Reference {
                sample: Arc::clone(&sample),
                start: 0,
                end,
                reversed: false,
                ratio: (f64::from(semitones) / 12.0).exp2(),
            };
            for (frame, heard) in left.iter().enumerate() {
                let error = (f64::from(*heard) - reference.at(frame)).abs();
                assert!(
                    error < 1e-6,
                    "{semitones} st at {rate} Hz, frame {frame}: {error}"
                );
            }
            // The first frame is the region's first frame itself.
            assert_eq!(
                left[0],
                sample.at(0, 0),
                "{semitones} st: the first frame is the file's"
            );
        }
    }
}

/// The same, from a region that starts inside the file: the first frame is
/// the region's first frame, and the rest follows it.
#[test]
fn a_pitched_render_of_a_region_starting_inside_the_file_follows_the_reference() {
    let sample = tone(4_800, RATE);
    let start = 1_001;
    let sound = Arc::new(Sound {
        face: Face::OneShot,
        one_shot: Some(sampler::Region {
            start,
            ..sampler::Region::whole(Arc::clone(&sample), 60)
        }),
        ..Sound::default()
    });
    for semitones in [2, 7, -3] {
        let mut device = Sampler::new(Arc::clone(&sound));
        let key = (60 + semitones) as u8;
        let (left, _) = render(&mut device, 2_000, 256, key, 100, &[]);
        let reference = Reference {
            sample: Arc::clone(&sample),
            start,
            end: sample.frames(),
            reversed: false,
            ratio: (f64::from(semitones) / 12.0).exp2(),
        };
        assert_eq!(
            left[0],
            sample.at(0, start as isize),
            "{semitones} st: the region's first frame is the file's at {start}"
        );
        for (frame, heard) in left.iter().enumerate() {
            let error = (f64::from(*heard) - reference.at(frame)).abs();
            assert!(
                error < 1e-6,
                "{semitones} st from {start}, frame {frame}: {error}"
            );
        }
    }
}

/// A reversed region pitched away from its root: the reference reads the
/// region backwards from its last frame.
#[test]
fn a_reversed_pitched_render_follows_the_reference() {
    let sample = tone(4_800, RATE);
    let end = sample.frames();
    let sound = Arc::new(Sound {
        face: Face::OneShot,
        one_shot: Some(sampler::Region {
            reversed: true,
            ..sampler::Region::whole(Arc::clone(&sample), 60)
        }),
        ..Sound::default()
    });
    for semitones in [0, 5, -5] {
        let mut device = Sampler::new(Arc::clone(&sound));
        let key = (60 + semitones) as u8;
        let (left, _) = render(&mut device, 2_000, 256, key, 100, &[]);
        let reference = Reference {
            sample: Arc::clone(&sample),
            start: 0,
            end,
            reversed: true,
            ratio: (f64::from(semitones) / 12.0).exp2(),
        };
        assert_eq!(
            left[0],
            sample.at(0, end as isize - 1),
            "{semitones} st: a reversed region starts at the file's last frame"
        );
        for (frame, heard) in left.iter().enumerate() {
            let error = (f64::from(*heard) - reference.at(frame)).abs();
            assert!(
                error < 1e-6,
                "{semitones} st reversed, frame {frame}: {error}"
            );
        }
    }
}

/// A file whose rate is not the render's: the region is played at the file's
/// speed, so the reading runs at `file rate / render rate` per frame.
#[test]
fn a_sample_at_another_rate_is_read_at_its_own_speed() {
    let file_rate = 24_000;
    let sample = tone(2_400, file_rate);
    let end = sample.frames();
    for semitones in [0, 7] {
        let mut device = Sampler::new(one_shot(Arc::clone(&sample), 60));
        let key = (60 + semitones) as u8;
        let (left, _) = render(&mut device, 900, 256, key, 100, &[]);
        let reference = Reference {
            sample: Arc::clone(&sample),
            start: 0,
            end,
            reversed: false,
            ratio: f64::from(file_rate) / f64::from(RATE) * (f64::from(semitones) / 12.0).exp2(),
        };
        assert_eq!(
            left[0],
            sample.at(0, 0),
            "{semitones} st at {file_rate} Hz: the first frame is the file's"
        );
        for (frame, heard) in left.iter().enumerate() {
            let error = (f64::from(*heard) - reference.at(frame)).abs();
            assert!(
                error < 1e-6,
                "{semitones} st at {file_rate} Hz, frame {frame}: {error}"
            );
        }
    }
}

/// E6: the same frames at every block size, 64, 128, 512 (taken as two of
/// the engine's chunks), 1024 and the odd 97.
#[test]
fn the_same_audio_at_any_block_size() {
    let sample = Arc::new(Sample::mono(
        RATE,
        (0..8_000)
            .map(|k| {
                let x = k as f64 * 0.007;
                (x.sin() * 0.4 + (x * 3.0).sin() * 0.2) as f32
            })
            .collect(),
    ));
    let frames = 5_000;
    let set = [(PITCH, at(PITCH, 3.5)), (ATTACK, at(ATTACK, 0.01))];
    let whole = {
        let mut device = Sampler::new(one_shot(sample.clone(), 60));
        let (left, _) = render(&mut device, frames, MOST, 63, 100, &set);
        left
    };
    for size in [64usize, 128, 512, 1024, 97] {
        let mut device = Sampler::new(one_shot(sample.clone(), 60));
        let (left, _) = render(&mut device, frames, size, 63, 100, &set);
        for (frame, (a, b)) in whole.iter().zip(&left).enumerate() {
            assert!(
                (a - b).abs() < 1e-6,
                "size {size} frame {frame}: {a} against {b}"
            );
        }
    }
}

/// The allocation guard: playing, choking, swapping the sound and letting it
/// go allocates and frees nothing on the audio thread, even with a sample far
/// larger than one block.
#[test]
fn the_render_never_touches_the_heap() {
    assert!(
        lumen_rt::guard_is_active(),
        "the guarded allocator is installed"
    );
    // A large sample: five seconds of stereo, about 2 MB.
    let frames = 5 * RATE as usize;
    let mut data = vec![0.0; frames * 2];
    for k in 0..frames {
        data[k] = (k as f32 * 0.001).sin() * 0.3;
        data[frames + k] = (k as f32 * 0.002).cos() * 0.3;
    }
    let big = Arc::new(Sample::new(RATE, 2, frames, data).expect("a stereo sample"));
    let mut sound = Sound {
        face: Face::Pads,
        ..Sound::default()
    };
    for note in [36u8, 38, 42, 46] {
        let mut pad = sampler::Pad::new(note, Some(sampler::Region::whole(big.clone(), note)));
        pad.choke = (note >= 42).then_some(1);
        sound.pads.push(pad);
    }
    // The sound the running one is replaced with: built here, off the audio
    // thread, as an edit does.
    let mut swapped = Sound {
        face: Face::Pads,
        ..Sound::default()
    };
    swapped.pads.push(sampler::Pad::new(
        36,
        Some(sampler::Region::whole(big.clone(), 36)),
    ));
    let replacement = Arc::new(swapped);
    let mut device = Sampler::new(Arc::new(sound));
    let mut left = vec![0.0; MOST];
    let mut right = vec![0.0; MOST];
    let events = block(MOST, &[(PITCH, at(PITCH, 2.0)), (FILTER, at(FILTER, 1.0))]);
    let (violations_sound, violations) = lumen_rt::count_rt_violations(|| {
        device.automate(&events);
        device.render(
            RATE,
            &[
                note_on(36, 100, 0),
                note_on(42, 100, 0),
                note_on(46, 90, 40),
            ],
            &mut left,
            &mut right,
        );
        // A note-off chokes the hats; the sound is then replaced whole and
        // plays on, which is the path a sample edit takes.
        device.render(RATE, &[note_off(42, 0)], &mut left, &mut right);
        let old = device.load_sound(Arc::clone(&replacement));
        assert!(old.is_some(), "the sound it played came back");
        for _ in 0..8 {
            device.automate(&events);
            device.render(RATE, &[], &mut left, &mut right);
        }
        device.silence();
        // The sound it played goes back to the interface, which frees it:
        // dropping it here would be the audio thread freeing a sound.
        old
    });
    assert_eq!(violations, 0, "the audio thread touched the heap");
    drop(violations_sound);
}

/// The amp envelope: a note released takes the release the table gives it,
/// and then stops sounding.
#[test]
fn a_note_fades_over_its_release_and_then_stops_sounding() {
    let set = [(RELEASE, at(RELEASE, 0.1))];
    // A constant sample, so what is heard is the envelope itself.
    let dc = Arc::new(Sample::mono(RATE, vec![1.0; 48_000]));
    let mut device = Sampler::new(one_shot(dc, 60));
    // A knob moves over 20 ms: let the release arrive before the note.
    settle(&mut device, 5, &set);
    let mut left = vec![0.0; MOST];
    let mut right = vec![0.0; MOST];
    device.automate(&block(MOST, &set));
    device.render(RATE, &[note_on(60, 100, 0)], &mut left, &mut right);
    assert_eq!(left[0], 1.0, "at full level at once");
    assert_eq!(left[255], 1.0, "and holding");
    device.render(RATE, &[note_off(60, 0)], &mut left, &mut right);
    // 0.1 s at 48 kHz is 4,800 frames: the level falls in a straight line.
    assert!((left[0] - 1.0).abs() < 0.001, "the release starts at 1");
    assert!(left[255] < 0.96 && left[255] > 0.93, "{}", left[255]);
    let heard = settle(&mut device, 20, &set);
    assert!(heard.iter().any(|v| v.abs() > 0.0));
    assert!(!device.active(), "the release has run out");
    let after = settle(&mut device, 1, &set);
    assert!(after.iter().all(|v| *v == 0.0), "and it is silent");
}

/// A looped One shot returns to the start of its region and keeps sounding;
/// an unlooped one stops at the end of it.
#[test]
fn a_looped_sample_is_heard_past_its_end_and_stops_with_the_note() {
    let sample = ramp(100, 0.001);
    let looped = Sound {
        face: Face::OneShot,
        one_shot: Some(sampler::Region {
            looping: true,
            ..sampler::Region::whole(sample.clone(), 60)
        }),
        ..Sound::default()
    };
    let mut device = Sampler::new(Arc::new(looped));
    let (left, _) = render(&mut device, 400, 128, 60, 100, &[]);
    // The region repeats every 100 frames.
    assert_eq!(left[0], 0.0);
    assert_eq!(left[150], left[50]);
    assert_eq!(left[250], left[50]);
    let once = Sound {
        face: Face::OneShot,
        one_shot: Some(sampler::Region::whole(sample, 60)),
        ..Sound::default()
    };
    let mut device = Sampler::new(Arc::new(once));
    let (left, _) = render(&mut device, 400, 128, 60, 100, &[]);
    assert_eq!(left[99], 99.0 * 0.001_f32);
    assert_eq!(left[100], 0.0, "past the end, nothing");
}

/// A reversed region plays its frames backwards, and its first frame is the
/// file's last.
#[test]
fn a_reversed_sample_plays_backwards() {
    let sample = ramp(100, 0.001);
    let sound = Sound {
        face: Face::OneShot,
        one_shot: Some(sampler::Region {
            reversed: true,
            ..sampler::Region::whole(sample.clone(), 60)
        }),
        ..Sound::default()
    };
    let mut device = Sampler::new(Arc::new(sound));
    let (left, _) = render(&mut device, 100, 100, 60, 100, &[]);
    assert_eq!(left[0], sample.at(0, 99), "the last frame first");
    assert_eq!(left[99], sample.at(0, 0));
}

/// Vel>Level sets a note's gain from its velocity; at the default it does
/// not.
#[test]
fn velocity_sets_the_level_when_it_is_told_to() {
    let sample = ramp(64, 0.01);
    let level = |velocity: u8, amount: f32| {
        let set = [(VELOCITY, at(VELOCITY, amount))];
        let mut device = Sampler::new(one_shot(sample.clone(), 60));
        // A knob moves over 20 ms: let the value arrive before the note.
        settle(&mut device, 5, &set);
        let (left, _) = render(&mut device, 64, 64, 60, velocity, &set);
        left[10]
    };
    let full = sample.at(0, 10);
    assert_eq!(level(64, 0.0), full, "the default ignores velocity");
    assert_eq!(level(127, 100.0), full);
    let half = level(64, 100.0);
    assert!(
        (half - full * 64.0 / 127.0).abs() < 1e-6,
        "{half} against {}",
        full * 64.0 / 127.0
    );
}

/// The filter takes the sound when it is on, and is out of the path when it
/// is off.
#[test]
fn the_filter_only_touches_the_sound_when_it_is_on() {
    let tone = Arc::new(Sample::mono(
        RATE,
        (0..4_800)
            .map(|k| (k as f64 * 0.3).sin() as f32 * 0.5)
            .collect(),
    ));
    let with = |mode: f32| {
        let set = [
            (FILTER, at(FILTER, mode)),
            (CUTOFF, at(CUTOFF, 400.0)),
            (RESONANCE, at(RESONANCE, 30.0)),
        ];
        let mut device = Sampler::new(one_shot(tone.clone(), 60));
        let (left, _) = render(&mut device, 4_800, MOST, 60, 100, &set);
        left
    };
    let off = with(0.0);
    assert_eq!(off[1_000], tone.at(0, 1_000), "off is the file itself");
    let low = with(1.0);
    assert!(low[1_000] != off[1_000], "the low pass changes it");
    // A 400 Hz low pass passes a slow signal and takes the fast one.
    let energy = |signal: &[f32]| {
        signal
            .iter()
            .map(|v| f64::from(*v) * f64::from(*v))
            .sum::<f64>()
    };
    assert!(energy(&low) < energy(&off) * 0.5, "the high tone is taken");
    // A high pass keeps the fast one and takes the low.
    let high = with(2.0);
    assert!(high[1_000] != off[1_000]);
}

/// Each pad's output pair plays on its own pair, and the pads on the track's
/// own output stay there.
#[test]
fn a_pad_routed_to_a_pair_plays_on_it_and_not_on_the_track() {
    let sample = ramp(256, 0.01);
    let mut sound = Sound {
        face: Face::Pads,
        ..Sound::default()
    };
    let mut kick = sampler::Pad::new(36, Some(sampler::Region::whole(sample.clone(), 36)));
    kick.output = 2;
    sound.pads.push(kick);
    sound.pads.push(sampler::Pad::new(
        38,
        Some(sampler::Region::whole(sample, 38)),
    ));
    let mut device = Sampler::new(Arc::new(sound));
    let mut left = vec![0.0; MOST];
    let mut right = vec![0.0; MOST];
    device.automate(&block(MOST, &[]));
    device.render(RATE, &[note_on(36, 100, 0)], &mut left, &mut right);
    assert!(left.iter().all(|v| *v == 0.0), "nothing on the main output");
    let (mut pair_left, mut pair_right) = (vec![0.0; MOST], vec![0.0; MOST]);
    assert!(device.output(2, &mut pair_left, &mut pair_right));
    assert!(pair_left.iter().any(|v| *v != 0.0), "the kick is on pair 2");
    assert!(device.output(1, &mut pair_left, &mut pair_right));
    assert!(pair_left.iter().all(|v| *v == 0.0), "pair 1 is silent");
    assert!(!device.output(0, &mut pair_left, &mut pair_right));
    // The snare is on the track's own output.
    device.render(RATE, &[note_on(38, 100, 0)], &mut left, &mut right);
    assert!(left.iter().any(|v| *v != 0.0));
}

/// A choke group: a new hit of one pad fades the others in the group over
/// their choke fade, and leaves a pad outside the group alone. Each pad's
/// sample is a different constant, so the output says which are sounding.
#[test]
fn a_choke_fades_the_group_and_leaves_the_rest_alone() {
    let constant = |value: f32| Arc::new(Sample::mono(RATE, vec![value; 48_000]));
    let mut sound = Sound {
        face: Face::Pads,
        ..Sound::default()
    };
    let mut open = sampler::Pad::new(46, Some(sampler::Region::whole(constant(0.5), 46)));
    open.choke = Some(1);
    let mut closed = sampler::Pad::new(42, Some(sampler::Region::whole(constant(0.125), 42)));
    closed.choke = Some(1);
    sound.pads.push(closed);
    sound.pads.push(open);
    sound.pads.push(sampler::Pad::new(
        36,
        Some(sampler::Region::whole(constant(0.25), 36)),
    ));
    let mut device = Sampler::new(Arc::new(sound));
    let mut left = vec![0.0; MOST];
    let mut right = vec![0.0; MOST];
    device.automate(&block(MOST, &[]));
    device.render(
        RATE,
        &[note_on(36, 100, 0), note_on(46, 100, 0)],
        &mut left,
        &mut right,
    );
    assert!((left[200] - 0.75).abs() < 1e-6, "{}", left[200]);
    assert_eq!(device.sounding(), 2);
    // The closed hat at frame 64 chokes the open one: it fades out over 5 ms
    // (240 frames), so by the end of the block the output is the kick and the
    // closed hat, and the kick, in no group, plays on throughout.
    device.automate(&block(MOST, &[]));
    device.render(RATE, &[note_on(42, 100, 64)], &mut left, &mut right);
    assert!(
        (left[63] - 0.75).abs() < 1e-6,
        "before the choke: {}",
        left[63]
    );
    assert!(
        (left[64 + 120] - (0.25 + 0.125 + 0.25)).abs() < 0.005,
        "halfway through the fade: {}",
        left[64 + 120]
    );
    assert!(
        (left[255] - 0.475).abs() < 0.005,
        "the fade runs to the end of the block: {}",
        left[255]
    );
    // 240 frames from frame 64 is frame 304: the next block has it gone.
    let heard = settle(&mut device, 1, &[]);
    assert!(
        (heard[MOST - 1] - 0.375).abs() < 1e-6,
        "after it: {}",
        heard[MOST - 1]
    );
    assert_eq!(device.sounding(), 2, "the open hat went, the rest stayed");
}

/// A note-off ends a pad in a choke group but not one outside it, which plays
/// to the end of its sample as a drum sampler does.
#[test]
fn a_note_off_chokes_a_grouped_pad_and_lets_an_ungrouped_one_ring() {
    let constant = |value: f32| Arc::new(Sample::mono(RATE, vec![value; 3_000]));
    let mut sound = Sound {
        face: Face::Pads,
        ..Sound::default()
    };
    let mut hat = sampler::Pad::new(42, Some(sampler::Region::whole(constant(0.5), 42)));
    hat.choke = Some(1);
    sound.pads.push(hat);
    sound.pads.push(sampler::Pad::new(
        36,
        Some(sampler::Region::whole(constant(0.25), 36)),
    ));
    let mut device = Sampler::new(Arc::new(sound));
    let mut left = vec![0.0; MOST];
    let mut right = vec![0.0; MOST];
    device.automate(&block(MOST, &[]));
    device.render(
        RATE,
        &[note_on(36, 100, 0), note_on(42, 100, 0)],
        &mut left,
        &mut right,
    );
    assert!((left[100] - 0.75).abs() < 1e-6);
    for _ in 0..2 {
        device.automate(&block(MOST, &[]));
        device.render(
            RATE,
            &[note_off(36, 0), note_off(42, 0)],
            &mut left,
            &mut right,
        );
    }
    assert_eq!(device.sounding(), 1, "only the kick is left");
    assert!((left[255] - 0.25).abs() < 1e-6, "{}", left[255]);
    // 3,000 frames are 12 blocks: the kick rings on to its end.
    assert!(device.active());
    let heard = settle(&mut device, 1, &[]);
    assert!((heard[0] - 0.25).abs() < 1e-6);
}

/// Pan sends a pad to a side; at the centre both sides are the file's own.
#[test]
fn a_pad_pans_between_the_sides() {
    let sample = ramp(64, 0.01);
    let mut sound = Sound {
        face: Face::Pads,
        ..Sound::default()
    };
    let mut pad = sampler::Pad::new(36, Some(sampler::Region::whole(sample, 36)));
    pad.pan = -1.0;
    sound.pads.push(pad);
    let mut device = Sampler::new(Arc::new(sound));
    let mut left = vec![0.0; 64];
    let mut right = vec![0.0; 64];
    device.automate(&block(64, &[]));
    device.render(RATE, &[note_on(36, 100, 0)], &mut left, &mut right);
    assert!(left[10] > 0.0);
    assert_eq!(right[10], 0.0, "hard left");
}

/// The zones answer a range of keys and velocities.
#[test]
fn a_zone_plays_the_keys_and_velocities_it_covers() {
    let low = ramp(256, 0.001);
    let high = ramp(256, 0.002);
    let mut sound = Sound {
        face: Face::Zones,
        ..Sound::default()
    };
    sound.zones.push(sampler::Zone {
        region: sampler::Region::whole(low.clone(), 36),
        low: 36,
        high: 47,
        low_velocity: 1,
        high_velocity: 63,
    });
    sound.zones.push(sampler::Zone {
        region: sampler::Region::whole(high.clone(), 36),
        low: 36,
        high: 47,
        low_velocity: 64,
        high_velocity: 127,
    });
    let first = |velocity: u8| {
        let mut device = Sampler::new(Arc::new(sound.clone()));
        let (left, _) = render(&mut device, 32, 32, 36, velocity, &[]);
        left[20]
    };
    assert_eq!(first(30), low.at(0, 20));
    assert_eq!(first(100), high.at(0, 20));
    // A key no zone holds plays nothing.
    let mut device = Sampler::new(Arc::new(sound));
    let (left, _) = render(&mut device, 32, 32, 60, 100, &[]);
    assert!(left.iter().all(|v| *v == 0.0));
}

/// The table is pinned by id, name and default (M8 slice B0, review L1): an
/// id is never reused and a default never silently moves.
#[test]
fn the_parameter_table_is_pinned_by_id_name_and_default() {
    let pinned: Vec<(u32, &str, f32)> = vec![
        (1, "Level", 0.0),
        (2, "Pitch", 0.0),
        (3, "Fine", 0.0),
        (4, "Attack", 0.0),
        (5, "Decay", 0.0),
        (6, "Sustain", 100.0),
        (7, "Release", 0.05),
        (8, "Filter", 0.0),
        (9, "Cutoff", 20_000.0),
        (10, "Res", 0.0),
        (11, "Vel>Level", 0.0),
    ];
    let table: Vec<(u32, &str, f32)> = TABLE
        .iter()
        .map(|info| (info.id, info.name, info.default))
        .collect();
    assert_eq!(table, pinned);
    let mut ids: Vec<u32> = TABLE.iter().map(|info| info.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), TABLE.len(), "every id is named once");
    assert_eq!(NAME, "sampler");
    // The table fits the room one block holds, as every built-in's must.
    assert!(TABLE.len() <= lumen_devices::MOST_QUEUES);
}

/// Each parameter reads in its own words and is read back.
#[test]
fn every_parameter_reads_and_is_read_back() {
    for info in TABLE {
        let text = info.text_of(info.normalised(info.default));
        assert!(!text.is_empty(), "{} says nothing", info.name);
        let read = info
            .value_of(&text)
            .unwrap_or_else(|| panic!("{}: {text:?} is not read back", info.name));
        let explained = info.value(read);
        let value = info.value(info.normalised(info.default));
        let tolerance = if info.unit == "Hz" { value * 0.02 } else { 0.6 };
        assert!(
            (explained - value).abs() <= tolerance,
            "{}: {text:?} reads as {explained}, not {value}",
            info.name
        );
    }
    assert_eq!(TABLE[7].text_of(1.0), "High pass");
    assert_eq!(TABLE[7].value_of("low pass"), Some(0.5));
}

/// A device with no sample plays nothing and asks for nothing.
#[test]
fn an_empty_sampler_is_silent_and_does_not_ask_to_ring_on() {
    let mut device = Sampler::silent();
    assert!(!device.active());
    let mut left = vec![0.0; 128];
    let mut right = vec![0.0; 128];
    device.automate(&block(128, &[]));
    device.render(RATE, &[note_on(36, 100, 0)], &mut left, &mut right);
    assert!(left.iter().all(|v| *v == 0.0));
    assert!(!device.active());
    assert!(device.sound().is_silent());
}

/// The sound is replaced whole: a note sounding keeps what it started with,
/// and the sound it played comes back for the caller to free.
#[test]
fn a_new_sound_does_not_disturb_a_note_already_sounding() {
    let first = ramp(64, 0.01);
    let second = ramp(64, 0.03);
    let mut device = Sampler::new(one_shot(first.clone(), 60));
    let mut left = vec![0.0; 64];
    let mut right = vec![0.0; 64];
    device.automate(&block(64, &[]));
    device.render(RATE, &[note_on(60, 100, 0)], &mut left, &mut right);
    let old = device
        .load_sound(one_shot(second, 60))
        .expect("the old sound");
    assert!(old.one_shot.is_some());
    device.automate(&block(64, &[]));
    device.render(RATE, &[], &mut left, &mut right);
    // The note still plays the sample it started with.
    assert_eq!(left[20], first.at(0, 64 + 20));
    // The same handle twice is not a replacement.
    assert!(device.load_sound(Arc::clone(device.sound())).is_none());
}

/// The Level knob: −20 dB is a tenth of the sound's own level (M8 slice B5,
/// review H3). The value glides to the knob's place over
/// [`params::GLIDE_SECONDS`], so the last block is what is measured.
#[test]
fn a_level_of_minus_twenty_decibels_is_a_tenth() {
    let peak = |db: f32| {
        let sample = Arc::new(Sample::mono(RATE, vec![0.5; 4_800]));
        let mut device = Sampler::new(one_shot(sample, 60));
        let set = [(sampler::LEVEL, at(sampler::LEVEL, db))];
        let (left, _) = render(&mut device, 2_048, MOST, 60, 100, &set);
        let settled = 2_048 - MOST;
        left[settled..]
            .iter()
            .fold(0.0f32, |most, value| most.max(value.abs()))
    };
    let unity = peak(0.0);
    let quiet = peak(-20.0);
    assert!((unity - 0.5).abs() < 1e-4, "unity is the sample: {unity}");
    assert!(
        (quiet / unity - 0.1).abs() < 1e-4,
        "−20 dB is a tenth: {quiet} against {unity}"
    );
}
