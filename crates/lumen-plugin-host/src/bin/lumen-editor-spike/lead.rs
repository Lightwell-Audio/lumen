//! The stand-in for Lumen: a window that pumps messages on its main thread
//! (as Lumen's UI thread does), a worker thread that runs the scenarios
//! against an editor host process, and the numbers that matter: how long the
//! stand-in's own message loop ever stalls while a plugin's editor is open,
//! hung, closed or killed.

use std::path::PathBuf;
use std::process::ExitCode;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::{Once, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, VK_SPACE, keybd_event};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, IsHungAppWindow, KillTimer, MSG,
    PostMessageW, PostQuitMessage, SW_MINIMIZE, SW_RESTORE, SWP_NOSIZE, SWP_NOZORDER, SetTimer,
    SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, WM_APP, WM_DESTROY, WM_KEYDOWN,
    WM_TIMER, WS_CAPTION, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
};

use crate::link::{Host, field, pair};
use crate::win::{per_monitor_dpi, register, wide, window_rect};

const CLASS: &str = "LumenEditorSpikeLead";
const TICK_MS: u32 = 10;
const WM_MIN: u32 = WM_APP + 1;
const WM_MOVE_AWAY: u32 = WM_APP + 2;
const WM_DESTROY_OWNER: u32 = WM_APP + 3;
const WM_DONE: u32 = WM_APP + 4;
const WM_MAKE_OWNER: u32 = WM_APP + 5;
const PENDING: u64 = u64::MAX;
/// What would make Lumen visibly freeze: a message loop that stops this long.
const FREEZE_US: u64 = 150_000;

static START: OnceLock<Instant> = OnceLock::new();
static MAIN: AtomicIsize = AtomicIsize::new(0);
static OWNER2: AtomicIsize = AtomicIsize::new(0);
static LAST_TICK_US: AtomicU64 = AtomicU64::new(0);
static MAX_GAP_US: AtomicU64 = AtomicU64::new(0);
static SPACES: AtomicU32 = AtomicU32::new(0);
static MIN_US: AtomicU64 = AtomicU64::new(PENDING);
static MOVE_US: AtomicU64 = AtomicU64::new(PENDING);
static DESTROY_US: AtomicU64 = AtomicU64::new(PENDING);
static EXIT: AtomicU32 = AtomicU32::new(0);
static CLOSED: AtomicU32 = AtomicU32::new(0);

fn now_us() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_micros() as u64
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    None,
    Owned,
    Top,
}

struct Opts {
    mode: Mode,
    plugin: PathBuf,
    class: String,
    manual: bool,
    skip_destroy: bool,
    synthetic_keys: bool,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut mode = Mode::None;
    let mut plugin = None;
    let mut class = "first".to_string();
    let (mut manual, mut skip_destroy, mut synthetic_keys) = (false, false, false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--owner" => {
                mode = match value("--owner")?.as_str() {
                    "none" => Mode::None,
                    "owned" => Mode::Owned,
                    "top" => Mode::Top,
                    other => return Err(format!("--owner is none, owned or top, not {other}")),
                }
            }
            "--plugin" => plugin = Some(PathBuf::from(value("--plugin")?)),
            "--class" => class = value("--class")?,
            "--manual" => manual = true,
            "--skip-destroy" => skip_destroy = true,
            "--synthetic-keys" => synthetic_keys = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let plugin = match plugin {
        Some(p) => p,
        None => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let beside = exe.with_file_name("lumen_test_plugin.dll");
            if !beside.is_file() {
                return Err("no test plugin beside this program: run `cargo build --release -p lumen-test-plugin`, or pass --plugin".into());
            }
            beside
        }
    };
    Ok(Opts {
        mode,
        plugin,
        class,
        manual,
        skip_destroy,
        synthetic_keys,
    })
}

// ------------------------------------------------------ the stand-in window --

unsafe extern "system" fn lead_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER => {
            let now = now_us();
            let last = LAST_TICK_US.swap(now, Ordering::Relaxed);
            if last != 0 {
                MAX_GAP_US.fetch_max(now - last, Ordering::Relaxed);
            }
            return 0;
        }
        WM_KEYDOWN if wparam as u32 == u32::from(VK_SPACE) => {
            // The stand-in's shortcut: Space, as play/stop is in Lumen.
            let n = SPACES.fetch_add(1, Ordering::Relaxed) + 1;
            let title = wide(&format!("Lumen stand-in - Space pressed {n} times"));
            // SAFETY: our window, on its own thread.
            unsafe { SetWindowTextW(hwnd, title.as_ptr()) };
            return 0;
        }
        WM_MIN => {
            let began = Instant::now();
            // SAFETY: our window, on its own thread.
            unsafe {
                ShowWindow(hwnd, SW_MINIMIZE);
                ShowWindow(hwnd, SW_RESTORE);
            }
            MIN_US.store(began.elapsed().as_micros() as u64, Ordering::Relaxed);
            return 0;
        }
        WM_MOVE_AWAY => {
            let began = Instant::now();
            let r = window_rect(hwnd);
            // SAFETY: our window, on its own thread.
            unsafe {
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    r.left + 40,
                    r.top + 40,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER,
                );
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    r.left,
                    r.top,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER,
                );
            }
            MOVE_US.store(began.elapsed().as_micros() as u64, Ordering::Relaxed);
            return 0;
        }
        WM_MAKE_OWNER => {
            let class = wide(CLASS);
            let title = wide("Second owner (stand-in)");
            // SAFETY: a window on this thread, which pumps its messages.
            let second = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    title.as_ptr(),
                    WS_POPUP | WS_CAPTION | WS_VISIBLE,
                    120,
                    120,
                    260,
                    90,
                    null_mut(),
                    null_mut(),
                    GetModuleHandleW(null()),
                    null(),
                )
            };
            OWNER2.store(second as isize, Ordering::Relaxed);
            return 0;
        }
        WM_DESTROY_OWNER => {
            let began = Instant::now();
            // SAFETY: the second window made above, on its own thread.
            unsafe { DestroyWindow(wparam as HWND) };
            DESTROY_US.store(began.elapsed().as_micros() as u64, Ordering::Relaxed);
            return 0;
        }
        WM_DONE => {
            EXIT.store(wparam as u32, Ordering::Relaxed);
            // SAFETY: our window.
            unsafe {
                KillTimer(hwnd, 1);
                DestroyWindow(hwnd);
            }
            return 0;
        }
        WM_DESTROY => {
            if hwnd as isize == MAIN.load(Ordering::Relaxed) {
                CLOSED.store(1, Ordering::Relaxed);
                // SAFETY: ends this thread's loop.
                unsafe { PostQuitMessage(0) };
            }
            return 0;
        }
        _ => {}
    }
    // SAFETY: default handling.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub fn run(args: &[String]) -> ExitCode {
    let opts = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("lumen-editor-spike run: {e}");
            return ExitCode::from(2);
        }
    };
    per_monitor_dpi();
    static ONCE: Once = Once::new();
    register(CLASS, lead_proc, &ONCE);
    let class = wide(CLASS);
    let title = wide("Lumen stand-in - Space pressed 0 times");
    // SAFETY: the stand-in's main window, pumped by the loop below.
    let main = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            560,
            180,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    if main.is_null() {
        eprintln!("lumen-editor-spike run: could not make the stand-in window");
        return ExitCode::from(1);
    }
    MAIN.store(main as isize, Ordering::Relaxed);
    // SAFETY: a timer on our window; the gap between its ticks is the stall.
    unsafe { SetTimer(main, 1, TICK_MS, None) };

    let worker = std::thread::spawn(move || {
        let code = if opts.manual {
            manual(&opts)
        } else {
            scenarios(&opts)
        };
        // SAFETY: posting to the main window.
        unsafe {
            PostMessageW(
                MAIN.load(Ordering::Relaxed) as HWND,
                WM_DONE,
                code as usize,
                0,
            )
        };
    });

    let mut msg = MSG {
        hwnd: null_mut(),
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: POINT { x: 0, y: 0 },
    };
    // SAFETY: the stand-in's message loop: the thing that must never freeze.
    unsafe {
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    let _ = worker.join();
    ExitCode::from(EXIT.load(Ordering::Relaxed) as u8)
}

// ------------------------------------------------------------ scenarios --

struct Checks {
    failed: u32,
}

impl Checks {
    fn add(&mut self, ok: bool, name: &str, detail: impl AsRef<str>) {
        if !ok {
            self.failed += 1;
        }
        println!(
            "{} {name}: {}",
            if ok { "PASS" } else { "FAIL" },
            detail.as_ref()
        );
    }
}

fn info(name: &str, detail: impl AsRef<str>) {
    println!("INFO {name}: {}", detail.as_ref());
}

const CALL: Duration = Duration::from_secs(8);
/// How long a window operation may take before Lumen is called blocked.
const BLOCKED: Duration = Duration::from_secs(8);

/// An `ok ...` answer's text, or why not.
fn call(host: &mut Host, command: &str) -> Result<String, String> {
    match host.send(command, CALL) {
        Ok(a) => match a.strip_prefix("ok ") {
            Some(rest) => Ok(rest.to_string()),
            None => Err(a),
        },
        Err(e) => Err(e),
    }
}

fn start(opts: &Opts, faults: &str) -> Result<Host, String> {
    let owner = (opts.mode == Mode::Owned).then(|| MAIN.load(Ordering::Relaxed));
    Host::spawn(
        &opts.plugin,
        &opts.class,
        owner,
        opts.mode == Mode::Top,
        faults,
    )
}

fn ms(d: Duration) -> String {
    format!("{:.0} ms", d.as_secs_f64() * 1e3)
}

fn main_hwnd() -> HWND {
    MAIN.load(Ordering::Relaxed) as HWND
}

/// Asks the stand-in's window thread to run `message`, and waits for its
/// answer in `slot` (microseconds), if it comes within `limit`.
fn on_main(message: u32, wparam: usize, slot: &AtomicU64, limit: Duration) -> Option<u64> {
    slot.store(PENDING, Ordering::Relaxed);
    // SAFETY: posting to the main window.
    unsafe { PostMessageW(main_hwnd(), message, wparam, 0) };
    let began = Instant::now();
    while began.elapsed() < limit {
        let v = slot.load(Ordering::Relaxed);
        if v != PENDING {
            return Some(v);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

fn scenarios(opts: &Opts) -> u32 {
    println!(
        "== editor spike: window relationship `{:?}`, plugin {} ==",
        opts.mode,
        opts.plugin.display()
    );
    let mut checks = Checks { failed: 0 };
    if let Err(e) = (|| -> Result<(), String> {
        let mut host = start(opts, "editor")?;
        open_and_close(opts, &mut host, &mut checks)?;
        resizing(&mut host, &mut checks)?;
        bounds(&mut host, &mut checks)?;
        scaling(&mut host, &mut checks)?;
        if opts.synthetic_keys {
            typing(&mut host, &mut checks)?;
        } else {
            info(
                "typing and shortcuts",
                "not run here (use --manual, or --synthetic-keys without touching the keyboard)",
            );
        }
        Ok(())
    })() {
        checks.add(false, "scenario setup", e);
    }
    if let Err(e) = hang(opts, &mut checks) {
        checks.add(false, "hung editor", e);
    }
    if !opts.skip_destroy
        && let Err(e) = destroy_owner(opts, &mut checks)
    {
        checks.add(false, "owner destroyed while its editor hangs", e);
    }
    let gap = MAX_GAP_US.load(Ordering::Relaxed);
    info(
        "stand-in message loop, whole run",
        format!("longest gap between 10 ms ticks {:.0} ms", gap as f64 / 1e3),
    );
    println!("== {:?}: {} check(s) failed ==", opts.mode, checks.failed);
    u32::from(checks.failed > 0)
}

fn open_and_close(opts: &Opts, host: &mut Host, checks: &mut Checks) -> Result<(), String> {
    let mut times = Vec::new();
    let mut first = String::new();
    for i in 0..3 {
        let began = Instant::now();
        let answer = call(host, "open")?;
        times.push(began.elapsed());
        if i == 0 {
            first = answer;
        }
        call(host, "close")?;
    }
    let answer = call(host, "open")?;
    let slowest = times.iter().max().copied().unwrap_or_default();
    checks.add(
        true,
        "open, close, reopen (x3)",
        format!("slowest open {}", ms(slowest)),
    );
    let route = field(&first, "route").unwrap_or("?");
    let state = call(host, "state")?;
    let owner = field(&state, "owner")
        .and_then(|o| o.parse::<isize>().ok())
        .unwrap_or(-1);
    let top = field(&state, "topmost") == Some("true");
    let (ok, what) = match opts.mode {
        Mode::None => (owner == 0 && !top, "unowned, not on top"),
        Mode::Owned => (
            owner == MAIN.load(Ordering::Relaxed) && route != "owner-failed",
            "owned by the stand-in's window",
        ),
        Mode::Top => (owner == 0 && top, "unowned and on top"),
    };
    checks.add(
        ok,
        "window relationship",
        format!("{what}; route {route}, owner {owner}, topmost {top}"),
    );
    info("first open", answer);
    Ok(())
}

/// The window's client size, the plugin's size and the content scale.
type Sizes = ((i32, i32), (i32, i32), f64);

fn sizes(answer: &str) -> Option<Sizes> {
    let client = pair(field(answer, "client")?)?;
    let plugin = pair(field(answer, "plugin")?)?;
    let dpi: f64 = field(answer, "dpi")?.parse().ok()?;
    Some((client, plugin, dpi / 96.0))
}

fn resizing(host: &mut Host, checks: &mut Checks) -> Result<(), String> {
    let bounds = call(host, "bounds")?;
    let (_, _, scale) = sizes(&bounds).ok_or("unreadable bounds")?;
    let near = |a: i32, b: f64| (f64::from(a) - b).abs() <= 1.5;

    let a = call(host, "resize 480 360")?;
    let (client, plugin, _) = sizes(&a).ok_or("unreadable size")?;
    checks.add(
        client == (480, 360) && plugin == client,
        "user resize reaches the plugin",
        format!(
            "asked 480x360, window {}x{}, plugin {}x{}",
            client.0, client.1, plugin.0, plugin.1
        ),
    );

    let a = call(host, "resize 5000 4000")?;
    let (client, plugin, _) = sizes(&a).ok_or("unreadable size")?;
    checks.add(
        near(client.0, 1600.0 * scale) && near(client.1, 1200.0 * scale) && plugin == client,
        "the plugin's maximum holds",
        format!(
            "asked 5000x4000, got {}x{} (limit {:.0}x{:.0})",
            client.0,
            client.1,
            1600.0 * scale,
            1200.0 * scale
        ),
    );

    let a = call(host, "resize 10 10")?;
    let (client, plugin, _) = sizes(&a).ok_or("unreadable size")?;
    checks.add(
        near(client.0, 240.0 * scale) && near(client.1, 160.0 * scale) && plugin == client,
        "the plugin's minimum holds",
        format!(
            "asked 10x10, got {}x{} (limit {:.0}x{:.0})",
            client.0,
            client.1,
            240.0 * scale,
            160.0 * scale
        ),
    );

    call(host, "resize 400 300")?;
    let a = call(host, "grow")?;
    let (client, plugin, _) = sizes(&a).ok_or("unreadable size")?;
    let want = (400.0 + 40.0 * scale, 300.0 + 30.0 * scale);
    checks.add(
        near(client.0, want.0) && near(client.1, want.1) && plugin == client,
        "the plugin asks for a bigger view and gets it",
        format!(
            "400x300 grew to {}x{} (expected about {:.0}x{:.0})",
            client.0, client.1, want.0, want.1
        ),
    );
    Ok(())
}

fn bounds(host: &mut Host, checks: &mut Checks) -> Result<(), String> {
    let before = call(host, "bounds")?;
    let outer = field(&before, "outer").ok_or("no outer")?.to_string();
    let client = pair(field(&before, "client").ok_or("no client")?).ok_or("bad client")?;
    let mut o = outer.split(',').map(|n| n.parse::<i32>().unwrap_or(0));
    let (left, top) = (o.next().unwrap_or(0), o.next().unwrap_or(0));
    call(host, "close")?;
    call(
        host,
        &format!("open {left} {top} {} {}", client.0, client.1),
    )?;
    let after = call(host, "bounds")?;
    let same_outer = field(&after, "outer") == Some(outer.as_str());
    let same_client = pair(field(&after, "client").unwrap_or("")) == Some(client);
    checks.add(
        same_outer && same_client,
        "remembered bounds come back",
        format!(
            "saved {outer} {}x{}, reopened {} {}",
            client.0,
            client.1,
            field(&after, "outer").unwrap_or("?"),
            field(&after, "client").unwrap_or("?")
        ),
    );
    Ok(())
}

fn scaling(host: &mut Host, checks: &mut Checks) -> Result<(), String> {
    let now = call(host, "bounds")?;
    let (_, before, s0) = sizes(&now).ok_or("unreadable size")?;
    let factor = s0 * 1.5;
    let a = call(host, &format!("scale {factor}"))?;
    let (client, plugin, _) = sizes(&a).ok_or("unreadable size")?;
    let want = (
        (f64::from(before.0) * 1.5).round() as i32,
        (f64::from(before.1) * 1.5).round() as i32,
    );
    checks.add(
        (plugin.0 - want.0).abs() <= 1 && (plugin.1 - want.1).abs() <= 1 && client == plugin,
        "a content scale change resizes the plugin and the window",
        format!("{}x{} at scale {s0:.2} became {}x{} at {factor:.2} (expected about {}x{}), window {}x{}", before.0, before.1, plugin.0, plugin.1, want.0, want.1, client.0, client.1),
    );
    call(host, &format!("scale {s0}"))?;
    info(
        "monitors",
        "moving the window between monitors of different scale is manual: see MT-VST-0, editors",
    );
    Ok(())
}

fn press(vk: u8) {
    // SAFETY: a key press and release into the foreground window, which the
    // caller has just checked is the editor's.
    unsafe {
        keybd_event(vk, 0, 0, 0);
        keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
    }
}

fn typing(host: &mut Host, checks: &mut Checks) -> Result<(), String> {
    call(host, "focus")?;
    std::thread::sleep(Duration::from_millis(300));
    let state = call(host, "state")?;
    if field(&state, "foreground") != Some("true") || field(&state, "focus") != Some("Edit") {
        checks.add(
            false,
            "typing into the plugin",
            format!("the editor did not get the keyboard focus, so no keys were sent ({state})"),
        );
        return Ok(());
    }
    let spaces = SPACES.load(Ordering::Relaxed);
    for vk in [0x48_u8, 0x49, VK_SPACE as u8] {
        press(vk);
    }
    std::thread::sleep(Duration::from_millis(300));
    let text = call(host, "text")?;
    let typed = field(&text, "text") == Some("hi_");
    let leaked = SPACES.load(Ordering::Relaxed) != spaces;
    checks.add(
        typed && !leaked,
        "typing reaches the plugin and fires no Lumen shortcut",
        format!(
            "field holds `{}`, the stand-in's Space shortcut fired {} time(s)",
            field(&text, "text").unwrap_or("?"),
            SPACES.load(Ordering::Relaxed) - spaces
        ),
    );
    Ok(())
}

/// A plugin whose editor stops answering: is Lumen frozen, how soon is it
/// known, and does the user have a way out.
fn hang(opts: &Opts, checks: &mut Checks) -> Result<(), String> {
    let mut host = start(opts, "editor_hang=1500")?;
    call(&mut host, "open")?;
    let hung_at = Instant::now() + Duration::from_millis(1500);
    // Only this incident is measured from here.
    MAX_GAP_US.store(0, Ordering::Relaxed);
    let frame = call(&mut host, "bounds")
        .ok()
        .and_then(|a| field(&a, "hwnd").and_then(|h| h.parse::<isize>().ok()))
        .unwrap_or(0) as HWND;

    let (mut cooperative, mut by_system) = (None, None);
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(12)
        && (cooperative.is_none() || by_system.is_none())
    {
        if cooperative.is_none()
            && Instant::now() > hung_at
            && host
                .send("ping", Duration::from_millis(1500))
                .is_ok_and(|a| a == "unresponsive")
        {
            cooperative = Some(Instant::now());
        }
        // SAFETY: a window handle of another process, read-only.
        if by_system.is_none() && !frame.is_null() && unsafe { IsHungAppWindow(frame) } != 0 {
            by_system = Some(Instant::now());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let since = |t: Option<Instant>| match t {
        Some(t) => ms(t.saturating_duration_since(hung_at)),
        None => "never".into(),
    };
    checks.add(
        cooperative.is_some(),
        "a hung editor is noticed by the host's own heartbeat",
        format!("{} after it stopped answering", since(cooperative)),
    );
    info(
        "a hung editor as the system sees it",
        format!(
            "IsHungAppWindow turned true {} after it stopped answering",
            since(by_system)
        ),
    );

    let frozen = MAX_GAP_US.load(Ordering::Relaxed);
    checks.add(
        frozen < FREEZE_US,
        "Lumen's message loop kept running while the editor hung",
        format!(
            "longest gap between ticks {:.0} ms (limit {:.0} ms)",
            frozen as f64 / 1e3,
            FREEZE_US as f64 / 1e3
        ),
    );

    match on_main(WM_MOVE_AWAY, 0, &MOVE_US, BLOCKED) {
        Some(us) => checks.add(
            us < 500_000,
            "moving Lumen's window with the editor hung",
            format!("took {:.0} ms", us as f64 / 1e3),
        ),
        None => checks.add(
            false,
            "moving Lumen's window with the editor hung",
            "Lumen's window was blocked for more than 8 s (it ended when the host was killed)",
        ),
    }

    match on_main(WM_MIN, 0, &MIN_US, BLOCKED) {
        Some(us) => checks.add(
            us < 500_000,
            "minimizing and restoring Lumen's window with the editor hung",
            format!("took {:.0} ms", us as f64 / 1e3),
        ),
        None => checks.add(
            false,
            "minimizing and restoring Lumen's window with the editor hung",
            "Lumen's window was blocked for more than 8 s (it ended when the host was killed)",
        ),
    }
    // The user's way out: end that plugin's host and start a healthy one.
    let began = Instant::now();
    host.kill();
    let mut fresh = start(opts, "editor")?;
    call(&mut fresh, "open")?;
    checks.add(
        true,
        "ending the hung host and reopening the editor",
        format!(
            "{} from the decision to a working editor",
            ms(began.elapsed())
        ),
    );
    let total = MAX_GAP_US.load(Ordering::Relaxed);
    info(
        "whole incident",
        format!("longest gap in Lumen's loop {:.0} ms", total as f64 / 1e3),
    );
    Ok(())
}

/// Destroying a window that owns an editor whose thread is hung: on exit, or
/// when a project closes. Only an owned editor is tied to it.
fn destroy_owner(opts: &Opts, checks: &mut Checks) -> Result<(), String> {
    if opts.mode != Mode::Owned {
        info(
            "owner destroyed while its editor hangs",
            "not applicable: the editor has no owner",
        );
        return Ok(());
    }
    OWNER2.store(0, Ordering::Relaxed);
    // SAFETY: posting to the main window.
    unsafe { PostMessageW(main_hwnd(), WM_MAKE_OWNER, 0, 0) };
    let began = Instant::now();
    while OWNER2.load(Ordering::Relaxed) == 0 && began.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let owner = OWNER2.load(Ordering::Relaxed);
    if owner == 0 {
        return Err("could not make a second owner window".into());
    }
    let mut host = Host::spawn(
        &opts.plugin,
        &opts.class,
        Some(owner),
        false,
        "editor_hang=800",
    )?;
    call(&mut host, "open")?;
    std::thread::sleep(Duration::from_millis(1800));
    match on_main(
        WM_DESTROY_OWNER,
        owner as usize,
        &DESTROY_US,
        Duration::from_secs(30),
    ) {
        Some(us) => checks.add(
            us < 2_000_000,
            "destroying an owner whose editor is hung",
            format!("took {:.0} ms", us as f64 / 1e3),
        ),
        None => checks.add(
            false,
            "destroying an owner whose editor is hung",
            "Lumen's window was blocked for more than 30 s",
        ),
    }
    Ok(())
}

// ----------------------------------------------------------- manual mode --

fn foreground_pid() -> Option<u32> {
    // SAFETY: read-only queries about the foreground window.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        (pid != 0).then_some(pid)
    }
}

fn manual(opts: &Opts) -> u32 {
    println!("== manual: window relationship `{:?}` ==", opts.mode);
    let mut host = match start(opts, "editor") {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    match call(&mut host, "open") {
        Ok(a) => println!("opened: {a}"),
        Err(e) => {
            eprintln!("could not open the editor: {e}");
            return 1;
        }
    }
    println!(
        "The editor is open. Try, in order: type in its text field; press Space (the stand-in's\n\
         title counts Space presses, so a count that rises while you typed in the plugin is a\n\
         leak); drag the editor over the stand-in and click the stand-in; minimize the stand-in;\n\
         drag the editor to a monitor of a different scale; click \"Grow view\"; resize it;\n\
         close it with its X. Close the stand-in window to finish."
    );
    let mut shown = 0;
    let mut polled = Instant::now();
    let mut raised = opts.mode == Mode::Top;
    while CLOSED.load(Ordering::Relaxed) == 0 {
        std::thread::sleep(Duration::from_millis(100));
        // Option C, refined: the editor stays above Lumen but not above other
        // programs, by being on top only while Lumen or an editor is the
        // foreground program. An asynchronous command over the pipe; Lumen's
        // window thread is never involved.
        if opts.mode == Mode::Top {
            let mine = foreground_pid().is_some_and(|p| p == std::process::id() || p == host.pid());
            if mine != raised {
                raised = mine;
                let _ = host.send(
                    if mine { "top on" } else { "top off" },
                    Duration::from_millis(1500),
                );
            }
        }
        let events = host.events();
        for (_, e) in events.iter().skip(shown) {
            println!("event: {e}");
        }
        shown = events.len();
        if polled.elapsed() > Duration::from_secs(3) {
            polled = Instant::now();
            if let Ok(s) = host.send("state", Duration::from_millis(1500)) {
                println!("state: {s}");
            }
        }
    }
    println!(
        "Space reached the stand-in {} time(s); longest gap in its message loop {:.0} ms",
        SPACES.load(Ordering::Relaxed),
        MAX_GAP_US.load(Ordering::Relaxed) as f64 / 1e3
    );
    0
}
