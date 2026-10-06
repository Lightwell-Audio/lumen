//! Step 2: the plugin runs in a separate host process. Lumen's side paces
//! blocks in real time, never waits past a deadline, survives a crash, and
//! restores the plugin in a fresh process from its last known state.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Diagnostics::Debug::{
    SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
};

use crate::ipc::{
    CMD_CRASH, CMD_GET_STATE, CMD_PROCESS, CMD_QUIT, CMD_SET_STATE, Event, HostProcess, Job,
    MAX_BLOCK, Mapping, STATE_CAPACITY, Timer, wait_done,
};
use crate::vst::{Note, Plugin};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 128;

/// The plugin-host process: load, activate, then serve commands forever.
pub fn child_main(base: &str, module: &str) -> ! {
    // A crashing plugin must end this process at once, not park it behind a dialog.
    // SAFETY: process-wide error-mode flags.
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX) };
    let mapping = Mapping::open(&format!("{base}-shm")).expect("mapping");
    let go = Event::open(&format!("{base}-go")).expect("go event");
    let done = Event::open(&format!("{base}-done")).expect("done event");
    let shared = mapping.shared();
    let mut plugin = match Plugin::load(Path::new(module), None).and_then(|mut p| {
        p.activate(RATE, MAX_BLOCK as i32)?;
        Ok(p)
    }) {
        Ok(p) => p,
        Err(_) => {
            shared.ready.0.store(2, Ordering::Release);
            std::process::exit(1);
        }
    };
    shared.ready.0.store(1, Ordering::Release);
    let (mut left, mut right) = (vec![0.0_f32; MAX_BLOCK], vec![0.0_f32; MAX_BLOCK]);
    let mut seen = 0;
    loop {
        let seq = loop {
            let seq = shared.request_seq.0.load(Ordering::Acquire);
            if seq != seen {
                break seq;
            }
            go.wait();
        };
        seen = seq;
        let status = match shared.command.0.load(Ordering::Relaxed) {
            CMD_QUIT => std::process::exit(0),
            // SAFETY: deliberately not safe: simulates a plugin crashing with
            // an access violation inside the host process.
            CMD_CRASH => {
                // SAFETY: deliberately not safe (see above).
                unsafe { std::ptr::null_mut::<u32>().write_volatile(1) };
                std::process::abort()
            }
            CMD_PROCESS => {
                let frames = (shared.frames.0.load(Ordering::Relaxed) as usize).min(MAX_BLOCK);
                let note = shared.note.0.load(Ordering::Relaxed);
                let notes: Vec<(i32, Note)> = match note {
                    0 => vec![],
                    p if p > 0 => vec![(
                        0,
                        Note::On {
                            pitch: p as i16,
                            velocity: 0.8,
                        },
                    )],
                    p => vec![(0, Note::Off { pitch: (-p) as i16 })],
                };
                left[..frames].fill(0.0);
                right[..frames].fill(0.0);
                let ok = plugin
                    .process(&notes, &mut left[..frames], &mut right[..frames])
                    .is_ok();
                let out = mapping.output(seq);
                for i in 0..frames {
                    // SAFETY: the slot holds MAX_BLOCK * 2 floats; i < frames <= MAX_BLOCK.
                    unsafe {
                        out.add(2 * i).write(left[i]);
                        out.add(2 * i + 1).write(right[i]);
                    }
                }
                if ok { 0 } else { -1 }
            }
            CMD_GET_STATE => match plugin.state() {
                Ok(bytes) if bytes.len() <= STATE_CAPACITY => {
                    // SAFETY: the state area holds STATE_CAPACITY bytes.
                    unsafe {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapping.state(), bytes.len())
                    };
                    shared
                        .state_len
                        .0
                        .store(bytes.len() as u32, Ordering::Relaxed);
                    0
                }
                _ => -1,
            },
            CMD_SET_STATE => {
                let len = (shared.state_len.0.load(Ordering::Relaxed) as usize).min(STATE_CAPACITY);
                // SAFETY: `len` bytes of the state area were written by the parent.
                let bytes = unsafe { std::slice::from_raw_parts(mapping.state_const(), len) };
                if plugin.set_state(bytes).is_ok() {
                    0
                } else {
                    -1
                }
            }
            _ => -1,
        };
        shared.status.0.store(status, Ordering::Release);
        shared.done_seq.0.store(seq, Ordering::Release);
        done.set();
    }
}

fn read_state(host: &HostProcess) -> Vec<u8> {
    let len = host.mapping.shared().state_len.0.load(Ordering::Acquire) as usize;
    // SAFETY: the host wrote `len` (<= capacity) bytes before signalling done.
    unsafe { std::slice::from_raw_parts(host.mapping.state_const(), len.min(STATE_CAPACITY)) }
        .to_vec()
}

struct Restored {
    host: HostProcess,
    respawn_ms: f64,
    state_identical: bool,
}

/// Lumen's side: real-time-paced blocks, crash injection, recovery.
pub fn run(module: &str) -> Result<(), String> {
    let job = std::sync::Arc::new(Job::new().map_err(|e| e.to_string())?);
    let started = Instant::now();
    let mut host = Some(HostProcess::spawn(module, 0, &job).map_err(|e| e.to_string())?);
    println!(
        "host process up, plugin loaded and active in {:.0} ms",
        started.elapsed().as_secs_f64() * 1e3
    );

    // Supervisor: kills dead hosts and brings up replacements with state, off the audio loop.
    let (to_supervisor, requests) = mpsc::channel::<(HostProcess, u32, Vec<u8>)>();
    let (to_audio, replacements) = mpsc::channel::<Result<Restored, String>>();
    let module_owned = module.to_string();
    let job_for_supervisor = std::sync::Arc::clone(&job);
    let supervisor = std::thread::spawn(move || {
        for (mut dead, generation, snapshot) in requests {
            dead.kill();
            drop(dead);
            let t = Instant::now();
            let result = HostProcess::spawn(&module_owned, generation, &job_for_supervisor)
                .map_err(|e| e.to_string())
                .and_then(|mut fresh| {
                    // SAFETY: the state area holds STATE_CAPACITY bytes; snapshot fits.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            snapshot.as_ptr(),
                            fresh.mapping.state(),
                            snapshot.len(),
                        )
                    };
                    fresh
                        .mapping
                        .shared()
                        .state_len
                        .0
                        .store(snapshot.len() as u32, Ordering::Release);
                    match fresh.call(CMD_SET_STATE, Duration::from_secs(5)) {
                        Ok(0) => {}
                        other => return Err(format!("setState in new host failed: {other:?}")),
                    }
                    let respawn_ms = t.elapsed().as_secs_f64() * 1e3;
                    fresh
                        .call(CMD_GET_STATE, Duration::from_secs(5))
                        .map_err(|e| e.to_string())?;
                    let state_identical = read_state(&fresh) == snapshot;
                    Ok(Restored {
                        host: fresh,
                        respawn_ms,
                        state_identical,
                    })
                });
            let _ = to_audio.send(result);
        }
    });

    let period = Duration::from_secs_f64(BLOCK as f64 / RATE);
    let deadline = period / 2;
    let total_blocks = (4.0 * RATE) as usize / BLOCK;
    let (snapshot_block, crash_block, off_block) = (375, 500, 1_300);
    let pacing = Timer::new().map_err(|e| e.to_string())?;
    let deadline_timer = Timer::new().map_err(|e| e.to_string())?;
    let mut audio = Vec::with_capacity(total_blocks * BLOCK * 2);
    let mut round_trips = Vec::new();
    let mut snapshot = Vec::new();
    let (mut misses, mut silent, mut overruns) = (0_u32, 0_usize, 0_usize);
    let (mut crash_detected, mut recovered_at) = (None, None);
    let mut send_note_on = true;
    let mut generation = 0;
    let mut next = Instant::now() + period;

    for block in 0..total_blocks {
        let now = Instant::now();
        if next > now {
            pacing.arm(next - now);
            pacing.wait();
        }
        next += period;
        let block_started = Instant::now();

        if host.is_none() {
            match replacements.try_recv() {
                Ok(Ok(restored)) => {
                    println!(
                        "supervisor: host restarted with last known state in {:.0} ms; restored state re-saves identical: {}",
                        restored.respawn_ms, restored.state_identical
                    );
                    host = Some(restored.host);
                    recovered_at = Some(block);
                    send_note_on = true; // held notes are lost with the crashed process
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {}
            }
        }
        let Some(current) = host.as_mut() else {
            silent += 1;
            audio.extend(std::iter::repeat_n(0.0, BLOCK * 2));
            continue;
        };

        if block == snapshot_block {
            // Production does this on a separate non-real-time channel; here
            // it takes one block slot, which is filled with silence.
            let t = Instant::now();
            current
                .call(CMD_GET_STATE, Duration::from_secs(1))
                .map_err(|e| e.to_string())?;
            snapshot = read_state(current);
            println!(
                "state snapshot: {} bytes in {:.2} ms (last known state for recovery)",
                snapshot.len(),
                t.elapsed().as_secs_f64() * 1e3
            );
            audio.extend(std::iter::repeat_n(0.0, BLOCK * 2));
            continue;
        }

        let (command, note) = match block {
            b if b == crash_block => (CMD_CRASH, 0),
            b if b == off_block => (CMD_PROCESS, -60),
            _ if send_note_on => {
                send_note_on = false;
                (CMD_PROCESS, 60)
            }
            _ => (CMD_PROCESS, 0),
        };
        let seq = current.submit(command, BLOCK, note);
        deadline_timer.arm(deadline);
        let done = loop {
            if current.is_done(seq) {
                break true;
            }
            if !wait_done(current, &deadline_timer) {
                break current.is_done(seq);
            }
        };
        if done && current.mapping.shared().status.0.load(Ordering::Acquire) == 0 {
            misses = 0;
            round_trips.push(block_started.elapsed().as_secs_f32() * 1e6);
            let out = current.mapping.output(seq);
            for i in 0..BLOCK * 2 {
                // SAFETY: the host finished writing this slot before signalling done.
                audio.push(unsafe { out.add(i).read() });
            }
        } else {
            misses += 1;
            silent += 1;
            audio.extend(std::iter::repeat_n(0.0, BLOCK * 2));
            if current.exited() || misses >= 3 {
                if crash_detected.is_none() {
                    crash_detected = Some((block, current.exit_code()));
                }
                generation += 1;
                let dead = host.take().expect("current host");
                to_supervisor
                    .send((dead, generation, snapshot.clone()))
                    .map_err(|e| e.to_string())?;
            }
        }
        if block_started.elapsed() > period {
            overruns += 1;
        }
    }
    drop(to_supervisor);
    let _ = supervisor.join();

    let level = |samples: &[f32]| {
        20.0 * samples
            .iter()
            .fold(0.0_f32, |m, s| m.max(s.abs()))
            .max(1e-9)
            .log10()
    };
    let before = &audio[..crash_block * BLOCK * 2];
    let after = recovered_at.map(|b| &audio[b * BLOCK * 2..]).unwrap_or(&[]);
    round_trips.sort_by(f32::total_cmp);
    let at = |p: f64| round_trips[((round_trips.len() - 1) as f64 * p) as usize];
    println!(
        "round trip incl. plugin processing (128 frames): p50 {:.0} / p99 {:.0} / max {:.0} µs (period 2,667)",
        at(0.5),
        at(0.99),
        at(1.0)
    );
    if let Some((block, code)) = crash_detected {
        println!(
            "crash injected at block {crash_block}; detected at block {block} (exit code {})",
            code.map(|c| format!("0x{:08X}", c as u32))
                .unwrap_or("?".into())
        );
    }
    println!(
        "audio before crash: peak {:.1} dBFS; after recovery: peak {:.1} dBFS; silent blocks {silent}; loop overruns {overruns}; recovered at block {:?}",
        level(before),
        level(after),
        recovered_at
    );
    let out = Path::new("prototypes/plugin-host/out");
    let _ = std::fs::create_dir_all(out);
    let wav = out.join("isolated-with-crash.wav");
    if crate::write_wav(&wav, &audio).is_ok() {
        println!("wrote {}", wav.display());
    }
    if level(before) < -60.0 || level(after) < -60.0 {
        return Err("no audio before the crash or after recovery".into());
    }
    Ok(())
}
