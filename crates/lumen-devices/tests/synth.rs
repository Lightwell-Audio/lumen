//! The built-in synth: sound at the exact frame, envelopes, the pedal,
//! pitch bend, voice stealing, presets and a fuzz for numbers gone wrong.

use std::path::{Path, PathBuf};

use lumen_core::midi::{Event, Msg};
use lumen_devices::{Instrument, Preset, Synth};
use lumen_rt::{count_rt_violations, guard_is_active};

lumen_rt::install_guarded_allocator!();

const RATE: u32 = 48_000;

fn on(offset: u32, key: u8, velocity: u8) -> Event {
    Event {
        offset,
        msg: Msg::note_on(0, key, velocity),
    }
}

fn off(offset: u32, key: u8) -> Event {
    Event {
        offset,
        msg: Msg::note_off(0, key),
    }
}

fn cc(offset: u32, number: u8, value: u8) -> Event {
    Event {
        offset,
        msg: Msg::controller(0, number, value),
    }
}

/// Renders `frames` of `synth` after `events` (offsets from the start), in
/// blocks of `block`, returning left and right.
fn render(
    synth: &mut Synth,
    events: &[Event],
    frames: usize,
    block: usize,
) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    let mut at = 0;
    while at < frames {
        let n = block.min(frames - at);
        let local: Vec<Event> = events
            .iter()
            .filter(|e| (e.offset as usize) >= at && (e.offset as usize) < at + n)
            .map(|e| Event {
                offset: e.offset - at as u32,
                msg: e.msg,
            })
            .collect();
        // The synth is the audio thread's work: nothing here may reach the
        // heap (the allocation guard, `realtime.rs`).
        let ((), heap) = count_rt_violations(|| {
            synth.render(RATE, &local, &mut left[at..at + n], &mut right[at..at + n]);
        });
        assert_eq!(heap, 0, "heap operation in the synth at frame {at}");
        at += n;
    }
    (left, right)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[test]
fn a_note_begins_on_its_frame_and_not_before() {
    assert!(guard_is_active(), "guarded allocator not installed");
    for block in [64, 100, 480] {
        let mut synth = Synth::new(Preset::Poly);
        let (left, right) = render(&mut synth, &[on(1_000, 60, 100)], 4_000, block);
        assert!(left[..1_000].iter().all(|v| *v == 0.0), "block {block}");
        assert!(right[..1_000].iter().all(|v| *v == 0.0));
        assert!(left[1_001..1_400].iter().any(|v| v.abs() > 0.0));
        assert!(rms(&left[2_000..4_000]) > 0.01, "it sounds");
    }
}

#[test]
fn the_output_does_not_depend_on_how_blocks_are_cut() {
    let events = [
        on(100, 60, 100),
        on(3_000, 64, 80),
        cc(5_000, 1, 100),
        Event {
            offset: 7_000,
            msg: Msg::pitch_bend(0, 4_000),
        },
        off(9_000, 60),
        off(11_000, 64),
    ];
    let reference = render(&mut Synth::new(Preset::Poly), &events, 20_000, 20_000);
    for block in [32, 77, 128, 512, 1_000, 1_024] {
        let split = render(&mut Synth::new(Preset::Poly), &events, 20_000, block);
        assert_eq!(split, reference, "blocks of {block}");
    }
}

#[test]
fn a_note_rings_then_releases_to_silence() {
    let mut synth = Synth::new(Preset::Poly);
    let (left, _) = render(&mut synth, &[on(0, 60, 127), off(24_000, 60)], 96_000, 128);
    assert!(rms(&left[10_000..20_000]) > 0.02);
    assert!(rms(&left[40_000..48_000]) < 0.001, "released after 250 ms");
    assert!(!synth.active() && synth.voices() == 0);
    assert!(peak(&left) < 1.0);
}

#[test]
fn the_sustain_pedal_holds_released_keys() {
    assert!(guard_is_active(), "guarded allocator not installed");
    let mut synth = Synth::new(Preset::Poly);
    let events = [
        cc(0, 64, 127),
        on(100, 60, 100),
        off(10_000, 60),
        cc(60_000, 64, 0),
    ];
    let (left, _) = render(&mut synth, &events, 48_000 * 2, 128);
    assert!(
        rms(&left[40_000..50_000]) > 0.01,
        "still sounding with the pedal down"
    );
    assert!(
        rms(&left[80_000..96_000]) < 0.001,
        "released with the pedal"
    );
    assert!(!synth.active());
}

#[test]
fn pitch_bend_changes_the_pitch() {
    fn crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }
    let mut a = Synth::new(Preset::Bass);
    let (flat, _) = render(&mut a, &[on(0, 45, 100)], 24_000, 128);
    let mut b = Synth::new(Preset::Bass);
    let up = Event {
        offset: 0,
        msg: Msg::pitch_bend(0, 8_191),
    };
    let (bent, _) = render(&mut b, &[up, on(0, 45, 100)], 24_000, 128);
    let ratio = crossings(&bent[4_000..]) as f32 / crossings(&flat[4_000..]) as f32;
    // Two semitones up is a ratio of about 1.122.
    assert!((ratio - 1.122).abs() < 0.04, "ratio {ratio}");
}

#[test]
fn a_seventeenth_note_steals_the_oldest_voice() {
    let mut synth = Synth::new(Preset::Pad);
    let notes: Vec<Event> = (0..17).map(|i| on(i * 10, 40 + i as u8, 100)).collect();
    let (left, _) = render(&mut synth, &notes, 20_000, 128);
    assert_eq!(synth.voices(), 16);
    assert!(peak(&left) < 1.0);
    assert!(left.iter().all(|v| v.is_finite()));
}

#[test]
fn a_preset_change_glides_sounding_notes_without_a_click_or_a_new_note() {
    let bright = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / rms(x);
    let steepest = |x: &[f32]| {
        x.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max)
    };
    let mut synth = Synth::new(Preset::Poly);
    let (before, _) = render(&mut synth, &[on(0, 60, 100)], 24_000, 128);
    synth.configure(Preset::Bass.number());
    assert_eq!(synth.preset(), Some(Preset::Bass));
    let glide = (lumen_devices::synth::GLIDE_SECONDS * RATE as f32) as usize;
    let (during, _) = render(&mut synth, &[], glide + 128, 128);
    let (after, _) = render(&mut synth, &[], 9_600, 128);
    assert_eq!(synth.voices(), 1, "no new note");
    // No step beyond what the note itself does, across the change.
    let straight = steepest(&before[4_000..]);
    assert!(
        steepest(&during) < straight * 2.0,
        "{} {straight}",
        steepest(&during)
    );
    // Once it has glided, the held note has the new preset's character:
    // the bass is darker than the poly.
    assert!(bright(&after) < bright(&before[12_000..]) * 0.8);
    // And matches a bass note held the same way (level within 15%).
    let mut bass = Synth::new(Preset::Bass);
    let skip = 24_000 + glide + 128;
    let (b, _) = render(&mut bass, &[on(0, 60, 100)], skip + 9_600, 128);
    let b = &b[skip..];
    assert!(
        (rms(&after) - rms(b)).abs() / rms(b) < 0.15,
        "{} {}",
        rms(&after),
        rms(b)
    );
    // Nothing hangs: release it and it ends.
    render(&mut synth, &[off(0, 60)], 96_000, 4_800);
    assert!(!synth.active());
}

#[test]
fn a_voice_in_release_keeps_releasing_through_a_preset_change() {
    let mut synth = Synth::new(Preset::Pad);
    render(&mut synth, &[on(0, 60, 100), off(9_000, 60)], 12_000, 128);
    assert!(synth.active());
    synth.configure(Preset::Bass.number());
    let (tail, _) = render(&mut synth, &[], 4_000, 128);
    assert!(
        tail.windows(2).all(|w| (w[1] - w[0]).abs() < 0.2),
        "no step"
    );
    render(&mut synth, &[], 96_000, 4_800);
    assert!(!synth.active(), "it still ends");
}

#[test]
fn a_new_note_has_the_new_sound() {
    let mut synth = Synth::new(Preset::Poly);
    synth.configure(Preset::Bass.number());
    let mut poly = Synth::new(Preset::Poly);
    let mut bass = Synth::new(Preset::Bass);
    let (p, _) = render(&mut poly, &[on(0, 72, 100)], 9_600, 128);
    let (b, _) = render(&mut bass, &[on(0, 72, 100)], 9_600, 128);
    let (n, _) = render(&mut synth, &[on(0, 72, 100)], 9_600, 128);
    assert_eq!(n, b);
    let bright = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / rms(x);
    assert!(bright(&p[4_000..]) > bright(&b[4_000..]));
}

#[test]
fn all_sound_off_cuts_and_all_notes_off_releases() {
    let mut synth = Synth::new(Preset::Pad);
    render(&mut synth, &[on(0, 60, 100), on(0, 64, 100)], 9_600, 128);
    assert!(synth.active());
    synth.render(RATE, &[cc(0, 123, 0)], &mut [0.0; 128], &mut [0.0; 128]);
    let (tail, _) = render(&mut synth, &[], 9_600, 128);
    assert!(rms(&tail[..2_000]) > 0.0, "the release tail rings");
    render(&mut synth, &[], 96_000, 4_800);
    assert!(!synth.active());
    render(&mut synth, &[on(0, 60, 100)], 4_800, 128);
    synth.render(RATE, &[cc(0, 120, 0)], &mut [0.0; 1], &mut [0.0; 1]);
    assert!(!synth.active(), "all sound off cuts at once");
}

#[test]
fn every_preset_stays_finite_and_bounded_under_a_random_storm() {
    assert!(guard_is_active(), "guarded allocator not installed");
    for preset in Preset::ALL {
        let mut synth = Synth::new(preset);
        let mut seed = 99_u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed >> 8
        };
        let mut events = Vec::new();
        for i in 0..400_u32 {
            let offset = i * 100;
            match next() % 6 {
                0 | 1 => events.push(on(
                    offset,
                    24 + (next() % 80) as u8,
                    1 + (next() % 127) as u8,
                )),
                2 => events.push(off(offset, 24 + (next() % 80) as u8)),
                3 => events.push(cc(offset, 64, (next() % 128) as u8)),
                4 => events.push(Event {
                    offset,
                    msg: Msg::pitch_bend(0, (next() % 16_384) as i16 - 8_192),
                }),
                _ => events.push(cc(offset, 1, (next() % 128) as u8)),
            }
        }
        let (left, right) = render(&mut synth, &events, 50_000, 128);
        assert!(
            left.iter().chain(&right).all(|v| v.is_finite()),
            "{preset:?}"
        );
        assert!(peak(&left).max(peak(&right)) < 4.0, "{preset:?}");
    }
}

#[test]
fn presets_have_stable_ids() {
    let ids: Vec<&str> = Preset::ALL.iter().map(|p| p.id()).collect();
    assert_eq!(ids, ["poly", "keys", "bass", "pluck", "pad"]);
    for p in Preset::ALL {
        assert_eq!(Preset::from_id(p.id()), Some(p));
        assert_eq!(Preset::from_number(p.number()), Some(p));
    }
}

/// The phrase every preset's golden fixture plays: three notes, the last one
/// quiet, all released inside the half second the fixture covers.
fn phrase() -> Vec<Event> {
    vec![
        on(0, 60, 100),
        off(6_000, 60),
        on(6_000, 64, 100),
        off(12_000, 64),
        on(12_000, 55, 40),
        off(18_000, 55),
    ]
}

/// Frames one fixture covers.
const FIXTURE_FRAMES: usize = 24_000;

fn golden_path(preset: Preset) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("preset_{}.pcm", preset.id()))
}

/// The fixture as `(left, right)`, from 16-bit samples.
fn golden(preset: Preset) -> (Vec<f32>, Vec<f32>) {
    let bytes = std::fs::read(golden_path(preset))
        .unwrap_or_else(|e| panic!("{}: {e}", golden_path(preset).display()));
    let mut left = Vec::with_capacity(FIXTURE_FRAMES);
    let mut right = Vec::with_capacity(FIXTURE_FRAMES);
    for pair in bytes.as_chunks::<4>().0 {
        let l = i16::from_le_bytes([pair[0], pair[1]]);
        let r = i16::from_le_bytes([pair[2], pair[3]]);
        left.push(f32::from(l) / 32_767.0);
        right.push(f32::from(r) / 32_767.0);
    }
    (left, right)
}

/// How far the sound of a preset may be from the render this build's
/// predecessor made of it, sample by sample (M8 slice B4, ADR-XXXX): a
/// thousandth of full scale, 60 dB down. The old path worked its filter and
/// pitch out in `f32` through the library's `tan`, `exp2`, `sin` and `cos`;
/// this one works them out in `f64` by hand so a golden is the same on every
/// platform, and The difference is under a hundred-thousandth of full scale
/// everywhere. The fixtures are 16-bit, which quantises at 3e-5.
const PRESET_TOLERANCE: f32 = 1e-4;

/// The five presets, as parameters, sound as they always have: the old
/// path's own render of each is pinned in `tests/golden/` and this one is
/// within [`PRESET_TOLERANCE`] of it at every sample.
#[test]
fn the_five_presets_sound_as_they_did() {
    for preset in Preset::ALL {
        let mut synth = Synth::new(preset);
        let (left, right) = render(&mut synth, &phrase(), FIXTURE_FRAMES, 512);
        let (old_left, old_right) = golden(preset);
        let mut worst = 0.0f32;
        for i in 0..FIXTURE_FRAMES {
            worst = worst
                .max((left[i] - old_left[i]).abs())
                .max((right[i] - old_right[i]).abs());
        }
        assert!(
            worst <= PRESET_TOLERANCE,
            "{} differs from the old sound by {worst}",
            preset.name()
        );
        // And it is not silence against silence: the fixtures have a sound.
        assert!(peak(&old_left) > 0.01, "{} sounds", preset.name());
    }
}

#[test]
fn a_preview_is_what_comes_next_and_leaves_the_synth_where_it_was() {
    let mut a = Synth::new(Preset::Poly);
    let mut b = Synth::new(Preset::Poly);
    let events = [on(0, 60, 100), on(0, 64, 100)];
    render(&mut a, &events, 2_000, 128);
    render(&mut b, &events, 2_000, 128);
    // `a` previews 96 frames; `b` really plays them.
    let (mut pl, mut pr) = (vec![0.0; 96], vec![0.0; 96]);
    a.preview(RATE, &mut pl, &mut pr);
    let (bl, br) = render(&mut b, &[], 96, 96);
    assert_eq!((&pl, &pr), (&bl, &br), "the preview is what plays next");
    // Nothing moved: `a` now plays those same frames.
    let (al, ar) = render(&mut a, &[], 96, 96);
    assert_eq!((&al, &ar), (&bl, &br), "and the synth did not advance");
    // Another rate adds nothing.
    let (mut xl, mut xr) = (vec![0.0; 16], vec![0.0; 16]);
    a.preview(44_100, &mut xl, &mut xr);
    assert!(xl.iter().chain(&xr).all(|v| *v == 0.0));
}

/// An event on `channel`.
fn on_channel(offset: u32, channel: u8, key: u8, velocity: u8) -> Event {
    Event {
        offset,
        msg: Msg::note_on(channel, key, velocity),
    }
}

fn off_channel(offset: u32, channel: u8, key: u8) -> Event {
    Event {
        offset,
        msg: Msg::note_off(channel, key),
    }
}

fn controller_on(offset: u32, channel: u8, number: u8, value: u8) -> Event {
    Event {
        offset,
        msg: Msg::controller(channel, number, value),
    }
}

#[test]
fn the_same_key_on_two_channels_is_two_notes_and_a_preview_ending_leaves_the_clip() {
    // A clip's note on channel 0 and a preview of the same key on channel 15.
    let mut synth = Synth::new(Preset::Poly);
    render(
        &mut synth,
        &[on_channel(0, 0, 50, 100), on_channel(0, 15, 50, 80)],
        4_800,
        128,
    );
    assert_eq!(synth.voices(), 2, "both sound");
    // The preview lets go; its release ends, and the clip's note rings on.
    let (left, _) = render(&mut synth, &[off_channel(0, 15, 50)], 96_000, 128);
    assert_eq!(synth.voices(), 1, "only the clip's note is left");
    assert!(rms(&left[92_000..]) > 0.01, "and it still rings");
}

#[test]
fn a_note_off_ends_only_the_note_on_its_own_channel() {
    for (ended, kept) in [(0, 15), (15, 0)] {
        let mut synth = Synth::new(Preset::Poly);
        render(
            &mut synth,
            &[on_channel(0, 0, 50, 100), on_channel(0, 15, 50, 100)],
            4_800,
            128,
        );
        render(&mut synth, &[off_channel(0, ended, 50)], 96_000, 128);
        assert_eq!(synth.voices(), 1, "channel {kept} keeps its note");
    }
}

#[test]
fn the_same_key_again_on_one_channel_lets_the_first_go() {
    let mut synth = Synth::new(Preset::Poly);
    render(&mut synth, &[on_channel(0, 0, 50, 100)], 9_600, 128);
    // The same key again, once the first is sounding: it is released.
    render(&mut synth, &[on_channel(0, 0, 50, 100)], 128, 128);
    assert_eq!(synth.voices(), 2, "the first is in release, still counted");
    render(&mut synth, &[], 96_000, 128);
    assert_eq!(synth.voices(), 1, "the second holds on");
    render(&mut synth, &[off_channel(0, 0, 50)], 96_000, 128);
    assert_eq!(synth.voices(), 0);
}

#[test]
fn a_note_waiting_for_a_steal_is_matched_by_channel_and_key() {
    let mut synth = Synth::new(Preset::Poly);
    // Sixteen notes on channel 0; a seventeenth on channel 15 with the same
    // key takes the oldest voice (channel 0, key 40) once it has faded.
    let mut events: Vec<Event> = (0..16).map(|i| on_channel(0, 0, 40 + i, 100)).collect();
    events.push(on_channel(1_000, 15, 40, 100));
    // Channel 0's key 40 lets go while its voice is still fading (about
    // 2 ms): it must not cancel channel 15's waiting note.
    events.push(off_channel(1_050, 0, 40));
    render(&mut synth, &events, 9_600, 128);
    assert_eq!(synth.voices(), 16, "channel 15's note took the voice");
}

#[test]
fn all_notes_off_and_all_sound_off_still_reach_every_channel() {
    let mut synth = Synth::new(Preset::Poly);
    render(
        &mut synth,
        &[on_channel(0, 0, 60, 100), on_channel(0, 15, 64, 100)],
        4_800,
        128,
    );
    render(&mut synth, &[controller_on(0, 0, 123, 0)], 96_000, 128);
    assert_eq!(synth.voices(), 0, "all notes off releases channel 15 too");
    render(&mut synth, &[on_channel(0, 15, 64, 100)], 4_800, 128);
    synth.render(
        RATE,
        &[controller_on(0, 0, 120, 0)],
        &mut [0.0; 1],
        &mut [0.0; 1],
    );
    assert!(!synth.active(), "all sound off cuts channel 15 too");
}

// ---- The controls (M8 slice B4) -------------------------------------------

/// Renders `frames` with a lane on `id`, cut into blocks of `every`, the way
/// the engine fills each block: the value the lane has at the block's first
/// frame, its own points inside, and its value at the last frame.
fn render_lane(
    synth: &mut Synth,
    id: u32,
    curve: &dyn Fn(usize) -> f32,
    points: &[(u32, f32)],
    events: &[Event],
    frames: usize,
    every: usize,
) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    let mut at = 0;
    while at < frames {
        let n = every.min(frames - at);
        let mut block = lumen_devices::ParamBlock::new();
        block.start(n);
        assert!(block.open(id));
        assert!(block.point(0, f64::from(curve(at))));
        for &(offset, value) in points {
            let offset = offset as usize;
            if offset > at && offset < at + n {
                assert!(block.point((offset - at) as u32, f64::from(value)));
            }
        }
        if n > 1 {
            assert!(block.point((n - 1) as u32, f64::from(curve(at + n - 1))));
        }
        let local: Vec<Event> = events
            .iter()
            .filter(|e| (e.offset as usize) >= at && (e.offset as usize) < at + n)
            .map(|e| Event {
                offset: e.offset - at as u32,
                msg: e.msg,
            })
            .collect();
        synth.automate(&block);
        let ((), heap) = count_rt_violations(|| {
            synth.render(RATE, &local, &mut left[at..at + n], &mut right[at..at + n]);
        });
        assert_eq!(heap, 0, "heap operation in the synth at frame {at}");
        at += n;
    }
    (left, right)
}

/// The lane's value at a frame: straight between its points.
fn lane_at(points: &[(u32, f32)], frame: usize) -> f32 {
    let after = points.partition_point(|p| p.0 as usize <= frame);
    let Some(before) = after.checked_sub(1) else {
        return points[0].1;
    };
    match points.get(after) {
        Some(&(next, value)) if next as usize > points[before].0 as usize => {
            let (at, from) = points[before];
            let span = (next - at) as f32;
            from + (value - from) * (frame as f32 - at as f32) / span
        }
        _ => points[before].1,
    }
}

/// A lane on the cutoff is played on its own samples: the corner the filter
/// is given at frame `k` is the lane's value at frame `k`, exact on the
/// lane's own points.
#[test]
fn automating_the_cutoff_is_sample_exact() {
    use lumen_devices::synth::params::{FILTER_CUTOFF, cutoff_hz};
    let points: Vec<(u32, f32)> = vec![(0, 0.15), (64, 0.9), (128, 0.2), (255, 0.75)];
    // The value the project holds is the lane's first, so nothing glides into
    // it: what is measured is the lane alone.
    let mut synth = Synth::new(Preset::Poly);
    synth.configure_values(&flash_values(Preset::Poly, FILTER_CUTOFF, 87.432));
    let events = [on(0, 60, 100)];
    let curve = |frame: usize| lane_at(&points, frame);
    render_lane(
        &mut synth,
        FILTER_CUTOFF,
        &curve,
        &points,
        &events,
        256,
        256,
    );
    let played = synth.played_cutoff(256);
    for frame in [0usize, 1, 2, 63, 64, 65, 127, 128, 129, 200, 254, 255] {
        let expected = cutoff_hz(curve(frame));
        assert!(
            (played[frame] - expected).abs() < 1e-2,
            "frame {frame}: {} Hz against {} Hz",
            played[frame],
            expected
        );
    }
}

/// The same lane plays the same samples whatever block size the engine cuts
/// it into: 64, 97, 128, 240 and all 256 at once. The curves agree to the
/// arithmetic's own rounding (the same allowance the effects' own test
/// makes).
#[test]
fn a_lane_plays_the_same_at_every_block_size() {
    use lumen_devices::synth::params::FILTER_CUTOFF;
    let points: Vec<(u32, f32)> = vec![
        (0, 0.2),
        (37, 0.75),
        (38, 0.3),
        (150, 0.6),
        (151, 0.15),
        (255, 0.9),
    ];
    let curve = |frame: usize| lane_at(&points, frame);
    let events = [on(0, 60, 100)];
    let mut reference: Vec<(f32, f32)> = Vec::new();
    for block in [256usize, 240, 128, 97, 64, 33, 512, 1_024] {
        let mut synth = Synth::new(Preset::Poly);
        let (left, right) = render_lane(
            &mut synth,
            FILTER_CUTOFF,
            &curve,
            &points,
            &events,
            256,
            block,
        );
        let whole: Vec<(f32, f32)> = left.into_iter().zip(right).collect();
        if reference.is_empty() {
            reference = whole;
            continue;
        }
        for (k, ((a, b), (c, d))) in whole.iter().zip(&reference).enumerate() {
            assert!(
                (a - c).abs() < 2e-6 && (b - d).abs() < 2e-6,
                "block {block}, frame {k}: ({a}, {b}) against ({c}, {d})"
            );
        }
    }
}

/// The preset's values with one parameter replaced (normalised).
fn flash_values(preset: Preset, id: u32, value: f32) -> Vec<(u32, f32)> {
    flash_many(preset, &[(id, value)])
}

/// The preset's values with several parameters replaced, in their own units.
fn flash_many(preset: Preset, changes: &[(u32, f32)]) -> Vec<(u32, f32)> {
    let mut values = preset.values();
    for &(id, value) in changes {
        for slot in values.iter_mut() {
            if slot.0 == id {
                slot.1 = lumen_devices::synth::params::find(id)
                    .map_or(slot.1, |info| info.normalised(value));
            }
        }
    }
    values
}

/// How bright a stretch of sound is: its sample-to-sample change over its
/// level.
fn bright(x: &[f32]) -> f32 {
    x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / rms(x).max(1e-6)
}

/// A value the project holds reaches a sounding note by gliding, not by
/// cutting it: changing the cutoff mid-note never steps, and it does arrive.
#[test]
fn a_value_the_project_holds_glides_into_a_sounding_note() {
    use lumen_devices::synth::params::FILTER_CUTOFF;
    let mut synth = Synth::new(Preset::Poly);
    let (before, _) = render(&mut synth, &[on(0, 60, 100)], 9_600, 256);
    synth.configure_values(&flash_values(Preset::Poly, FILTER_CUTOFF, 200.0));
    assert!(synth.preset().is_none(), "the sound is no longer a preset");
    let (after, _) = render(&mut synth, &[], 9_600, 256);
    let steepest = |x: &[f32]| {
        x.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max)
    };
    // The join of the two halves is as smooth as either half.
    let joint = [before[8_000..].to_vec(), after.clone()].concat();
    let own = steepest(&before[8_000..]).max(steepest(&after));
    assert!(
        steepest(&joint) <= own * 1.05,
        "{} against {own}",
        steepest(&joint)
    );
    // And the held note is the new sound: much darker than it was.
    assert!(bright(&after[4_000..]) < bright(&before[4_000..]) * 0.6);
    assert_eq!(synth.voices(), 1, "the note never stopped");
}

/// Noise is noise: it adds a broadband hiss, and at nothing it adds none.
#[test]
fn the_noise_generator_adds_a_hiss() {
    use lumen_devices::synth::params::NOISE;
    let quiet = flash_values(Preset::Poly, NOISE, 0.0);
    let hiss = flash_values(Preset::Poly, NOISE, 60.0);
    let mut a = Synth::with_values(&quiet);
    let mut b = Synth::with_values(&hiss);
    let (plain, _) = render(&mut a, &[on(0, 60, 100)], 9_600, 256);
    let (noisy, _) = render(&mut b, &[on(0, 60, 100)], 9_600, 256);
    assert!(bright(&noisy[2_000..]) > bright(&plain[2_000..]) * 1.5);
    assert!(peak(&noisy) < 1.0);
}

/// The filter's four shapes are four different sounds, and the 24 dB low-pass
/// takes more of the top away than the 12 dB one. Measured at a 400 Hz
/// corner, where a saw's harmonics are in reach of all of them.
#[test]
fn every_filter_shape_is_its_own_sound() {
    use lumen_devices::synth::params::{FILTER_CUTOFF, FILTER_TYPE, find};
    let mut sounds = Vec::new();
    for kind in 0..4 {
        let values = flash_many(
            Preset::Poly,
            &[(FILTER_TYPE, kind as f32), (FILTER_CUTOFF, 400.0)],
        );
        let mut synth = Synth::with_values(&values);
        let (left, _) = render(&mut synth, &[on(0, 60, 100)], 9_600, 256);
        sounds.push(left);
    }
    // How much of the top each one keeps: its sample-to-sample change.
    let rough = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / x.len() as f32;
    let top: Vec<f32> = sounds.iter().map(|s| rough(&s[4_000..])).collect();
    assert!(
        top[1] < top[0] * 0.8,
        "24 dB takes more of the top: {top:?}"
    );
    assert!(
        top[2] > top[0] * 1.5,
        "the high-pass keeps only the top: {top:?}"
    );
    for a in 0..4 {
        for b in 0..4 {
            if a >= b {
                continue;
            }
            let difference = sounds[a][4_000..]
                .iter()
                .zip(&sounds[b][4_000..])
                .map(|(x, y)| (x - y).abs())
                .sum::<f32>()
                / 5_600.0;
            assert!(difference > 5e-3, "shapes {a} and {b} sound alike");
        }
    }
    assert_eq!(
        find(FILTER_TYPE).unwrap().choices,
        lumen_devices::synth::params::FILTERS
    );
}

/// The filter's envelope opens the corner and lets it fall back: it changes
/// the sound while it is open, and leaves it alone once it has decayed.
#[test]
fn the_filter_envelope_moves_the_corner() {
    use lumen_devices::synth::params::{FILTER_ATTACK, FILTER_DECAY, FILTER_ENV, FILTER_SUSTAIN};
    let flat = flash_many(Preset::Poly, &[(FILTER_ENV, 0.0)]);
    let moved = flash_many(
        Preset::Poly,
        &[
            (FILTER_ENV, 3.0),
            (FILTER_SUSTAIN, 0.0),
            (FILTER_DECAY, 0.05),
            (FILTER_ATTACK, 0.001),
        ],
    );
    let mut a = Synth::with_values(&flat);
    let mut b = Synth::with_values(&moved);
    let (plain, _) = render(&mut a, &[on(0, 60, 100)], 24_000, 256);
    let (env, _) = render(&mut b, &[on(0, 60, 100)], 24_000, 256);
    // How far the envelope moves the sound, as a share of its own level.
    let apart = |from: usize, to: usize| {
        let difference: f32 = env[from..to]
            .iter()
            .zip(&plain[from..to])
            .map(|(x, y)| (x - y).abs())
            .sum::<f32>()
            / (to - from) as f32;
        difference / rms(&plain[from..to]).max(1e-6)
    };
    assert!(
        apart(100, 1_200) > 0.05,
        "it opens the corner: {}",
        apart(100, 1_200)
    );
    assert!(
        apart(18_000, 22_000) < 0.01,
        "and falls back: {}",
        apart(18_000, 22_000)
    );
}

/// The LFO moves the pitch, the cutoff or the amp, and at nothing it does
/// nothing. At 1 Hz and full depth, a quarter of a second apart is an
/// opposite half of its cycle.
#[test]
fn the_lfo_reaches_its_three_destinations() {
    use lumen_devices::synth::params::{LFO_DEPTH, LFO_RATE, LFO_TARGET};
    let mut sounds = Vec::new();
    for target in 0..4 {
        let values = flash_many(
            Preset::Poly,
            &[
                (LFO_TARGET, target as f32),
                (LFO_DEPTH, 100.0),
                (LFO_RATE, 1.0),
            ],
        );
        let mut synth = Synth::with_values(&values);
        let (left, _) = render(&mut synth, &[on(0, 60, 100)], 48_000, 256);
        sounds.push(left);
    }
    let crossings = |x: &[f32]| x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    // A quarter cycle in (the sine's crest) against three quarters in.
    let (crest, trough) = (12_000..14_000, 36_000..38_000);
    assert!(
        (crossings(&sounds[1][crest.clone()]) as f32)
            > (crossings(&sounds[1][trough.clone()]) as f32) * 1.5,
        "the pitch is moved"
    );
    assert!(
        bright(&sounds[2][crest.clone()]) > bright(&sounds[2][trough.clone()]) * 1.3,
        "the cutoff sweeps"
    );
    assert!(
        peak(&sounds[3][trough.clone()]) < peak(&sounds[3][crest.clone()]) * 0.5,
        "the amp moves"
    );
    // Off: the two halves are the same sound.
    assert_eq!(
        crossings(&sounds[0][crest]),
        crossings(&sounds[0][trough]),
        "off does nothing"
    );
}

/// Glide takes a new note from the last one's pitch to its own.
#[test]
fn glide_slides_a_new_note_from_the_last() {
    use lumen_devices::synth::params::GLIDE;
    let sliding_values = flash_many(Preset::Poly, &[(GLIDE, 0.2)]);
    let mut sliding = Synth::with_values(&sliding_values);
    render(&mut sliding, &[on(0, 60, 100)], 1_200, 256);
    let (glided, _) = render(&mut sliding, &[on(0, 72, 100)], 24_000, 256);
    let mut straight = Synth::with_values(&flash_many(Preset::Poly, &[(GLIDE, 0.0)]));
    render(&mut straight, &[on(0, 60, 100)], 1_200, 256);
    let (plain, _) = render(&mut straight, &[on(0, 72, 100)], 24_000, 256);
    let crossings = |x: &[f32]| x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    // Just after it starts it is well below the note it was played at.
    assert!(
        (crossings(&glided[1_000..2_000]) as f32) < (crossings(&plain[1_000..2_000]) as f32) * 0.8,
        "the note starts low"
    );
    // Long after, it is the note that was played.
    let late = crossings(&glided[14_000..18_000]) as i32 - crossings(&plain[14_000..18_000]) as i32;
    assert!(late.abs() <= 3, "and arrives: {late}");
}

/// Voices limits what sounds at once, and a note a full synth cannot start
/// is counted rather than lost.
#[test]
fn voices_limit_what_sounds_and_a_lost_note_is_counted() {
    use lumen_devices::synth::params::VOICES;
    for limit in [1usize, 2, 8, 16] {
        let values = flash_values(Preset::Poly, VOICES, limit as f32);
        let mut synth = Synth::with_values(&values);
        let notes: Vec<Event> = (0..8).map(|i| on(i * 100, 48 + i as u8, 100)).collect();
        render(&mut synth, &notes, 9_600, 256);
        assert!(synth.voices() <= limit, "{limit}: {}", synth.voices());
    }
    // Every voice already fading out: the newest note takes a place and the
    // one it displaces is counted (KNOWN_ISSUES, the M5 review's row).
    let values = flash_values(Preset::Poly, VOICES, 1.0);
    let mut synth = Synth::with_values(&values);
    let mut left = vec![0.0f32; 64];
    let mut right = vec![0.0f32; 64];
    let events = [on(0, 60, 100), on(1, 62, 100), on(2, 64, 100)];
    synth.render(RATE, &events, &mut left, &mut right);
    assert!(synth.dropped() > 0, "a note no voice could take is counted");
    assert_eq!(synth.voices(), 1, "and the voice plays the newest");
}

/// Velocity is what it is set to: at nothing, a quiet note is as loud as a
/// loud one; at full, it is as quiet as it was played.
#[test]
fn velocity_sensitivity_is_a_control() {
    use lumen_devices::synth::params::VELOCITY;
    for (setting, expect) in [(0.0f32, 1.0f32), (100.0, 0.39)] {
        let values = flash_values(Preset::Poly, VELOCITY, setting);
        let mut loud = Synth::with_values(&values);
        let mut soft = Synth::with_values(&values);
        let (a, _) = render(&mut loud, &[on(0, 60, 127)], 9_600, 256);
        let (b, _) = render(&mut soft, &[on(0, 60, 50)], 9_600, 256);
        let ratio = rms(&b[4_000..]) / rms(&a[4_000..]);
        assert!(
            (ratio - expect).abs() < 0.1,
            "velocity {setting}: ratio {ratio}"
        );
    }
    // Velocity also opens the filter, by the amount the filter's own control
    // says.
    let soft = flash_many(
        Preset::Poly,
        &[(lumen_devices::synth::params::FILTER_VEL, 2.0)],
    );
    let mut loud_note = Synth::with_values(&soft);
    let mut quiet_note = Synth::with_values(&soft);
    let (l, _) = render(&mut loud_note, &[on(0, 60, 127)], 9_600, 256);
    let (q, _) = render(&mut quiet_note, &[on(0, 60, 30)], 9_600, 256);
    assert!(bright(&l[4_000..]) > bright(&q[4_000..]) * 1.1);
}

/// A synced LFO follows the tempo: twice the tempo, twice the rate.
#[test]
fn a_synced_lfo_follows_the_tempo() {
    use lumen_devices::synth::params::{LFO_DEPTH, LFO_DIVISION, LFO_SYNC, LFO_TARGET, find};
    let mut values = flash_values(Preset::Poly, LFO_TARGET, 3.0);
    for slot in &mut values {
        match slot.0 {
            LFO_SYNC => slot.1 = find(LFO_SYNC).unwrap().normalised(1.0),
            LFO_DIVISION => slot.1 = find(LFO_DIVISION).unwrap().normalised(2.0),
            LFO_DEPTH => slot.1 = find(LFO_DEPTH).unwrap().normalised(100.0),
            _ => {}
        }
    }
    let dips = |bpm: f32| {
        let mut synth = Synth::with_values(&values);
        synth.set_tempo(bpm);
        assert_eq!(synth.tempo(), bpm);
        let (left, _) = render(&mut synth, &[on(0, 60, 100)], 96_000, 256);
        // The tremolo's cycles, counted as the level rises through the middle
        // of its swing.
        let levels: Vec<f32> = left.chunks(2_000).map(peak).collect();
        let low = levels.iter().copied().fold(f32::MAX, f32::min);
        let high = levels.iter().copied().fold(0.0f32, f32::max);
        let middle = (low + high) / 2.0;
        levels
            .windows(2)
            .filter(|w| w[0] < middle && w[1] >= middle)
            .count()
    };
    let slow = dips(60.0);
    let fast = dips(120.0);
    assert!(slow >= 1, "60 bpm keeps time: {slow}");
    assert!(fast > slow, "120 bpm is faster: {slow} then {fast}");
}

/// FNV-1a 64: a stable hash, written here so a pin cannot drift with a
/// library.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A render of everything the controls do at once — the noise, the 24 dB
/// filter with its envelope, the LFO on the cutoff, glide, velocity and both
/// oscillators — pinned by hash. The synth's maths is `f64` arithmetic worked
/// out by hand, so this hash is the same on Windows, Linux and macOS (M8
/// slice B4).
/// The values the pinned render is made with: **literals**, not worked out
/// from the units, so a platform whose `powf` or `sqrt` rounds differently
/// cannot move the pin (review M1). Normalised, by id; the table's defaults
/// for every parameter not named.
const PINNED_INPUT: &[(u32, f32)] = &[
    (1, 1.0),           // Oscillator 1 shape: square
    (3, 0.37),          // Oscillator 1 detune
    (7, 0.25),          // Oscillator 2 octave: -1
    (11, 0.25),         // Noise
    (12, 0.333_333_34), // Filter type: Low 24
    (13, 0.353_152_6),  // Filter cutoff
    (14, 0.592_156_5),  // Resonance
    (15, 0.75),         // Filter envelope
    (16, 0.6),          // Filter key follow
    (18, 0.076_632_22), // Amp attack
    (19, 0.271_219_88), // Amp decay
    (20, 0.4),          // Amp sustain
    (21, 0.292_211_6),  // Amp release
    (29, 0.666_666_7),  // LFO target: Cutoff
    (27, 0.384_538_26), // LFO rate
    (30, 0.4),          // LFO depth
    (31, 0.1),          // Glide
    (32, 0.2),          // Voices: 4
    (33, 0.7),          // Velocity
];

#[test]
fn a_render_of_every_control_is_pinned() {
    // Poly's values with the input above over them: the sound every control
    // makes at once.
    let mut values = lumen_devices::Preset::Poly.values();
    for &(id, value) in PINNED_INPUT {
        if let Some(slot) = values.iter_mut().find(|(known, _)| *known == id) {
            slot.1 = value;
        }
    }
    let mut synth = Synth::with_values(&values);
    let events = [
        on(0, 48, 100),
        on(0, 55, 70),
        off(4_000, 48),
        on(4_000, 60, 120),
        off(9_000, 55),
        on(9_000, 43, 60),
        off(14_000, 60),
        off(14_000, 43),
    ];
    let (left, right) = render(&mut synth, &events, 24_000, 128);
    let mut bytes = Vec::with_capacity(24_000 * 8);
    for i in 0..24_000 {
        bytes.extend_from_slice(&left[i].to_bits().to_le_bytes());
        bytes.extend_from_slice(&right[i].to_bits().to_le_bytes());
    }
    let hash = fnv1a64(&bytes);
    assert_eq!(
        hash, 0x5397_ee9c_06c0_c347,
        "the pinned render has changed: {hash:016x}. A change here is a change of sound or of the \
         maths; it is deliberate, and the number is written down on purpose."
    );
}

/// A program change, a list of values and a block of lanes all run on the
/// audio thread, so none of them may reach the heap (review H1).
#[test]
fn loading_a_preset_and_taking_values_allocate_nothing() {
    assert!(guard_is_active(), "guarded allocator not installed");
    let mut synth = Synth::new(Preset::Poly);
    render(&mut synth, &[on(0, 60, 100)], 512, 256);
    // Built outside the guard: a block reserves its room when it is made.
    let values = flash_many(
        Preset::Bass,
        &[(lumen_devices::synth::params::FILTER_CUTOFF, 800.0)],
    );
    let mut block = lumen_devices::ParamBlock::new();
    block.start(128);
    assert!(block.open(lumen_devices::synth::params::FILTER_CUTOFF));
    assert!(block.point(0, 0.2));
    assert!(block.point(64, 0.9));
    let mut left = [0.0f32; 128];
    let mut right = [0.0f32; 128];
    let ((), heap) = count_rt_violations(|| {
        for preset in Preset::ALL {
            synth.configure(preset.number());
        }
        synth.configure_values(&values);
        synth.automate(&block);
        synth.render(RATE, &[], &mut left, &mut right);
    });
    assert_eq!(heap, 0, "a value change reached the heap");
}

/// Changing the filter's shape under a held note slides from one response to
/// the other: no sample jumps more than the sound itself does (review M3).
#[test]
fn a_filter_shape_change_does_not_click() {
    use lumen_devices::synth::params::{FILTER_TYPE, find};
    let steepest = |x: &[f32]| {
        x.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max)
    };
    for kind in [1.0f32, 2.0, 3.0] {
        let mut synth = Synth::new(Preset::Poly);
        let (before, _) = render(&mut synth, &[on(0, 60, 100)], 9_600, 256);
        synth.configure_values(&flash_many(Preset::Poly, &[(FILTER_TYPE, kind)]));
        let (after, _) = render(&mut synth, &[], 9_600, 256);
        let joint = [before[8_000..].to_vec(), after.clone()].concat();
        let own = steepest(&before[8_000..]).max(steepest(&after));
        assert!(
            steepest(&joint) <= own * 1.1,
            "shape {kind} steps {} against the sound's own {own}",
            steepest(&joint)
        );
        // And the new shape is what plays: brighter or darker than before.
        let bright =
            |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / rms(x).max(1e-6);
        assert!(
            (bright(&after[4_000..]) - bright(&before[4_000..])).abs() / bright(&before[4_000..])
                > 0.05,
            "shape {kind} is a different sound"
        );
    }
    assert_eq!(find(FILTER_TYPE).unwrap().name, "Filter type");
}

/// All notes off lets go of a note waiting for a voice to fade out: it must
/// never sound (review L6).
#[test]
fn all_notes_off_cancels_a_note_waiting_for_a_steal() {
    use lumen_devices::synth::params::VOICES;
    let values = flash_values(Preset::Poly, VOICES, 1.0);
    let mut synth = Synth::with_values(&values);
    // One voice: the second note waits for the first to fade out.
    let mut left = vec![0.0f32; 64];
    let mut right = vec![0.0f32; 64];
    synth.render(
        RATE,
        &[on(0, 60, 100), on(1, 64, 100)],
        &mut left,
        &mut right,
    );
    assert!(
        synth.voices() > 0,
        "the voice is fading out for the next note"
    );
    // All notes off: the waiting note is let go too.
    synth.render(RATE, &[cc(0, 123, 0)], &mut left, &mut right);
    let (tail, _) = render(&mut synth, &[], 9_600, 256);
    assert_eq!(synth.voices(), 0, "nothing starts after all notes off");
    assert!(
        tail[4_000..].iter().all(|v| v.abs() < 1e-4),
        "and nothing sounds"
    );
}
