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
        .render(&mut source, (position, starts), first, &mut l, &mut r)
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
        .render(&mut source, (&|k| k as f64, &[0, 3_000]), 0, &mut a, &mut b)
        .unwrap();
    assert_eq!((a, b), (l, r));
}

#[test]
fn slice_starts_are_ordered_apart_and_inside_the_file() {
    let got = starts(
        [0.5, 0.1, 0.1005, f64::NAN, 9.0, -1.0, 0.0],
        48_000,
        200_000,
    );
    assert_eq!(got, vec![0, 4_800, 24_000]);
    assert_eq!(starts([], 48_000, 100), vec![0]);
    // MIN_SLICE apart, not just distinct.
    let close = starts([0.5, 0.5 + 1_000.0 / 48_000.0], 48_000, 100_000);
    assert_eq!(close, vec![0, 24_000]);
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
