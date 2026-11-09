use super::*;

#[test]
fn a_tempo_in_a_name_is_a_number_beside_bpm() {
    for (name, want) in [
        ("Funk Loop 98bpm", Some(98.0)),
        ("Funk Loop 98 BPM", Some(98.0)),
        ("drums_120_bpm", Some(120.0)),
        ("drums-bpm-92.5", Some(92.5)),
        ("BPM140 hats", Some(140.0)),
        ("Pad (bpm 85)", Some(85.0)),
        ("Kick 808", None),
        ("Take 120", None),
        ("bpm", None),
        ("Loop 5bpm", None),
        ("Loop 5000 bpm", None),
        ("", None),
    ] {
        assert_eq!(in_name(name), want, "{name:?}");
    }
}

/// A WAV file with an "acid" chunk between the format and the sound.
fn wav_with_loop_tag(path: &Path, flags: u32, bpm: f32) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF\0\0\0\0WAVE");
    bytes.extend_from_slice(b"fmt \x10\0\0\0\x01\0\x01\0\x80\xbb\0\0\0\x77\x01\0\x02\0\x10\0");
    // An odd-length chunk first: its pad byte must be skipped.
    bytes.extend_from_slice(b"junk\x03\0\0\0abc\0");
    bytes.extend_from_slice(b"acid\x18\0\0\0");
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&bpm.to_le_bytes());
    bytes.extend_from_slice(b"data\x04\0\0\0\0\0\0\0");
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn a_wav_loop_tag_gives_its_tempo_and_whether_it_is_a_one_shot() {
    let dir = std::env::temp_dir().join(format!("lumen-tempo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("loop.wav");
    wav_with_loop_tag(&a, 0, 93.5);
    assert_eq!(
        in_file(&a),
        Some(FileTempo {
            bpm: 93.5,
            one_shot: false
        })
    );
    let b = dir.join("shot.wav");
    wav_with_loop_tag(&b, 1, 120.0);
    assert_eq!(in_file(&b).map(|t| t.one_shot), Some(true));
    // A tempo outside Lumen's limits, and a file that is not one, say nothing.
    let c = dir.join("odd.wav");
    wav_with_loop_tag(&c, 0, 5.0);
    assert_eq!(in_file(&c), None);
    let d = dir.join("text.wav");
    std::fs::write(&d, b"not a wave file at all").unwrap();
    assert_eq!(in_file(&d), None);
    assert_eq!(in_file(&dir.join("missing.wav")), None);
    // A one-shot stays one when its own tempo is no use and the name has one
    // (review round 1), and a loop takes the name's tempo as a loop.
    let e = dir.join("Hit 120 BPM.wav");
    wav_with_loop_tag(&e, 1, 0.0);
    assert_eq!(
        stated(&e),
        Some(FileTempo {
            bpm: 120.0,
            one_shot: true
        })
    );
    assert_eq!(in_file(&e), None, "the name is not the file's own");
    let f = dir.join("Pad 90 BPM.wav");
    wav_with_loop_tag(&f, 0, f32::NAN);
    assert_eq!(
        stated(&f),
        Some(FileTempo {
            bpm: 90.0,
            one_shot: false
        })
    );
    let _ = std::fs::remove_dir_all(&dir);
}
