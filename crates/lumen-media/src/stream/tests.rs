use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::cache::Cache;
use super::convert::Converter;
use super::*;
use crate::take::TakeWriter;
use crate::wav::{Sample, Spec, Writer};

fn folder(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lumen-stream-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn signal(frames: usize, step: f32) -> Vec<f32> {
    (0..frames).map(|i| (i as f32 * step).sin() * 0.8).collect()
}

fn write_wav(path: &Path, rate: u32, sample: Sample, channels: &[Vec<f32>]) {
    let mut writer = Writer::create(
        path,
        Spec {
            sample_rate: rate,
            channels: channels.len() as u16,
            sample,
        },
    )
    .unwrap();
    let parts: Vec<&[f32]> = channels.iter().map(Vec::as_slice).collect();
    writer.write(&parts).unwrap();
    writer.finish().unwrap();
}

fn decoded(path: &Path) -> crate::Audio {
    crate::decode(path, &AtomicBool::new(false), &mut |_| {}).unwrap()
}

/// Reads `count` frames from `from` of every played channel.
fn read(path: &Path, from: u64, count: usize) -> Vec<Vec<f32>> {
    let mut reader = Reader::open(path).unwrap();
    let mut out = vec![vec![f32::NAN; count]; reader.layout().played()];
    let mut parts: Vec<&mut [f32]> = out.iter_mut().map(Vec::as_mut_slice).collect();
    reader.read(from, &mut parts).unwrap();
    out
}

/// A WAV file by hand: `tag` 1 (PCM), 3 (float), 2 (compressed), or 0xFFFE
/// (extensible, with `sub` as its sub-format).
fn wav_bytes(tag: u16, sub: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
    let block = channels * bits.div_ceil(8);
    let mut fmt = Vec::new();
    fmt.extend(tag.to_le_bytes());
    fmt.extend(channels.to_le_bytes());
    fmt.extend(rate.to_le_bytes());
    fmt.extend((rate * u32::from(block)).to_le_bytes());
    fmt.extend(block.to_le_bytes());
    fmt.extend(bits.to_le_bytes());
    if tag == 0xFFFE {
        fmt.extend(22_u16.to_le_bytes());
        fmt.extend(bits.to_le_bytes());
        fmt.extend(3_u32.to_le_bytes());
        fmt.extend(sub.to_le_bytes());
        fmt.extend([
            0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
        ]);
    }
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((4 + 12 + 8 + fmt.len() as u32 + 8 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVE");
    // A chunk Lumen does not use, of odd length, to be skipped with its pad.
    out.extend(b"junk");
    out.extend(3_u32.to_le_bytes());
    out.extend([1, 2, 3, 0]);
    out.extend(b"fmt ");
    out.extend((fmt.len() as u32).to_le_bytes());
    out.extend(fmt);
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

#[test]
fn written_wavs_read_anywhere_exactly_as_decoded() {
    let dir = folder("wavs");
    let channels = vec![signal(10_000, 0.031), signal(10_000, 0.017)];
    for sample in [Sample::I16, Sample::I24, Sample::F32] {
        let path = dir.join(format!("{sample:?}.wav"));
        write_wav(&path, 44_100, sample, &channels);
        let whole = decoded(&path);
        let layout = layout(&path).unwrap().unwrap();
        assert_eq!(layout.frames, 10_000);
        assert_eq!(layout.sample_rate, 44_100);
        for (from, count) in [(0, 512), (1_234, 4_000), (9_900, 100)] {
            let got = read(&path, from, count);
            for (c, got) in got.iter().enumerate() {
                let want = &whole.channel(c)[from as usize..from as usize + count];
                assert_eq!(got.as_slice(), want, "{sample:?} from {from}");
            }
        }
    }
}

#[test]
fn frames_past_the_end_are_silence() {
    let dir = folder("end");
    let path = dir.join("short.wav");
    write_wav(&path, 48_000, Sample::F32, &[signal(100, 0.1)]);
    let mut reader = Reader::open(&path).unwrap();
    let mut out = vec![f32::NAN; 64];
    assert_eq!(reader.read(80, &mut [&mut out]).unwrap(), 20);
    assert_eq!(out[19], signal(100, 0.1)[99]);
    assert!(out[20..].iter().all(|&v| v == 0.0));
    assert_eq!(reader.read(1_000, &mut [&mut out]).unwrap(), 0);
    assert!(out.iter().all(|&v| v == 0.0));
}

#[test]
fn other_pcm_layouts_read_as_the_decoder_reads_them() {
    let dir = folder("pcm");
    let ints: Vec<i32> = (0..600_i32)
        .map(|i| i.wrapping_mul(7_654_321) ^ (i << 20))
        .collect();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "i32",
            wav_bytes(
                1,
                0,
                2,
                48_000,
                32,
                &ints
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            ),
        ),
        (
            "u8",
            wav_bytes(
                1,
                0,
                1,
                48_000,
                8,
                &ints.iter().map(|v| *v as u8).collect::<Vec<_>>(),
            ),
        ),
        (
            "f64",
            wav_bytes(
                3,
                0,
                2,
                48_000,
                64,
                &ints
                    .iter()
                    .flat_map(|v| (f64::from(*v) / 3e9).to_le_bytes())
                    .collect::<Vec<_>>(),
            ),
        ),
        (
            "extensible-i24",
            wav_bytes(
                0xFFFE,
                1,
                2,
                48_000,
                24,
                &ints
                    .iter()
                    .flat_map(|v| v.to_le_bytes()[..3].to_vec())
                    .collect::<Vec<_>>(),
            ),
        ),
    ];
    for (name, bytes) in cases {
        let path = dir.join(format!("{name}.wav"));
        fs::write(&path, bytes).unwrap();
        let whole = decoded(&path);
        let got = read(&path, 0, whole.frames());
        for (c, got) in got.iter().enumerate() {
            assert_eq!(got.as_slice(), whole.channel(c), "{name}, channel {c}");
        }
    }
}

#[test]
fn compressed_and_foreign_files_are_converted_instead() {
    let dir = folder("foreign");
    let adpcm = dir.join("adpcm.wav");
    fs::write(&adpcm, wav_bytes(2, 0, 1, 48_000, 4, &[0; 64])).unwrap();
    assert_eq!(layout(&adpcm).unwrap(), None);
    let text = dir.join("notes.txt");
    fs::write(&text, "not audio at all").unwrap();
    assert_eq!(layout(&text).unwrap(), None);
    assert!(matches!(
        Reader::open(&text),
        Err(MediaError::Unsupported(_))
    ));
}

fn aiff_bytes(kind: Option<&[u8; 4]>, channels: u16, bits: u16, data: &[u8]) -> Vec<u8> {
    let frames = data.len() as u32 / (u32::from(channels) * u32::from(bits.div_ceil(8)));
    let mut comm = Vec::new();
    comm.extend(channels.to_be_bytes());
    comm.extend(frames.to_be_bytes());
    comm.extend(bits.to_be_bytes());
    // 48,000 as an 80-bit extended number.
    comm.extend([0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]);
    if let Some(kind) = kind {
        comm.extend(kind);
        comm.extend([0, 0]);
    }
    let mut out = Vec::new();
    out.extend(b"FORM");
    out.extend((4 + 8 + comm.len() as u32 + 8 + 8 + data.len() as u32).to_be_bytes());
    out.extend(if kind.is_some() { b"AIFC" } else { b"AIFF" });
    out.extend(b"COMM");
    out.extend((comm.len() as u32).to_be_bytes());
    out.extend(comm);
    out.extend(b"SSND");
    out.extend((8 + data.len() as u32).to_be_bytes());
    out.extend([0; 8]);
    out.extend(data);
    out
}

#[test]
fn aiff_reads_big_endian_swapped_and_float_samples() {
    let dir = folder("aiff");
    let values: Vec<i16> = (0..400).map(|i| (i * 163 - 30_000) as i16).collect();
    let plain = dir.join("plain.aiff");
    fs::write(
        &plain,
        aiff_bytes(
            None,
            2,
            16,
            &values
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<_>>(),
        ),
    )
    .unwrap();
    let whole = decoded(&plain);
    assert_eq!(layout(&plain).unwrap().unwrap().sample_rate, 48_000);
    let got = read(&plain, 0, whole.frames());
    assert_eq!(got[0].as_slice(), whole.channel(0));
    assert_eq!(got[1].as_slice(), whole.channel(1));

    let want: Vec<f32> = values.iter().map(|&v| f32::from(v) / 32_768.0).collect();
    let swapped = dir.join("swapped.aifc");
    fs::write(
        &swapped,
        aiff_bytes(
            Some(b"sowt"),
            1,
            16,
            &values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        ),
    )
    .unwrap();
    assert_eq!(read(&swapped, 0, values.len())[0], want);
    let float = dir.join("float.aifc");
    fs::write(
        &float,
        aiff_bytes(
            Some(b"fl32"),
            1,
            32,
            &want
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<_>>(),
        ),
    )
    .unwrap();
    assert_eq!(read(&float, 0, values.len())[0], want);
}

#[test]
fn a_take_being_written_reads_to_its_last_refresh() {
    let dir = folder("take");
    let path = dir.join("take.wav");
    let mut take = TakeWriter::create(&path, 48_000, 1).unwrap();
    let first = signal(3_000, 0.05);
    take.write(&[&first]).unwrap();
    take.refresh().unwrap();
    take.write(&[&signal(500, 0.2)]).unwrap();
    let layout = layout(&path).unwrap().unwrap();
    assert_eq!(
        layout.frames, 3_000,
        "audio after the refresh is not named yet"
    );
    assert_eq!(read(&path, 0, 3_000)[0], first);
    drop(take);
}

#[test]
fn rf64_names_its_length_in_ds64() {
    let dir = folder("rf64");
    let samples: Vec<f32> = signal(1_000, 0.07);
    let data: Vec<u8> = samples.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut bytes = Vec::new();
    bytes.extend(b"RF64");
    bytes.extend(u32::MAX.to_le_bytes());
    bytes.extend(b"WAVE");
    bytes.extend(b"ds64");
    bytes.extend(28_u32.to_le_bytes());
    bytes.extend(0_u64.to_le_bytes());
    bytes.extend((data.len() as u64).to_le_bytes());
    bytes.extend(1_000_u64.to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    bytes.extend(b"fmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(3_u16.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(48_000_u32.to_le_bytes());
    bytes.extend((48_000_u32 * 4).to_le_bytes());
    bytes.extend(4_u16.to_le_bytes());
    bytes.extend(32_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(u32::MAX.to_le_bytes());
    bytes.extend(&data);
    // Something after the audio that is not audio.
    bytes.extend(b"LIST\x04\x00\x00\x00abcd");
    let path = dir.join("long.wav");
    fs::write(&path, bytes).unwrap();
    let layout = layout(&path).unwrap().unwrap();
    assert_eq!(layout.frames, 1_000);
    assert_eq!(read(&path, 0, 1_000)[0], samples);
}

/// Converts in blocks of the sizes in `steps`, round and round.
fn in_blocks(from: u32, to: u32, channels: &[Vec<f32>], steps: &[usize]) -> Vec<Vec<f32>> {
    let mut converter = Converter::new(from, to, channels.len()).unwrap();
    let mut out = vec![Vec::new(); channels.len()];
    let mut sink = |block: &[&[f32]]| {
        for (all, part) in out.iter_mut().zip(block) {
            all.extend_from_slice(part);
        }
        Ok(())
    };
    let mut at = 0;
    for &step in steps.iter().cycle() {
        if at >= channels[0].len() {
            break;
        }
        let end = (at + step).min(channels[0].len());
        let parts: Vec<&[f32]> = channels.iter().map(|c| &c[at..end]).collect();
        converter.push(&parts, &mut sink).unwrap();
        at = end;
    }
    converter.finish(&mut sink).unwrap();
    out
}

#[test]
fn converting_in_blocks_matches_converting_at_once() {
    for (from, to) in [
        (44_100, 48_000),
        (48_000, 44_100),
        (96_000, 48_000),
        (48_000, 48_000),
    ] {
        for frames in [1, 100, 4_095, 4_096, 4_097, 8_192, 8_193, 20_000] {
            let channels = vec![signal(frames, 0.013), signal(frames, 0.029)];
            let whole = crate::resample(
                &crate::Audio::new(from, channels.clone()).unwrap(),
                to,
                &AtomicBool::new(false),
            )
            .unwrap();
            for steps in [&[frames][..], &[1_000, 37, 5_000], &[4_096]] {
                let got = in_blocks(from, to, &channels, steps);
                for (c, got) in got.iter().enumerate() {
                    assert_eq!(
                        got.as_slice(),
                        whole.channel(c),
                        "{from} to {to}, {frames} frames in steps of {steps:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_cache_converts_once_and_plays_what_memory_plays() {
    let dir = folder("cache");
    let source = dir.join("source.wav");
    write_wav(
        &source,
        44_100,
        Sample::I24,
        &[signal(30_000, 0.01), signal(30_000, 0.02)],
    );
    let cache = Cache::new(dir.join("cache"));
    let cancel = AtomicBool::new(false);
    assert_eq!(cache.find(&source, 48_000).unwrap(), None);

    let prepared = cache
        .prepare(&source, 48_000, &cancel, &mut |_| {})
        .unwrap();
    assert!(prepared.converted);
    assert_eq!(prepared.layout.sample_rate, 48_000);
    let memory = crate::load(&source, 48_000, &cancel, &mut |_| {}).unwrap();
    assert_eq!(prepared.layout.frames as usize, memory.frames());
    let streamed = read(&prepared.path, 0, memory.frames());
    assert_eq!(streamed[0].as_slice(), memory.channel(0));
    assert_eq!(streamed[1].as_slice(), memory.channel(1));

    // Found again without converting.
    assert_eq!(cache.find(&source, 48_000).unwrap(), Some(prepared.clone()));
    // At its own rate the source streams as it is.
    let direct = cache.find(&source, 44_100).unwrap().unwrap();
    assert!(!direct.converted);
    assert_eq!(direct.path, source);
}

#[test]
fn a_changed_source_is_converted_again() {
    let dir = folder("changed");
    let source = dir.join("source.wav");
    write_wav(&source, 44_100, Sample::F32, &[signal(5_000, 0.01)]);
    let cache = Cache::new(dir.join("cache"));
    let cancel = AtomicBool::new(false);
    let first = cache
        .prepare(&source, 48_000, &cancel, &mut |_| {})
        .unwrap();
    write_wav(&source, 44_100, Sample::F32, &[signal(9_000, 0.03)]);
    assert_eq!(cache.find(&source, 48_000).unwrap(), None);
    let second = cache
        .prepare(&source, 48_000, &cancel, &mut |_| {})
        .unwrap();
    assert_ne!(first.path, second.path);
    let memory = crate::load(&source, 48_000, &cancel, &mut |_| {}).unwrap();
    assert_eq!(
        read(&second.path, 0, memory.frames())[0].as_slice(),
        memory.channel(0)
    );
}

#[test]
fn a_cancelled_conversion_leaves_nothing_that_is_found() {
    let dir = folder("cancel");
    let source = dir.join("source.wav");
    write_wav(&source, 44_100, Sample::F32, &[signal(50_000, 0.01)]);
    let cache = Cache::new(dir.join("cache"));
    let cancel = AtomicBool::new(false);
    let result = cache.prepare(&source, 48_000, &cancel, &mut |p| {
        if p > 0.0 {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert_eq!(result, Err(MediaError::Cancelled));
    assert_eq!(cache.find(&source, 48_000).unwrap(), None);
    let left: Vec<_> = fs::read_dir(cache.dir()).unwrap().collect();
    assert!(left.is_empty(), "no copy or key is left: {left:?}");
}

#[test]
fn peaks_read_in_blocks_equal_the_peaks_of_the_file_in_memory() {
    let dir = folder("peaks");
    let cancel = AtomicBool::new(false);
    // Longer than one scan block, and not a whole number of buckets.
    let frames = 2 * SCAN + 1_234;
    for channels in [
        vec![signal(frames, 0.01)],
        vec![signal(frames, 0.01), signal(frames, 0.002)],
    ] {
        let path = dir.join(format!("{}.wav", channels.len()));
        write_wav(&path, 48_000, Sample::I24, &channels);
        let want = decoded(&path).peaks(&cancel).unwrap();
        let mut last = 0.0;
        let got = peaks(&path, &cancel, &mut |p| last = p).unwrap().unwrap();
        assert_eq!(got, want, "{} channels", channels.len());
        assert_eq!(last, 1.0);
    }
    cancel.store(true, Ordering::Relaxed);
    assert_eq!(peaks(&dir.join("1.wav"), &cancel, &mut |_| {}), Ok(None));
}

#[test]
fn a_reversed_copy_holds_the_frames_in_the_other_order_and_is_made_once() {
    let dir = folder("reversed");
    let source = dir.join("source.wav");
    // More than one block, so blocks are reversed and joined.
    let frames = 70_000;
    let channels = vec![signal(frames, 0.01), signal(frames, 0.003)];
    write_wav(&source, 48_000, Sample::I16, &channels);
    let cache = Cache::new(dir.join("cache"));
    let cancel = AtomicBool::new(false);
    let made = cache.reversed(&source, &cancel).unwrap();
    assert!(made.converted);
    assert_eq!(made.layout.frames, frames as u64);
    let forward = read(&source, 0, frames);
    let back = read(&made.path, 0, frames);
    for c in 0..2 {
        let mut want = forward[c].clone();
        want.reverse();
        assert_eq!(back[c], want, "channel {c}");
    }
    let modified = fs::metadata(&made.path).unwrap().modified().unwrap();
    let again = cache.reversed(&source, &cancel).unwrap();
    assert_eq!(again.path, made.path);
    assert_eq!(
        fs::metadata(&again.path).unwrap().modified().unwrap(),
        modified
    );

    cancel.store(true, Ordering::Relaxed);
    let other = dir.join("other.wav");
    write_wav(&other, 48_000, Sample::F32, &[signal(1_000, 0.1)]);
    assert_eq!(cache.reversed(&other, &cancel), Err(MediaError::Cancelled));
}
