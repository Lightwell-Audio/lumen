//! Smoke tests of the real binary (ADR-0029): `lumen` starts with its window
//! and GPU, draws, imports, saves, opens and closes, as a user would run
//! it. Each run uses its own settings file and autosave folder and the
//! silent test device, so it never touches the user's settings, autosaves
//! or sound card (ASIO devices are often exclusive).
//!
//! They need a desktop and a GPU, so they are ignored in CI and run at the
//! end of every slice (DEVELOPMENT.md, "End-to-end tests"):
//! `cargo test -p lumen-app --test smoke -- --ignored` (a debug build, so it
//! never clashes with a release Lumen that is running).
//! A window flashes up for each.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use lumen_project::testing::TempDir;

/// The longest a run may take before it counts as hung.
const LIMIT: Duration = Duration::from_secs(90);

/// Runs `lumen` with `args`, isolated in `dir`, and returns its output once
/// it exits. Panics if it hangs or fails.
fn lumen(dir: &Path, args: &[&str]) -> String {
    let settings = dir.join("settings.json");
    let autosave = dir.join("autosave");
    let mut child = Command::new(env!("CARGO_BIN_EXE_lumen"))
        .args(["--audio", "test:fake"])
        .arg("--settings")
        .arg(&settings)
        .arg("--autosave-dir")
        .arg(&autosave)
        .arg("--crash-dir")
        .arg(dir.join("crashes"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start lumen");
    let started = Instant::now();
    while child.try_wait().expect("poll lumen").is_none() {
        if started.elapsed() > LIMIT {
            let _ = child.kill();
            panic!("lumen {args:?} did not finish within {LIMIT:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let Output {
        status,
        stdout,
        stderr,
    } = child.wait_with_output().expect("lumen output");
    let stdout = String::from_utf8_lossy(&stdout).into_owned();
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(
        status.success(),
        "lumen {args:?} failed ({status}):\n{stdout}\n{stderr}"
    );
    assert!(!stderr.contains("panicked"), "lumen {args:?}:\n{stderr}");
    stdout
}

/// A stereo WAV of `seconds` of a quiet tone at 48 kHz.
fn wav(path: &Path, seconds: f32) -> PathBuf {
    let rate = 48_000_u32;
    let frames = (seconds * rate as f32) as usize;
    let data: Vec<u8> = (0..frames)
        .flat_map(|i| {
            let [a, b] = (((i as f32 * 0.05).sin() * 3_000.0) as i16).to_le_bytes();
            [a, b, a, b]
        })
        .collect();
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16_u32.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(2_u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 4).to_le_bytes());
    out.extend(4_u16.to_le_bytes());
    out.extend(16_u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).expect("write the test WAV");
    path.to_path_buf()
}

/// How many distinct colours a PNG has (a blank window has one or two).
fn colours(png: &Path) -> usize {
    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(png).expect("open the screenshot"),
    ));
    let mut reader = decoder.read_info().expect("a PNG");
    let mut pixels = vec![0; reader.output_buffer_size().expect("a size")];
    let info = reader.next_frame(&mut pixels).expect("decode");
    let mut seen: Vec<&[u8]> = pixels[..info.buffer_size()].chunks(4).collect();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

#[test]
#[ignore = "needs a desktop and a GPU: run at the end of every slice"]
fn starts_draws_and_closes() {
    let dir = TempDir::new("smoke-start");
    let out = lumen(dir.path(), &["--smoke-test"]);
    assert!(out.contains("smoke test: 30 frames rendered"), "{out}");
}

#[test]
#[ignore = "needs a desktop and a GPU: run at the end of every slice"]
fn imports_saves_opens_and_draws_a_project() {
    let dir = TempDir::new("smoke-project");
    let audio = wav(&dir.path().join("loop.wav"), 3.0);
    let song = dir.path().join("Song");
    let out = lumen(
        dir.path(),
        &[
            "--open-audio",
            audio.to_str().expect("a UTF-8 path"),
            "--save-as",
            song.to_str().expect("a UTF-8 path"),
        ],
    );
    assert!(out.contains("saved"), "{out}");
    let file = std::fs::read_dir(&song)
        .ok()
        .and_then(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .find(|p| p.extension().is_some_and(|x| x == "lumen"))
        })
        .or_else(|| Some(song.with_extension("lumen")).filter(|p| p.exists()))
        .unwrap_or_else(|| panic!("no project saved for {}:\n{out}", song.display()));
    let project = lumen_project::from_bytes(&std::fs::read(&file).expect("read the project"))
        .expect("a valid project");
    assert_eq!(project.tracks.len(), 1);
    assert_eq!(project.tracks[0].clips.len(), 1);

    // Opened again, it draws its clip and waveform.
    let shot = dir.path().join("song.png");
    let out = lumen(
        dir.path(),
        &[
            file.to_str().expect("a UTF-8 path"),
            "--screenshot",
            shot.to_str().expect("a UTF-8 path"),
        ],
    );
    assert!(out.contains("screenshot saved"), "{out}");
    assert!(colours(&shot) > 50, "the window looks blank");
}

#[test]
#[ignore = "needs a desktop and a GPU: run at the end of every slice"]
fn the_quality_bar_project_draws() {
    let dir = TempDir::new("smoke-stress");
    let out = lumen(dir.path(), &["--stress", "--smoke-test"]);
    assert!(out.contains("smoke test: 30 frames rendered"), "{out}");
}

#[test]
#[ignore = "needs a desktop and a GPU: run at the end of every slice"]
fn the_light_theme_at_200_percent_draws() {
    let dir = TempDir::new("smoke-light");
    let shot = dir.path().join("light.png");
    let out = lumen(
        dir.path(),
        &[
            "--appearance",
            "light",
            "--scale",
            "2",
            "--screenshot",
            shot.to_str().expect("a UTF-8 path"),
        ],
    );
    assert!(out.contains("screenshot saved"), "{out}");
    assert!(colours(&shot) > 20, "the window looks blank");
}

/// The first-run window draws on the real binary when asked (M9 R7b); a
/// screenshot alone never opens it.
#[test]
#[ignore = "needs a desktop and a GPU: run at the end of every slice"]
fn the_first_run_window_draws() {
    let dir = TempDir::new("smoke-first-run");
    let shot = dir.path().join("first-run.png");
    let out = lumen(
        dir.path(),
        &[
            "--first-run",
            "--screenshot",
            shot.to_str().expect("a UTF-8 path"),
        ],
    );
    assert!(out.contains("screenshot saved"), "{out}");
    assert!(colours(&shot) > 20, "the window looks blank");
}

/// A panic leaves a crash report naming what failed and where, and the
/// next start offers it (owner, M3 slice 6c).
#[test]
#[ignore = "runs the real binary; needs a desktop and a GPU"]
fn a_panic_leaves_a_crash_report() {
    let report = crash_report("--crash-test");
    assert!(
        report.contains("What:     --crash-test: a deliberate panic"),
        "{report}"
    );
    assert!(report.contains("Where:    crates"), "{report}");
    assert!(report.contains("Backtrace:"), "{report}");
}

/// A failed allocation is not a panic, yet it leaves a report too, with
/// the backtrace that says where (owner: the deep-zoom crash left none).
#[test]
#[ignore = "runs the real binary; needs a desktop and a GPU"]
fn a_failed_allocation_leaves_a_crash_report() {
    let report = crash_report("--crash-test-memory");
    assert!(
        report.contains(&format!(
            "What:     memory allocation of {} bytes failed",
            isize::MAX
        )),
        "{report}"
    );
    assert!(
        report.contains("lumen_app::system::crash::fail"),
        "{report}"
    );
}

/// Runs `lumen` with a deliberate failure (`flag`) and returns its report.
fn crash_report(flag: &str) -> String {
    let dir = TempDir::new("smoke-crash");
    let crashes = dir.path().join("crashes");
    let mut child = Command::new(env!("CARGO_BIN_EXE_lumen"))
        .args(["--audio", "test:fake", flag])
        .arg("--settings")
        .arg(dir.path().join("settings.json"))
        .arg("--autosave-dir")
        .arg(dir.path().join("autosave"))
        .arg("--crash-dir")
        .arg(&crashes)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start lumen");
    let started = Instant::now();
    while child.try_wait().expect("poll deliberate crash").is_none() {
        if started.elapsed() > LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            panic!("lumen {flag} did not fail within {LIMIT:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let status = child.wait().expect("wait for deliberate crash");
    assert!(!status.success(), "{flag} must end in a crash");
    let reports: Vec<PathBuf> = std::fs::read_dir(&crashes)
        .expect("a crash folder")
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    assert_eq!(reports.len(), 1, "{reports:?}");
    std::fs::read_to_string(&reports[0]).unwrap()
}

// ── One Lumen per settings file (M9 R8, ADR-0117) ────────────────────────

/// Starts `lumen` isolated in `dir` (the same settings file for each run in
/// one `dir`), without waiting.
fn spawn(dir: &Path, args: &[&str]) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_lumen"))
        .args(["--audio", "test:fake"])
        .arg("--settings")
        .arg(dir.join("settings.json"))
        .arg("--autosave-dir")
        .arg(dir.join("autosave"))
        .arg("--crash-dir")
        .arg(dir.join("crashes"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start lumen")
}

/// Waits for `child` (at most `limit`, then kills it) and returns whether it
/// exited 0 by itself, with its stdout and stderr.
fn finish(mut child: std::process::Child, limit: Duration) -> (bool, String, String) {
    let started = Instant::now();
    let mut killed = false;
    while child.try_wait().expect("poll lumen").is_none() {
        if started.elapsed() > limit {
            let _ = child.kill();
            killed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let Output {
        status,
        stdout,
        stderr,
    } = child.wait_with_output().expect("lumen output");
    (
        status.success() && !killed,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    )
}

fn song(dir: &Path, name: &str) -> PathBuf {
    let project = lumen_project::testing::project_of(name, Vec::new());
    let path = dir.join(format!("{name}.lumen"));
    lumen_project::save(&project, &path, &Default::default()).expect("save");
    path
}

#[test]
#[ignore = "needs a desktop and a GPU: run at the end of every slice"]
fn a_second_start_hands_its_project_to_the_first() {
    let dir = TempDir::new("smoke-instance-hand");
    let first = spawn(dir.path(), &["--quit-after-ms", "12000"]);
    let night = song(dir.path(), "Night Drive");
    let second = spawn(dir.path(), &[night.to_str().unwrap()]);
    let (handed, out, err) = finish(second, LIMIT);
    assert!(
        handed,
        "the second start exits 0 once handed over:\n{out}\n{err}"
    );
    let (_, out, err) = finish(first, LIMIT);
    assert!(
        out.contains("instance: closing with Night Drive"),
        "{out}\n{err}"
    );
}

#[test]
#[ignore = "needs a desktop and a GPU: run at the end of every slice"]
fn two_starts_at_once_make_one_window() {
    let dir = TempDir::new("smoke-instance-race");
    let a = spawn(dir.path(), &["--quit-after-ms", "8000"]);
    let b = spawn(dir.path(), &["--quit-after-ms", "8000"]);
    let (a_ok, a_out, a_err) = finish(a, LIMIT);
    let (b_ok, b_out, b_err) = finish(b, LIMIT);
    assert!(a_ok && b_ok, "{a_out}\n{a_err}\n{b_out}\n{b_err}");
    let windows = [&a_out, &b_out]
        .iter()
        .filter(|o| o.contains("instance: closing with"))
        .count();
    assert_eq!(windows, 1, "exactly one ran its window:\n{a_out}\n{b_out}");
}

#[test]
#[ignore = "needs a desktop and a GPU: run at the end of every slice"]
fn a_holder_that_does_not_answer_is_not_replaced() {
    let dir = TempDir::new("smoke-instance-deaf");
    let first = spawn(dir.path(), &["--instance-deaf", "--quit-after-ms", "40000"]);
    std::thread::sleep(Duration::from_secs(3));
    let second = spawn(dir.path(), &["--smoke-test"]);
    let (exited, out, err) = finish(second, Duration::from_secs(25));
    assert!(
        !exited,
        "the second start waits in its own window:\n{out}\n{err}"
    );
    assert!(
        err.contains("is already running but is not answering"),
        "{err}"
    );
    assert!(
        !out.contains("smoke test:"),
        "it started no window of its own: {out}"
    );
    let mut first = first;
    let _ = first.kill();
    let _ = first.wait();
}

#[test]
#[ignore = "needs a desktop and a GPU: run at the end of every slice"]
fn a_refusal_shows_its_reason_and_does_not_exit_0() {
    let dir = TempDir::new("smoke-instance-busy");
    let first = spawn(dir.path(), &["--instance-busy", "--quit-after-ms", "30000"]);
    std::thread::sleep(Duration::from_secs(3));
    let second = spawn(dir.path(), &[]);
    let (exited, _, err) = finish(second, Duration::from_secs(8));
    assert!(!exited, "{err}");
    assert!(
        err.contains("did not open the file") && err.contains("busy"),
        "{err}"
    );
    let mut first = first;
    let _ = first.kill();
    let _ = first.wait();
}

#[test]
#[ignore = "needs a desktop and a GPU: run at the end of every slice"]
fn a_holder_that_closes_during_the_retry_lets_the_next_start_run() {
    let dir = TempDir::new("smoke-instance-closes");
    let first = spawn(dir.path(), &["--instance-deaf", "--quit-after-ms", "4000"]);
    std::thread::sleep(Duration::from_secs(2));
    let night = song(dir.path(), "Night Drive");
    let second = spawn(
        dir.path(),
        &[night.to_str().unwrap(), "--quit-after-ms", "6000"],
    );
    let (ok, out, err) = finish(second, LIMIT);
    assert!(ok, "{out}\n{err}");
    assert!(
        out.contains("instance: closing with Night Drive"),
        "it ran as the instance: {out}"
    );
    let _ = finish(first, LIMIT);
}

#[test]
#[ignore = "needs a desktop and a GPU: run at the end of every slice"]
fn two_settings_files_give_two_windows() {
    let a = TempDir::new("smoke-instance-two-a");
    let b = TempDir::new("smoke-instance-two-b");
    let first = spawn(a.path(), &["--smoke-test"]);
    let second = spawn(b.path(), &["--smoke-test"]);
    for child in [first, second] {
        let (ok, out, err) = finish(child, LIMIT);
        assert!(ok && out.contains("smoke test:"), "{out}\n{err}");
    }
}
