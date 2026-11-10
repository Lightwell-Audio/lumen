use super::*;

/// A file in memory.
struct Memory(Vec<Vec<f32>>);

impl Source for Memory {
    fn channels(&self) -> usize {
        self.0.len()
    }

    fn frames(&self) -> u64 {
        self.0[0].len() as u64
    }

    fn read(&mut self, from: u64, out: &mut [&mut [f32]]) -> Result<(), MediaError> {
        for (c, out) in out.iter_mut().enumerate() {
            let data = &self.0[c];
            let from = from as usize;
            let n = out.len().min(data.len().saturating_sub(from));
            out[..n].copy_from_slice(&data[from..from + n]);
        }
        Ok(())
    }
}

/// A file that is never quite the same twice in a row.
fn file(frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| ((i as f64 * 0.37).sin() * 0.5 + (i % 17) as f64 * 0.01) as f32)
        .collect()
}

fn render(
    data: &[f32],
    position: &dyn Fn(i64) -> f64,
    starts: &[i64],
    first: i64,
    n: usize,
) -> Vec<f32> {
    let mut source = Memory(vec![data.to_vec()]);
    let (mut l, mut r) = (vec![9.0; n], vec![]);
    Slicer::new()
        .render(&mut source, (position, starts, None), first, &mut l, &mut r)
        .unwrap();
    l
}

#[test]
fn a_file_played_as_it_was_made_is_copied_through_its_slices() {
    let data = file(9_000);
    let out = render(&data, &|k| k as f64, &[0, 2_000, 5_000], 0, 9_000);
    assert_eq!(out, data);
    // Offset by whole frames: the same frames, later.
    let out = render(&data, &|k| k as f64 + 100.0, &[0, 2_000, 5_000], 0, 3_000);
    assert_eq!(out, data[100..3_100]);
}

#[test]
fn a_slower_song_places_each_slice_where_it_starts_and_leaves_a_gap() {
    let data = file(9_000);
    let starts = [0, 2_000, 5_000];
    // Half speed: the file takes twice the frames.
    let out = render(&data, &|k| k as f64 * 0.5, &starts, 0, 18_000);
    // Slice 1 starts at output 4,000, whole and at the file's own speed.
    assert_eq!(out[4_000..4_000 + 2_000], data[2_000..4_000]);
    // Slice 0 plays 2,000 frames, the last FADE of them faded; then silence.
    assert_eq!(out[..2_000 - FADE as usize], data[..2_000 - FADE as usize]);
    assert!(out[2_000 - 20..2_000].iter().all(|v| v.abs() < 0.05));
    assert!(out[2_000..4_000].iter().all(|v| *v == 0.0), "the gap");
}

#[test]
fn a_faster_song_ends_a_slice_under_the_next_attack() {
    let data = file(9_000);
    let starts = [0, 2_000, 5_000];
    // Twice the speed: slice 1 starts at output 1,000.
    let out = render(&data, &|k| k as f64 * 2.0, &starts, 0, 4_500);
    assert_eq!(out[..1_000], data[..1_000]);
    for m in 0..FADE {
        let k = (1_000 + m) as usize;
        let g = 1.0 - (m + 1) as f32 / (FADE + 1) as f32;
        let want = data[2_000 + m as usize] + data[k] * g;
        assert!(
            (out[k] - want).abs() < 1e-6,
            "frame {k}: {} against {want}",
            out[k]
        );
    }
    // After the fade, the new slice alone, whole.
    let after = (1_000 + FADE) as usize;
    assert_eq!(
        out[after..1_000 + 1_500],
        data[2_000 + FADE as usize..3_500]
    );
}

#[test]
fn any_way_of_asking_gives_the_same_bits() {
    let data = file(30_000);
    let starts = [0, 2_000, 5_000, 9_000, 15_000, 22_000];
    // Slower, then faster, then the file's own speed.
    let position = |k: i64| -> f64 {
        if k < 6_000 {
            k as f64 * 0.5
        } else if k < 9_000 {
            3_000.0 + (k - 6_000) as f64 * 2.0
        } else {
            9_000.0 + (k - 9_000) as f64
        }
    };
    let n = 20_000;
    let whole = render(&data, &position, &starts, 0, n);
    for piece in [1_usize, 7, 333, 1_024, 4_096] {
        let mut joined = Vec::new();
        let mut at = 0;
        while at < n {
            let len = piece.min(n - at);
            joined.extend(render(&data, &position, &starts, at as i64, len));
            at += len;
        }
        assert_eq!(joined, whole, "pieces of {piece}");
    }
    // A jump into the middle, onto a slice's fade.
    let from = 3_950;
    assert_eq!(
        render(&data, &position, &starts, from, 800),
        whole[from as usize..from as usize + 800]
    );
    // Before the file: silence.
    let early = render(&data, &|k| k as f64 - 500.0, &starts, 0, 1_000);
    assert!(early[..500].iter().all(|v| *v == 0.0));
    assert_eq!(early[500..], data[..500]);
}

#[test]
fn stereo_slices_keep_their_channels() {
    let l = file(6_000);
    let r: Vec<f32> = l.iter().map(|v| -v).collect();
    let mut source = Memory(vec![l.clone(), r.clone()]);
    let (mut a, mut b) = (vec![0.0; 6_000], vec![0.0; 6_000]);
    Slicer::new()
        .render(
            &mut source,
            (&|k| k as f64, &[0, 3_000], None),
            0,
            &mut a,
            &mut b,
        )
        .unwrap();
    assert_eq!((a, b), (l, r));
}

#[test]
fn slice_starts_are_ordered_apart_and_inside_the_file() {
    let got = starts(
        [0.5, 0.1, 0.1005, f64::NAN, 9.0, -1.0, 0.0],
        [],
        48_000,
        200_000,
    );
    assert_eq!(got, vec![0, 4_800, 24_000]);
    assert_eq!(starts([], [], 48_000, 100), vec![0]);
    // A transient closer than MIN_SLICE to another start is not a slice.
    let close = starts([0.5, 0.5 + 1_000.0 / 48_000.0], [], 48_000, 100_000);
    assert_eq!(close, vec![0, 24_000]);
}

#[test]
fn a_warp_markers_place_is_always_a_slice_start() {
    // A marker 10 ms after a transient, and one 100 frames after the file's
    // start: both are starts; the transients beside them give way.
    let got = starts([0.1], [0.110, 100.0 / 48_000.0, 0.5], 48_000, 200_000);
    assert_eq!(got, vec![0, 100, 5_280, 24_000]);
    // A marker on the first frame is that start.
    assert_eq!(starts([], [0.0], 48_000, 1_000), vec![0]);
}

#[test]
fn a_position_a_hair_short_of_a_whole_frame_has_reached_it() {
    // The map plays frame 1,052 at 1,051.9999999999998, as floating point
    // often does; the file is still copied whole.
    let data = file(6_000);
    let out = render(&data, &|k| k as f64 - 2.0e-13, &[0, 1_052, 3_001], 0, 6_000);
    assert_eq!(out, data);
}

#[test]
fn a_song_many_times_faster_still_gives_the_same_bits_however_it_is_asked() {
    // Twenty times the speed puts the slices 120 frames apart in the song:
    // closer than a fade, so a slice's tail runs under several.
    let data = file(20_000);
    let starts = [0, 2_400, 4_800, 7_200, 9_600];
    let position = |k: i64| k as f64 * 20.0;
    let whole = render(&data, &position, &starts, 0, 1_000);
    for piece in [1_usize, 5, 64, 250] {
        let mut joined = Vec::new();
        let mut at = 0;
        while at < whole.len() {
            let len = piece.min(whole.len() - at);
            joined.extend(render(&data, &position, &starts, at as i64, len));
            at += len;
        }
        assert_eq!(joined, whole, "pieces of {piece}");
    }
    // A run that begins while the first slice is still fading under the rest.
    assert_eq!(render(&data, &position, &starts, 250, 100), whole[250..350]);
}

#[test]
fn the_ends_of_the_number_line_do_not_overflow() {
    let data = file(2_000);
    let out = render(&data, &|k| k as f64, &[0, 1_000], i64::MAX, 1);
    assert_eq!(out, vec![0.0]);
    let out = render(&data, &|k| k as f64, &[0, 1_000], i64::MAX - 1, 4);
    assert!(out.iter().all(|v| *v == 0.0));
    let out = render(&data, &|_| f64::INFINITY, &[0, 1_000], 0, 4);
    assert!(out.iter().all(|v| *v == 0.0));
}

#[test]
fn scanning_finds_the_hits_of_a_file() {
    // Four bursts of a decaying noise, half a second apart.
    let rate = 48_000_u32;
    let mut data = vec![0.0_f32; 4 * 24_000];
    let mut seed = 12_345_u32;
    for burst in 0..4 {
        for i in 0..4_800 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0;
            data[burst * 24_000 + 2_400 + i] = noise * (1.0 - i as f32 / 4_800.0);
        }
    }
    let found = scan(&mut Memory(vec![data]), rate).unwrap();
    for burst in 0..4 {
        let want = (burst * 24_000 + 2_400) as f64 / f64::from(rate);
        assert!(
            found.iter().any(|t| (t - want).abs() < 0.01),
            "no hit near {want}: {found:?}"
        );
    }
}

#[test]
fn a_stereo_pair_of_opposite_polarity_is_not_cancelled_to_nothing() {
    let rate = 48_000_u32;
    let mut left = vec![0.0_f32; 3 * 24_000];
    let mut seed = 99_u32;
    for burst in 0..3 {
        for i in 0..4_800 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0;
            left[burst * 24_000 + 2_400 + i] = noise * (1.0 - i as f32 / 4_800.0);
        }
    }
    let right: Vec<f32> = left.iter().map(|v| -v).collect();
    let found = scan(&mut Memory(vec![left, right]), rate).unwrap();
    for burst in 0..3 {
        let want = (burst * 24_000 + 2_400) as f64 / f64::from(rate);
        assert!(found.iter().any(|t| (t - want).abs() < 0.01), "{found:?}");
    }
}

#[test]
fn the_last_slice_fades_at_half_speed_and_a_copy_stays_exact() {
    let data = file(9_000);
    let starts = [0, 2_000, 5_000];
    // Half speed: the last slice (4,000 frames) starts at output 10,000, plays
    // to its end at 14,000 and the end of the file fades.
    let out = render(&data, &|k| k as f64 * 0.5, &starts, 0, 20_000);
    let last = 10_000;
    assert_eq!(out[last..last + 3_000], data[5_000..8_000]);
    assert!(
        out[last + 4_000 - 20..last + 4_000]
            .iter()
            .all(|v| v.abs() < 0.05)
    );
    assert!(out[last + 4_000..].iter().all(|v| *v == 0.0));
    // At the file's own speed nothing is faded: the file, bit for bit.
    let out = render(&data, &|k| k as f64, &starts, 0, 9_000);
    assert_eq!(out, data);
}

fn render_pitched(
    data: &[f32],
    (position, pitch): (&dyn Fn(i64) -> f64, &dyn Fn(i64) -> f64),
    starts: &[i64],
    first: i64,
    n: usize,
) -> Vec<f32> {
    let mut source = Memory(vec![data.to_vec()]);
    let (mut l, mut r) = (vec![9.0; n], vec![]);
    Slicer::new()
        .render(
            &mut source,
            (position, starts, Some(pitch)),
            first,
            &mut l,
            &mut r,
        )
        .unwrap();
    l
}

/// The amplitude of the sine at `cycles` per frame in `x`, under a Hann window.
fn amplitude(x: &[f32], cycles: f64) -> f64 {
    let n = x.len() as f64;
    let (mut re, mut im, mut window) = (0.0, 0.0, 0.0);
    for (i, v) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n).cos();
        let phase = std::f64::consts::TAU * cycles * i as f64;
        re += w * f64::from(*v) * phase.cos();
        im += w * f64::from(*v) * phase.sin();
        window += w;
    }
    2.0 * re.hypot(im) / window
}

fn sine(frames: usize, cycles: f64) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f64::consts::TAU * cycles * i as f64).sin() as f32 * 0.5)
        .collect()
}

/// An octave up plays a hit at twice the rate: its pitch doubles, it lasts
/// half as long, and it still begins where the map puts it.
#[test]
fn a_transposed_slice_is_read_at_the_rate_of_its_pitch() {
    let data = sine(8_000, 0.02);
    let starts = [0, 4_000];
    let up = render_pitched(&data, (&|k| k as f64, &|_| 1_200.0), &starts, 0, 8_000);
    // The first slice's 4,000 file frames play in 2,000 output frames.
    let heard = &up[200..1_800];
    assert!(amplitude(heard, 0.04) > 0.45, "an octave up");
    assert!(amplitude(heard, 0.02) < 0.02, "nothing at the old pitch");
    // It is over by frame 2,000, then silence until the second slice at 4,000.
    assert!(up[2_100..3_900].iter().all(|v| v.abs() < 1e-6), "the gap");
    let second = &up[4_200..5_800];
    assert!(
        amplitude(second, 0.04) > 0.45,
        "the next slice starts on time"
    );
    // An octave down plays at half the rate and is cut by the next slice.
    let down = render_pitched(&data, (&|k| k as f64, &|_| -1_200.0), &starts, 0, 8_000);
    assert!(amplitude(&down[200..3_800], 0.01) > 0.45, "an octave down");
}

/// A pitch of nothing is no pitch at all, bit for bit.
#[test]
fn a_pitch_of_nothing_plays_as_it_was_made() {
    let data = file(9_000);
    type Case = (Box<dyn Fn(i64) -> f64>, Vec<i64>);
    let cases: [Case; 3] = [
        (Box::new(|k| k as f64), vec![0, 2_000, 5_000]),
        (Box::new(|k| k as f64 * 0.5), vec![0, 2_000, 5_000]),
        (Box::new(|k| k as f64 * 2.0), vec![0, 1_500, 3_000, 5_000]),
    ];
    for (position, starts) in &cases {
        let plain = render(&data, position, starts, 0, 9_000);
        let zero = render_pitched(&data, (position, &|_| 0.0), starts, 0, 9_000);
        assert_eq!(plain, zero);
    }
}

/// A pitch that moves along the slice gives the same bits however it is
/// asked for, in one piece, in pages or from the middle.
#[test]
fn a_moving_pitch_gives_the_same_bits_however_it_is_asked() {
    let data = file(30_000);
    let starts = [0, 6_000, 13_000, 21_000];
    let position = |k: i64| k as f64 * 0.8 + 100.0;
    let pitch = |k: i64| 500.0 * (k as f64 / 3_000.0).sin() + 200.0;
    let n = 24_000;
    let whole = render_pitched(&data, (&position, &pitch), &starts, 0, n);
    assert!(
        whole.iter().any(|v| v.abs() > 0.1),
        "something was rendered"
    );
    let mut joined = Vec::new();
    for at in (0..n).step_by(4_096) {
        let len = 4_096.min(n - at);
        joined.extend(render_pitched(
            &data,
            (&position, &pitch),
            &starts,
            at as i64,
            len,
        ));
    }
    assert_eq!(joined, whole, "pages");
    assert_eq!(
        render_pitched(&data, (&position, &pitch), &starts, 9_001, 777),
        whole[9_001..9_778],
        "from the middle"
    );
}

/// Where a slice reads the file follows its pitch: constant at a rate, and a
/// ramp's rate is the average of its ends.
#[test]
fn a_slice_reads_the_file_at_the_rate_of_its_pitch() {
    let s = Slice {
        from: 0,
        len: 4_000,
        at: 0,
        next: i64::MAX,
    };
    let mut octave = Reads::new(&s, &|_| 1_200.0);
    octave.extend(&|_| 1_200.0, 4_000, 4_000);
    assert!((octave.position(1_600) - 3_200.0).abs() < 1e-9);
    assert_eq!(octave.length(4_000), Some(2_000), "half as long");
    let mut flat = Reads::new(&s, &|_| 0.0);
    flat.extend(&|_| 0.0, 8_000, 4_000);
    assert_eq!(flat.length(4_000), Some(4_000));
    assert_eq!(flat.length(9_000), None, "not reached");
}
