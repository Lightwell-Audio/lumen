use super::*;

const RATE: u32 = 48_000;

/// A click: a short burst of decaying noise from a fixed sequence.
fn click(out: &mut [f32], at: usize, gain: f32) {
    let mut seed = 0x1234_5678_u32;
    for i in 0..2_400 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
        let decay = (-(i as f32) / 400.0).exp();
        if let Some(slot) = out.get_mut(at + i) {
            *slot += gain * noise * decay;
        }
    }
}

/// `beats` beats at `bpm`, a click on each (a louder one on every fourth),
/// starting `lead` seconds in, in a buffer `seconds` long.
fn beats(bpm: f64, beats: usize, lead: f64, seconds: f64) -> Vec<f32> {
    let mut out = vec![0.0; (seconds * f64::from(RATE)) as usize];
    for b in 0..beats {
        let at = ((lead + b as f64 * 60.0 / bpm) * f64::from(RATE)) as usize;
        click(&mut out, at, if b % 4 == 0 { 0.9 } else { 0.5 });
    }
    out
}

/// A quiet noise floor, so a soft click is a few dB above something.
fn floor(frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames];
    let mut seed = 99_u32;
    for slot in &mut out {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *slot = ((seed >> 8) as f32 / 8_388_608.0 - 1.0) * 0.003;
    }
    out
}

fn detect(samples: &[f32]) -> Analysis {
    let mut d = Detector::new(RATE);
    for block in samples.chunks(4_096) {
        d.add(block, None);
    }
    d.finish()
}

#[test]
fn a_one_bar_loop_gets_its_tempo_at_any_speed() {
    // Review round 2: a file under two seconds had no tempo, so the common
    // one-bar loops above 120 BPM never followed the song.
    for bpm in [100.0, 120.0, 140.0, 160.0, 180.0] {
        let seconds = 4.0 * 60.0 / bpm;
        let found = detect(&beats(bpm, 4, 0.0, seconds));
        let tempo = found.tempo.unwrap_or_else(|| panic!("no tempo at {bpm}"));
        assert!((tempo.bpm - bpm).abs() < 0.5, "{} for {bpm}", tempo.bpm);
        assert_eq!(tempo.beats, Some(4), "{bpm}");
        assert!(tempo.is_loop(), "{bpm}");
        assert!(
            tempo.confidence >= CONFIDENT,
            "{} at {bpm}",
            tempo.confidence
        );
    }
}

#[test]
fn a_short_file_with_no_rhythm_still_has_no_tempo() {
    let mut samples = floor(60_000);
    click(&mut samples, 10_000, 0.9);
    assert!(detect(&samples).tempo.is_none());
}

#[test]
fn an_empty_file_has_nothing_to_find() {
    let found = detect(&[]);
    assert_eq!(found.seconds, 0.0);
    assert!(found.transients.is_empty() && found.tempo.is_none());
}

#[test]
fn a_sound_in_the_last_partial_hop_is_found() {
    // A hop is 5 ms (240 frames). The file ends 100 frames into one: a
    // sound that starts there used to be dropped with the unfinished hop.
    let hop = (f64::from(RATE) * HOP_SECONDS).round() as usize;
    let end = 40 * hop + 100;
    let mut samples = vec![0.0; end];
    let at = 40 * hop + 20;
    for (i, slot) in samples[at..].iter_mut().enumerate() {
        *slot = if i % 2 == 0 { 0.8 } else { -0.8 };
    }
    let found = detect(&samples);
    assert_eq!(found.transients.len(), 1, "{:?}", found.transients);
    let want = at as f64 / f64::from(RATE);
    assert!((found.transients[0].seconds - want).abs() < 0.001);
    assert!((found.seconds - end as f64 / f64::from(RATE)).abs() < 1e-9);
}

#[test]
fn a_file_shorter_than_a_hop_is_read_as_it_is() {
    let mut samples = vec![0.0; 100];
    for slot in &mut samples[10..] {
        *slot = 0.9;
    }
    let found = detect(&samples);
    assert_eq!(found.transients.len(), 1);
    assert!(found.transients[0].seconds < 0.001);
}

#[test]
fn transients_are_found_where_the_sounds_start() {
    let samples = beats(120.0, 16, 0.25, 8.25);
    let found = detect(&samples);
    assert_eq!(found.transients.len(), 16, "{:?}", found.transients);
    for (b, t) in found.transients.iter().enumerate() {
        let want = 0.25 + b as f64 * 0.5;
        assert!(
            (t.seconds - want).abs() < 0.003,
            "beat {b}: {} for {want}",
            t.seconds
        );
        assert!(t.strength > 0.5, "a click out of silence is strong");
    }
}

#[test]
fn a_regular_beat_gives_its_tempo() {
    for bpm in [90.0, 120.0, 140.0] {
        // Not a whole number of beats: the tempo comes from the rhythm alone.
        let samples = beats(bpm, 32, 0.1, 32.0 * 60.0 / bpm + 30.0 / bpm);
        let tempo = detect(&samples).tempo.expect("a tempo");
        assert!((tempo.bpm - bpm).abs() < 1.5, "{} for {bpm}", tempo.bpm);
        assert_eq!(tempo.beats, None);
        assert!(tempo.confidence > CONFIDENT, "{}", tempo.confidence);
    }
}

#[test]
fn a_loop_of_whole_bars_gets_the_exact_tempo() {
    // Two bars at 100 bpm, cut to the sample: 4.8 s.
    let bpm = 100.0;
    let seconds = 8.0 * 60.0 / bpm;
    let samples = beats(bpm, 8, 0.0, seconds);
    let tempo = detect(&samples).tempo.expect("a tempo");
    assert_eq!(tempo.beats, Some(8));
    assert!((tempo.bpm - 100.0).abs() < 1e-3, "{}", tempo.bpm);
    assert!(tempo.is_loop());
    assert!(tempo.confidence >= CONFIDENT);
}

#[test]
fn no_rhythm_gives_no_tempo() {
    let silence = vec![0.0; RATE as usize * 10];
    let found = detect(&silence);
    assert!(found.transients.is_empty());
    assert_eq!(found.tempo, None);

    // A held tone has no sounds starting in it after the first.
    let tone: Vec<f32> = (0..RATE as usize * 10)
        .map(|i| 0.5 * (i as f32 * 0.05).sin())
        .collect();
    assert_eq!(detect(&tone).tempo, None);

    // Too few clicks to call a tempo.
    let three = beats(120.0, 3, 0.5, 6.0);
    assert_eq!(detect(&three).tempo, None);
}

#[test]
fn sensitivity_chooses_among_the_same_transients() {
    // Loud clicks with soft ones between.
    let mut samples = floor(RATE as usize * 4);
    for b in 0..8 {
        let at = (b as f64 * 0.5 * f64::from(RATE)) as usize;
        click(&mut samples, at, 0.9);
        click(&mut samples, at + RATE as usize / 4, 0.02);
    }
    let found = detect(&samples);
    let at = |s| found.transients_at(s).count();
    assert_eq!(at(100), 16, "everything at full sensitivity");
    assert_eq!(at(0), 8, "only the sharp ones at none");
    assert!(at(0) <= at(25) && at(25) <= at(50) && at(50) <= at(100));
    let near = found.nearest(0.51, 0, 0.05).expect("the click at 0.5 s");
    assert!((near.seconds - 0.5).abs() < 0.003);
    assert_eq!(found.nearest(0.26, 0, 0.01), None, "the soft one is hidden");
    assert!(found.nearest(0.26, 100, 0.01).is_some());
}

#[test]
fn the_same_audio_gives_the_same_analysis_whatever_the_blocks() {
    let samples = beats(128.0, 24, 0.0, 24.0 * 60.0 / 128.0);
    let mut d = Detector::new(RATE);
    for block in samples.chunks(777) {
        d.add(block, None);
    }
    assert_eq!(d.finish(), detect(&samples));
}

#[test]
fn stereo_is_averaged() {
    let mono = beats(120.0, 16, 0.25, 8.25);
    let other: Vec<f32> = mono.iter().map(|s| -s).collect();
    let mut d = Detector::new(RATE);
    // A channel and its opposite cancel; the same on both is the mono file.
    d.add(&mono, Some(&other));
    assert!(d.finish().transients.is_empty());
    let mut d = Detector::new(RATE);
    d.add(&mono, Some(&mono));
    assert_eq!(d.finish(), detect(&mono));
}

#[test]
fn an_analysis_survives_its_bytes_and_damage_is_refused() {
    let found = detect(&beats(100.0, 8, 0.0, 4.8));
    assert!(found.tempo.is_some());
    let bytes = found.to_bytes();
    assert_eq!(Analysis::from_bytes(&bytes), Some(found.clone()));
    let none = Analysis {
        seconds: 1.0,
        transients: Vec::new(),
        tempo: None,
    };
    assert_eq!(Analysis::from_bytes(&none.to_bytes()), Some(none));
    for cut in [0, 3, 5, bytes.len() - 1] {
        assert_eq!(Analysis::from_bytes(&bytes[..cut]), None, "cut at {cut}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(Analysis::from_bytes(&longer), None);
    let mut other = bytes;
    other[4] = 2;
    assert_eq!(Analysis::from_bytes(&other), None, "another version");
    let mut huge = found.to_bytes();
    huge[13..17].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(Analysis::from_bytes(&huge), None, "a count the bytes lack");
}

#[test]
fn a_cancelled_scan_gives_nothing() {
    let samples = beats(120.0, 8, 0.0, 4.0);
    let cancel = AtomicBool::new(true);
    let read = |at: u64, out: &mut [&mut [f32]]| {
        let n = out[0].len().min(samples.len() - at as usize);
        out[0][..n].copy_from_slice(&samples[at as usize..at as usize + n]);
        Ok(n)
    };
    let none = analyse(read, RATE, samples.len() as u64, 1, &cancel, &mut |_| {});
    assert_eq!(none.unwrap(), None);
}
