//! The built-in synth: sound at the exact frame, envelopes, the pedal,
//! pitch bend, voice stealing, presets and a fuzz for numbers gone wrong.

use lumen_core::midi::{Event, Msg};
use lumen_devices::{Instrument, Preset, Synth};

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
        synth.render(RATE, &local, &mut left[at..at + n], &mut right[at..at + n]);
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
    for block in [32, 77, 128, 1_000] {
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
    assert_eq!(synth.preset(), Preset::Bass);
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
