
use super::*;

fn sine(hz: f32, rate: f32, frames: usize, amplitude: f32) -> Vec<f32> {
    (0..frames)
        .map(|i| amplitude * (std::f32::consts::TAU * hz * i as f32 / rate).sin())
        .collect()
}

/// A sine on a bin's own frequency reads its amplitude there, and
/// nothing anywhere else (the transform and its scale are right).
#[test]
fn a_sine_on_a_bin_reads_its_amplitude_in_that_bin() {
    let rate = 48_000.0;
    // Bin 40 of 1024 over 24 kHz is 937.5 Hz.
    let bin = 40;
    let hz = bin as f32 * rate / 2.0 / BINS as f32;
    let samples = sine(hz, rate, SIZE, 0.5);
    let (mut tap, mut reader) = pair();
    reader.set_on(true);
    tap.write(&samples, &samples);
    assert_eq!(reader.read(), Read::Window);
    let spectrum = reader.spectrum();
    // 0.5 of full scale is −6 dB.
    assert!(
        (spectrum[bin] - (-6.02)).abs() < 0.2,
        "{} dB at its bin",
        spectrum[bin]
    );
    // A Hann window's main lobe is three bins wide, so its neighbours sit
    // 6 dB down and 31 dB down; nothing further away is anything at all.
    assert!(
        (spectrum[bin - 1] - (-12.0)).abs() < 1.0,
        "{}",
        spectrum[bin - 1]
    );
    for (other, db) in spectrum.iter().enumerate() {
        if other.abs_diff(bin) > 2 {
            assert!(*db < -25.0, "{other} reads {db}");
        }
    }
}

/// The transform is the one a direct sum gives: every bin to 1e-3.
#[test]
fn the_transform_is_the_one_a_direct_sum_gives() {
    let n = 64;
    let signal: Vec<f32> = (0..n)
        .map(|i| (i as f32 * 0.31).sin() + 0.4 * (i as f32 * 1.7).cos())
        .collect();
    let mut re = signal.clone();
    let mut im = vec![0.0; n];
    transform(&mut re, &mut im);
    for k in 0..n {
        let (mut want_re, mut want_im) = (0.0f32, 0.0f32);
        for (i, sample) in signal.iter().enumerate() {
            let angle = -std::f32::consts::TAU * (k * i) as f32 / n as f32;
            want_re += sample * angle.cos();
            want_im += sample * angle.sin();
        }
        assert!((re[k] - want_re).abs() < 1e-3, "bin {k} re: {}", re[k]);
        assert!((im[k] - want_im).abs() < 1e-3, "bin {k} im: {}", im[k]);
    }
}

/// A tap that nothing listens to writes nothing at all.
#[test]
fn a_tap_nothing_reads_writes_nothing() {
    let (mut tap, mut reader) = pair();
    assert!(!tap.is_on());
    tap.write(&[1.0; 64], &[1.0; 64]);
    assert_eq!(reader.read(), Read::Nothing, "nothing was written");
    reader.set_on(true);
    assert!(tap.is_on());
    tap.write(&[1.0; SIZE], &[1.0; SIZE]);
    assert_eq!(reader.read(), Read::Window);
    assert!(reader.window().iter().all(|x| *x == 1.0));
    // A second read of a still writer finds nothing new.
    assert_eq!(reader.read(), Read::Nothing, "the window has not moved");
}

/// The reader gets the newest window, not the first one.
#[test]
fn the_reader_gets_the_newest_window() {
    let (mut tap, mut reader) = pair();
    reader.set_on(true);
    tap.write(&vec![1.0; SIZE], &vec![1.0; SIZE]);
    tap.write(&vec![2.0; SIZE], &vec![2.0; SIZE]);
    assert_eq!(reader.read(), Read::Window);
    assert!(reader.window().iter().all(|x| *x == 2.0));
}

/// L1: a reader lapped on every try keeps the whole window it held and
/// says it is stale, never a copy that is two windows joined — the promise
/// the module makes is the one the code keeps.
#[test]
fn a_reader_lapped_on_every_try_keeps_the_window_before() {
    let (mut tap, mut reader) = pair();
    reader.set_on(true);
    tap.write(&vec![1.0; SIZE], &vec![1.0; SIZE]);
    assert_eq!(reader.read(), Read::Window);
    assert!(reader.window().iter().all(|x| *x == 1.0));
    let spectrum_before = reader.spectrum().to_vec();

    // The writer moves on, and every try of the next read is lapped.
    tap.write(&vec![2.0; SIZE], &vec![2.0; SIZE]);
    reader.pretend_laps(TRIES);
    assert_eq!(reader.read(), Read::Stale);
    assert!(
        reader.window().iter().all(|x| *x == 1.0),
        "the window before is kept, not a torn one"
    );
    assert_eq!(
        reader.spectrum(),
        spectrum_before,
        "and its spectrum is not worked out again"
    );

    // The next read, with no lap, takes the new window.
    assert_eq!(reader.read(), Read::Window);
    assert!(reader.window().iter().all(|x| *x == 2.0));
}

/// Silence reads the floor, and a tone stands where it is in the picture.
#[test]
fn a_picture_of_silence_is_the_floor_and_a_tone_stands_where_it_is() {
    let rate = 48_000;
    let (mut tap, mut reader) = pair();
    reader.set_on(true);
    tap.write(&vec![0.0; SIZE], &vec![0.0; SIZE]);
    assert_eq!(reader.read(), Read::Window);
    let mut quiet = Display::new((20.0, 20_000.0));
    quiet.update(reader.spectrum(), rate, 1.0 / 60.0);
    assert!(quiet.bins().iter().all(|db| *db == MIN_DB));

    let samples = sine(1_000.0, rate as f32, SIZE, 0.5);
    tap.write(&samples, &samples);
    assert_eq!(reader.read(), Read::Window);
    let mut picture = Display::new((20.0, 20_000.0));
    for _ in 0..30 {
        picture.update(reader.spectrum(), rate, 1.0 / 60.0);
    }
    let loudest = picture
        .bins()
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(at, _)| at)
        .unwrap();
    let share = loudest as f32 / (picture.len() - 1) as f32;
    let hz = 20.0 * 1000f32.powf(share);
    assert!(
        (hz - 1_000.0).abs() < 40.0,
        "{hz} Hz: {}",
        picture.bins()[loudest]
    );
    assert!(
        (picture.bins()[loudest] - -6.0).abs() < 1.0,
        "a half-scale tone reads {}",
        picture.bins()[loudest]
    );
}

/// The picture is a bin every forty-eighth of an octave across the face's
/// own axis: a fixed count, whatever the window holds.
#[test]
fn the_picture_has_a_bin_every_forty_eighth_of_an_octave() {
    let picture = Display::new((20.0, 20_000.0));
    assert_eq!(picture.len(), POINTS);
    assert_eq!(picture.axis(), (20.0, 20_000.0));
    let spacing = picture.centres[1] / picture.centres[0];
    assert!(
        (spacing.log2() * PER_OCTAVE - 1.0).abs() < 0.02,
        "{spacing}"
    );
}

/// A steady tone is a still picture: once it has settled, no bin moves
/// from one frame to the next. The old "loudest bin under each point"
/// picture moved by decibels here.
#[test]
fn a_steady_tone_gives_a_still_picture() {
    let frames = 90;
    let hop = 800;
    let signal: Vec<f32> = (0..SIZE + frames * hop)
        .map(|i| (0.5 * (std::f64::consts::TAU * 997.0 * i as f64 / 48_000.0).sin()) as f32)
        .collect();
    let picture = run(&signal, hop, frames, 1.0 / 60.0);
    let settled = &picture[30..];
    let mut worst: f32 = 0.0;
    for bin in 0..settled[0].len() {
        let mean = settled.iter().map(|frame| frame[bin]).sum::<f32>() / settled.len() as f32;
        let variance = settled
            .iter()
            .map(|frame| (frame[bin] - mean).powi(2))
            .sum::<f32>()
            / settled.len() as f32;
        worst = worst.max(variance.sqrt());
    }
    assert!(worst < 0.25, "the still tone moved {worst} dB");
}

/// Pink noise is a flat, smooth picture: no bin stands out from its
/// neighbours and the whole axis reads the same level, rather than the
/// log-spaced bins of a transform's own bins (a stair at the bottom and a
/// comb at the top).
#[test]
fn pink_noise_is_a_flat_and_smooth_picture() {
    let frames = 90;
    let hop = 800;
    let signal = pink(SIZE + frames * hop);
    let picture = run(&signal, hop, frames, 1.0 / 60.0);
    let settled = &picture[30..];
    let last = &settled[settled.len() - 1];
    // Smooth: neighbouring bins are near each other.
    let roughness: f32 =
        last.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / (last.len() - 1) as f32;
    assert!(roughness < 1.0, "{roughness} dB a bin");
    let worst = last
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 4.0, "a step of {worst} dB between two bins");
    // Flat: an octave at the bottom, in the middle and at the top reads
    // alike (the first bins, under one transform bin, are the transform's
    // own limit and are left out).
    let mean = |from: f32, to: f32| {
        let at = |hz: f32| {
            let share = (hz / 20.0).log10() / 3.0;
            ((share * (last.len() - 1) as f32) as usize).min(last.len() - 1)
        };
        let (from, to) = (at(from), at(to));
        last[from..to].iter().sum::<f32>() / (to - from) as f32
    };
    let (bottom, middle, top) = (
        mean(100.0, 200.0),
        mean(1_000.0, 2_000.0),
        mean(10_000.0, 20_000.0),
    );
    for (name, at) in [("bottom", bottom), ("middle", middle), ("top", top)] {
        assert!(
            (at - middle).abs() < 4.0,
            "the {name} reads {at} against {middle}"
        );
    }
    // And it is a still picture too: pink noise moves less than a decibel
    // from frame to frame in the middle of the axis, and no bin (the ones
    // under a transform bin excepted) moves by more than three.
    let mut stds: Vec<f32> = (0..last.len())
        .map(|bin| {
            let mean = settled.iter().map(|frame| frame[bin]).sum::<f32>() / settled.len() as f32;
            (settled
                .iter()
                .map(|frame| (frame[bin] - mean).powi(2))
                .sum::<f32>()
                / settled.len() as f32)
                .sqrt()
        })
        .collect();
    stds.sort_by(f32::total_cmp);
    let median = stds[stds.len() / 2];
    let worst = stds[stds.len() - 1];
    assert!(median < 1.5, "pink noise moved {median} dB a frame");
    assert!(worst < 3.0, "a bin moved {worst} dB a frame");
}

/// Under the transform's first bin the picture is interpolated, not
/// stepped: a spectrum that climbs steadily climbs steadily in the
/// picture. A picture of the loudest bin under each point held one value
/// across every point below 700 Hz.
#[test]
fn the_low_end_is_interpolated_not_stepped() {
    // Amplitude ∝ bin number, so the power climbs 6 dB an octave.
    let spectrum: Vec<f32> = (0..BINS)
        .map(|bin| 20.0 * (bin.max(1) as f32).log10())
        .collect();
    let mut picture = Display::new((20.0, 20_000.0));
    picture.update(&spectrum, 48_000, 1.0);
    // The transform's own first bin is 23.4 Hz; from the first bin whose
    // band reaches past it, every bin climbs.
    let bin_hz = 48_000.0 / 2.0 / BINS as f32;
    let first = picture
        .centres
        .iter()
        .position(|hz| hz * ABOVE > bin_hz)
        .unwrap();
    assert!(first > 4, "{first} bins under the first one");
    let low: Vec<f32> = picture.bins()[..60].to_vec();
    for pair in low[first..].windows(2) {
        assert!(
            pair[1] > pair[0] + 0.05,
            "the low end stepped: {pair:?} in {low:?}"
        );
    }
    // And under the first bin, where a transform cannot see, the picture is
    // straight rather than stepped down.
    for pair in low[..first].windows(2) {
        assert!(
            (pair[1] - pair[0]).abs() < 0.02,
            "the deepest bins are not straight: {pair:?}"
        );
    }
}

/// The ballistics are the same in wall-clock time whatever the frame rate
/// is: a fall covers the same distance after the same hundred
/// milliseconds at 30, 60, 144 and 240 frames a second.
#[test]
fn the_ballistics_keep_their_time_whatever_the_frame_rate() {
    for fps in [30.0, 60.0, 144.0, 240.0] {
        let dt = 1.0 / fps;
        let mut shown = 0.0f32;
        let mut at = 0.0f32;
        let mut marks = Vec::new();
        while at < 0.2999 {
            shown = follow(shown, MIN_DB, dt);
            at += dt;
            if [0.1f32, 0.2]
                .iter()
                .any(|mark| (at - mark).abs() < dt * 0.01)
            {
                marks.push((at, shown));
            }
        }
        for (at, shown) in marks {
            let want = MIN_DB + (0.0 - MIN_DB) * (-at / FALL).exp();
            assert!(
                (shown - want).abs() < 0.05,
                "{fps} fps at {at} s: {shown} against {want}"
            );
        }
    }
}

/// A rise is ten milliseconds and a fall is three hundred: the two time
/// constants the picture is read at.
#[test]
fn a_step_rises_in_ten_milliseconds_and_falls_in_three_hundred() {
    let risen = follow(MIN_DB, 0.0, RISE);
    let want = MIN_DB + (0.0 - MIN_DB) * (1.0 - (-1.0f32).exp());
    assert!((risen - want).abs() < 0.05, "{risen} against {want}");
    let fallen = follow(0.0, MIN_DB, FALL);
    let want = MIN_DB + (0.0 - MIN_DB) * (-1.0f32).exp();
    assert!((fallen - want).abs() < 0.05, "{fallen} against {want}");
    // Nearly there after five times the constant, and never past it.
    assert!((follow(MIN_DB, 0.0, RISE * 5.0) - 0.0).abs() < 1.0);
    assert!(follow(0.0, MIN_DB, 0.0) == 0.0);
}

/// A measurement, not a test: what one window costs the interface, from
/// the transform's bins to the picture's.
#[test]
#[ignore = "a measurement: prints times, asserts only a generous bound"]
fn the_cost_of_a_picture() {
    let spectrum: Vec<f32> = (0..BINS)
        .map(|bin| -90.0 + 60.0 * ((bin as f32) * 0.37).sin().abs())
        .collect();
    let mut picture = Display::new((20.0, 20_000.0));
    picture.update(&spectrum, 48_000, 1.0 / 60.0);
    let mut times = Vec::new();
    for _ in 0..1000 {
        let started = std::time::Instant::now();
        picture.update(&spectrum, 48_000, 1.0 / 60.0);
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "iris picture: {} bins, {:.3} ms median, {:.3} ms worst",
        picture.len(),
        times[500],
        times[999]
    );
    assert!(times[500] < 10.0, "{} ms", times[500]);
}

/// A window of `signal` folded into a picture, `frames` of them: what the
/// interface does when it draws.
fn run(signal: &[f32], hop: usize, frames: usize, dt: f32) -> Vec<Vec<f32>> {
    let (mut tap, mut reader) = pair();
    reader.set_on(true);
    tap.write(&signal[..SIZE], &signal[..SIZE]);
    let mut display = Display::new((20.0, 20_000.0));
    let mut at = SIZE;
    let mut out = Vec::new();
    for _ in 0..frames {
        if at + hop <= signal.len() {
            tap.write(&signal[at..at + hop], &signal[at..at + hop]);
            at += hop;
        }
        assert_eq!(reader.read(), Read::Window);
        display.update(reader.spectrum(), 48_000, dt);
        out.push(display.bins().to_vec());
    }
    out
}

/// Pink noise, from white noise through the usual seven-pole approximation
/// (a test's own signal, not the product's DSP).
fn pink(frames: usize) -> Vec<f32> {
    let mut state = 0x2F6E_2B5Du32;
    let mut b = [0.0f32; 7];
    (0..frames)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = (state >> 8) as f32 / (1 << 24) as f32 - 0.5;
            b[0] = 0.99886 * b[0] + white * 0.055_517_9;
            b[1] = 0.99332 * b[1] + white * 0.075_075_9;
            b[2] = 0.96900 * b[2] + white * 0.153_852;
            b[3] = 0.86650 * b[3] + white * 0.310_485_6;
            b[4] = 0.55000 * b[4] + white * 0.532_952_2;
            b[5] = -0.7616 * b[5] - white * 0.016_898;
            let out = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + white * 0.5362;
            b[6] = white * 0.115_926;
            out * 0.11
        })
        .collect()
}
