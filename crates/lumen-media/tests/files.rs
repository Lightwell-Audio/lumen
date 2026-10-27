//! Decoding and conversion against files written by the tests themselves:
//! WAV (16-bit, 24-bit, float) and AIFF. FLAC, MP3 and Ogg Vorbis rely on
//! symphonia's own tests and the by-ear check (MT-MEDIA-1).

use std::f64::consts::TAU;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use lumen_media::{Audio, MediaError, decode, load, resample};

/// A unique file in the system temp folder, removed when dropped.
struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str, bytes: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "lumen-media-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `frames` of a sine at `hz` on channel 0 and its inverse on channel 1.
fn stereo_sine(rate: u32, hz: f64, frames: usize) -> Vec<[f64; 2]> {
    (0..frames)
        .map(|i| {
            let v = (TAU * hz * i as f64 / f64::from(rate)).sin() * 0.5;
            [v, -v]
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Pcm {
    I16,
    I24,
    F32,
}

fn wav(rate: u32, frames: &[[f64; 2]], pcm: Pcm) -> Vec<u8> {
    let (bits, format_tag): (u16, u16) = match pcm {
        Pcm::I16 => (16, 1),
        Pcm::I24 => (24, 1),
        Pcm::F32 => (32, 3),
    };
    let block = 2 * bits / 8;
    let mut data = Vec::new();
    for frame in frames {
        for &v in frame {
            match pcm {
                Pcm::I16 => data.extend(((v * 32_767.0).round() as i16).to_le_bytes()),
                Pcm::I24 => data.extend(&((v * 8_388_607.0).round() as i32).to_le_bytes()[..3]),
                Pcm::F32 => data.extend((v as f32).to_le_bytes()),
            }
        }
    }
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(format_tag.to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * u32::from(block)).to_le_bytes());
    out.extend(block.to_le_bytes());
    out.extend(bits.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

/// 80-bit IEEE extended, as AIFF stores its sample rate.
fn extended(value: f64) -> [u8; 10] {
    let exponent = value.log2().floor() as i32;
    let mantissa = (value / 2_f64.powi(exponent) * 2_f64.powi(63)) as u64;
    let mut out = [0; 10];
    out[..2].copy_from_slice(&((exponent + 16_383) as u16).to_be_bytes());
    out[2..].copy_from_slice(&mantissa.to_be_bytes());
    out
}

fn aiff(rate: u32, frames: &[[f64; 2]]) -> Vec<u8> {
    let mut data = Vec::new();
    for frame in frames {
        for &v in frame {
            data.extend(((v * 32_767.0).round() as i16).to_be_bytes());
        }
    }
    let mut comm = Vec::new();
    comm.extend(2_u16.to_be_bytes());
    comm.extend((frames.len() as u32).to_be_bytes());
    comm.extend(16_u16.to_be_bytes());
    comm.extend(extended(f64::from(rate)));
    let mut out = Vec::new();
    out.extend(b"FORM");
    out.extend((4 + 8 + comm.len() as u32 + 16 + data.len() as u32).to_be_bytes());
    out.extend(b"AIFFCOMM");
    out.extend((comm.len() as u32).to_be_bytes());
    out.extend(comm);
    out.extend(b"SSND");
    out.extend((8 + data.len() as u32).to_be_bytes());
    out.extend([0; 8]);
    out.extend(data);
    out
}

fn decode_file(file: &TempFile) -> Result<Audio, MediaError> {
    decode(&file.0, &AtomicBool::new(false), &mut |_| {})
}

fn assert_matches(audio: &Audio, expected: &[[f64; 2]], tolerance: f64) {
    assert_eq!(audio.channels(), 2);
    assert_eq!(audio.frames(), expected.len());
    for (i, frame) in expected.iter().enumerate() {
        for (c, &want) in frame.iter().enumerate() {
            let got = f64::from(audio.channel(c)[i]);
            assert!(
                (got - want).abs() <= tolerance,
                "frame {i} ch {c}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn wav_files_decode_to_their_samples() {
    let frames = stereo_sine(44_100, 440.0, 10_000);
    for (pcm, name, tolerance) in [
        (Pcm::I16, "16.wav", 1.0 / 32_767.0),
        (Pcm::I24, "24.wav", 1.0 / 8_388_607.0),
        (Pcm::F32, "f32.wav", 1e-7),
    ] {
        let file = TempFile::new(name, &wav(44_100, &frames, pcm));
        let audio = decode_file(&file).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(audio.sample_rate, 44_100, "{name}");
        assert_matches(&audio, &frames, tolerance);
    }
}

#[test]
fn aiff_files_decode_to_their_samples() {
    let frames = stereo_sine(48_000, 1_000.0, 4_800);
    let file = TempFile::new("a.aiff", &aiff(48_000, &frames));
    let audio = decode_file(&file).unwrap();
    assert_eq!(audio.sample_rate, 48_000);
    assert_matches(&audio, &frames, 1.0 / 32_767.0);
}

/// Rising zero crossings per second on channel 0.
fn frequency(audio: &Audio) -> f64 {
    let samples = audio.channel(0);
    let rising = samples
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    rising as f64 / audio.seconds()
}

#[test]
fn conversion_keeps_length_and_pitch() {
    let seconds = 2.0;
    for (from, to) in [(44_100, 48_000), (48_000, 44_100), (96_000, 48_000)] {
        let frames = stereo_sine(from, 440.0, (f64::from(from) * seconds) as usize);
        let file = TempFile::new("c.wav", &wav(from, &frames, Pcm::F32));
        let original = decode_file(&file).unwrap();
        let converted = resample(&original, to, &AtomicBool::new(false)).unwrap();
        assert_eq!(converted.sample_rate, to);
        let expected = (f64::from(to) * seconds) as i64;
        assert!(
            (converted.frames() as i64 - expected).abs() <= 2,
            "{from} -> {to}: {} frames, expected {expected}",
            converted.frames()
        );
        let hz = frequency(&converted);
        assert!((hz - 440.0).abs() < 1.0, "{from} -> {to}: {hz} Hz");
        // The level survives too (no gain change through the filter).
        let peak = converted
            .channel(0)
            .iter()
            .fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.01, "{from} -> {to}: peak {peak}");
    }
}

#[test]
fn load_decodes_converts_and_reports_progress() {
    let frames = stereo_sine(44_100, 440.0, 44_100);
    let file = TempFile::new("l.wav", &wav(44_100, &frames, Pcm::I16));
    let mut seen = Vec::new();
    let audio = load(&file.0, 48_000, &AtomicBool::new(false), &mut |p| {
        seen.push(p)
    })
    .unwrap();
    assert_eq!(audio.sample_rate, 48_000);
    assert_eq!(seen.last(), Some(&1.0));
    assert!(
        seen.windows(2).all(|w| w[0] <= w[1]),
        "progress went backwards"
    );
}

#[test]
fn a_cancelled_load_stops() {
    let frames = stereo_sine(44_100, 440.0, 44_100);
    let file = TempFile::new("x.wav", &wav(44_100, &frames, Pcm::I16));
    let result = load(&file.0, 48_000, &AtomicBool::new(true), &mut |_| {});
    assert_eq!(result, Err(MediaError::Cancelled));
}

#[test]
fn bad_files_are_errors_not_panics() {
    let text = TempFile::new("notes.txt", b"not audio at all");
    assert!(matches!(
        decode_file(&text),
        Err(MediaError::Unsupported(_))
    ));

    let mut truncated = wav(44_100, &stereo_sine(44_100, 440.0, 1_000), Pcm::I16);
    truncated.truncate(30);
    let truncated = TempFile::new("t.wav", &truncated);
    assert!(decode_file(&truncated).is_err());

    let empty = TempFile::new("e.wav", &wav(44_100, &[], Pcm::I16));
    assert!(decode_file(&empty).is_err());

    let missing = std::env::temp_dir().join("lumen-media-no-such-file.wav");
    assert!(matches!(
        decode(&missing, &AtomicBool::new(false), &mut |_| {}),
        Err(MediaError::Io(_))
    ));
}

#[test]
fn a_header_is_read_without_decoding() {
    let frames = stereo_sine(44_100, 440.0, 10_000);
    for (name, bytes) in [
        ("h.wav", wav(44_100, &frames, Pcm::I16)),
        ("h.aif", aiff(44_100, &frames)),
    ] {
        let file = TempFile::new(name, &bytes);
        let header = lumen_media::probe(&file.0).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(header.sample_rate, Some(44_100), "{name}");
        assert_eq!(header.channels, Some(2), "{name}");
        assert_eq!(header.frames, Some(10_000), "{name}");
    }
    let text = TempFile::new("h.txt", b"not audio");
    assert!(lumen_media::probe(&text.0).is_err());
    let missing = std::env::temp_dir().join("lumen-media-no-such-header.wav");
    assert!(matches!(
        lumen_media::probe(&missing),
        Err(MediaError::Io(_))
    ));
}
