//! The editor host: a process that loads one plugin and shows its editor in a
//! top-level window of its own, on its main thread, and takes commands on
//! stdin from the stand-in for Lumen (`lead.rs`).
//!
//! Three threads, as the production host will have: this main thread makes
//! and pumps the editor window; a reader thread takes commands and waits for
//! the window thread's answer with a deadline (so a hung editor is reported
//! and never waited on forever); the plugin's audio thread is not started
//! here.
//!
//! Each command line is `<id> <command> [arguments]`; each gets exactly one
//! answer line `<id> ok ...`, `<id> err ...` or `<id> unresponsive`. Lines
//! beginning `event` are not answers.

use std::cell::RefCell;
use std::ffi::c_void;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Once, mpsc};
use std::time::Duration;

use lumen_plugin_host::module::AUDIO_CLASS;
use lumen_plugin_host::{Instance, Module};
use vst3::Steinberg::{
    IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewContentScaleSupport,
    IPlugViewContentScaleSupportTrait, IPlugViewTrait, ViewRect, kPlatformTypeHWND, kResultOk,
    kResultTrue, tresult,
};
use vst3::{Class, ComPtr, ComWrapper};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BM_CLICK, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
    DestroyWindow, DispatchMessageW, GW_OWNER, GWL_EXSTYLE, GWLP_HWNDPARENT, GetForegroundWindow,
    GetMessageW, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HWND_MESSAGE,
    HWND_NOTOPMOST, HWND_TOPMOST, IsHungAppWindow, IsWindowVisible, MINMAXINFO, MSG, PostMessageW,
    PostQuitMessage, SIZE_MINIMIZED, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, WM_APP, WM_CLOSE, WM_DPICHANGED, WM_GETMINMAXINFO, WM_KILLFOCUS, WM_SETFOCUS,
    WM_SIZE, WS_CAPTION, WS_CLIPCHILDREN, WS_EX_TOPMOST, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
    WS_THICKFRAME,
};

use crate::win::{
    class_name, client_size, find_descendant, per_monitor_dpi, register, wide, window_rect,
};

const FRAME_CLASS: &str = "LumenEditorSpikeFrame";
const DISPATCH_CLASS: &str = "LumenEditorSpikeDispatch";
const STYLE: u32 =
    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_THICKFRAME | WS_CLIPCHILDREN;
/// How long the reader thread waits for the window thread before saying so.
const WAIT: Duration = Duration::from_millis(600);

struct Args {
    plugin: PathBuf,
    class: String,
    owner: Option<isize>,
    topmost: bool,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut plugin = None;
    let mut class = "first".to_string();
    let mut owner = None;
    let mut topmost = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--plugin" => plugin = Some(PathBuf::from(value("--plugin")?)),
            "--class" => class = value("--class")?,
            "--owner" => {
                owner = Some(
                    value("--owner")?
                        .parse::<isize>()
                        .map_err(|_| "--owner is a window handle number".to_string())?,
                );
            }
            "--topmost" => topmost = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        plugin: plugin.ok_or("--plugin is required")?,
        class,
        owner,
        topmost,
    })
}

// ---------------------------------------------------------------- state --

struct Editor {
    view: ComPtr<IPlugView>,
    scale: Option<ComPtr<IPlugViewContentScaleSupport>>,
    /// Kept alive for as long as the view may call it.
    _frame: ComWrapper<FrameObj>,
    hwnd: isize,
    route: &'static str,
    attached: bool,
}

struct State {
    // Declared first, so it ends before the module it came from.
    instance: Instance,
    _module: Module,
    owner: Option<isize>,
    topmost: bool,
    editor: Option<Editor>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.borrow_mut().as_mut().map(f))
}

/// The pieces a window procedure needs, copied out so no borrow is held
/// while the plugin runs (it calls back into the frame).
#[derive(Clone)]
struct Live {
    view: ComPtr<IPlugView>,
    scale: Option<ComPtr<IPlugViewContentScaleSupport>>,
    hwnd: HWND,
    attached: bool,
    topmost: bool,
}

fn live() -> Option<Live> {
    with_state(|s| {
        let topmost = s.topmost;
        s.editor.as_ref().map(|e| Live {
            view: e.view.clone(),
            scale: e.scale.clone(),
            hwnd: e.hwnd as HWND,
            attached: e.attached,
            topmost,
        })
    })
    .flatten()
}

fn event(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "event {text}");
    let _ = out.flush();
}

// ---------------------------------------------------------- frame object --

struct FrameObj;

impl Class for FrameObj {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for FrameObj {
    /// The plugin asks for a different view size. The window follows, and its
    /// `WM_SIZE` tells the view.
    unsafe fn resizeView(&self, _view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() {
            return vst3::Steinberg::kInvalidArgument;
        }
        // SAFETY: the plugin passes a valid rectangle.
        let r = unsafe { &*new_size };
        set_client_size(r.right - r.left, r.bottom - r.top);
        event(&format!(
            "plugin-resize {}x{}",
            r.right - r.left,
            r.bottom - r.top
        ));
        kResultTrue
    }
}

fn outer_for(client: (i32, i32), dpi: u32, topmost: bool) -> (i32, i32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: client.0,
        bottom: client.1,
    };
    let ex = if topmost { WS_EX_TOPMOST } else { 0 };
    // SAFETY: a valid rectangle to adjust.
    unsafe { AdjustWindowRectExForDpi(&mut r, STYLE, 0, ex, dpi) };
    (r.right - r.left, r.bottom - r.top)
}

fn set_client_size(w: i32, h: i32) {
    let Some(live) = live() else {
        return;
    };
    // SAFETY: a window this thread made.
    let dpi = unsafe { GetDpiForWindow(live.hwnd) };
    let (ow, oh) = outer_for((w, h), dpi, live.topmost);
    // SAFETY: as above.
    unsafe {
        SetWindowPos(
            live.hwnd,
            null_mut(),
            0,
            0,
            ow,
            oh,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

fn plugin_size(view: &ComPtr<IPlugView>) -> (i32, i32) {
    let mut r = ViewRect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: a live view and a valid out pointer.
    unsafe { view.getSize(&mut r) };
    (r.right - r.left, r.bottom - r.top)
}

// ------------------------------------------------------- the frame window --

unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_GETMINMAXINFO => {
            if let Some(live) = live() {
                // SAFETY: `lparam` is the system's `MINMAXINFO`.
                let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
                let ask = |w: i32, h: i32| {
                    let mut r = ViewRect {
                        left: 0,
                        top: 0,
                        right: w,
                        bottom: h,
                    };
                    // SAFETY: a live view and a valid rectangle.
                    unsafe { live.view.checkSizeConstraint(&mut r) };
                    (r.right - r.left, r.bottom - r.top)
                };
                // SAFETY: a window this thread made.
                let dpi = unsafe { GetDpiForWindow(hwnd) };
                let min = outer_for(ask(1, 1), dpi, live.topmost);
                let max = outer_for(ask(100_000, 100_000), dpi, live.topmost);
                info.ptMinTrackSize = POINT { x: min.0, y: min.1 };
                info.ptMaxTrackSize = POINT { x: max.0, y: max.1 };
                return 0;
            }
        }
        WM_SIZE if wparam as u32 != SIZE_MINIMIZED => {
            if let Some(live) = live().filter(|l| l.attached) {
                let (w, h) = client_size(hwnd);
                let mut r = ViewRect {
                    left: 0,
                    top: 0,
                    right: w,
                    bottom: h,
                };
                // SAFETY: a live view and a valid rectangle.
                unsafe { live.view.onSize(&mut r) };
            }
        }
        WM_SETFOCUS => {
            if let Some(live) = live().filter(|l| l.attached) {
                // SAFETY: a live view.
                unsafe { live.view.onFocus(1) };
            }
        }
        WM_KILLFOCUS => {
            if let Some(live) = live().filter(|l| l.attached) {
                // SAFETY: a live view.
                unsafe { live.view.onFocus(0) };
            }
        }
        WM_DPICHANGED => {
            let dpi = (wparam & 0xFFFF) as u32;
            let scale = dpi as f32 / 96.0;
            if let Some(live) = live() {
                if let Some(support) = &live.scale {
                    // SAFETY: a live interface.
                    unsafe { support.setContentScaleFactor(scale) };
                }
                // SAFETY: `lparam` is the system's suggested rectangle.
                let r = unsafe { &*(lparam as *const RECT) };
                // SAFETY: a window this thread made.
                unsafe {
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                };
                let (w, h) = plugin_size(&live.view);
                set_client_size(w, h);
                event(&format!("dpi {dpi} scale {scale:.2} plugin {w}x{h}"));
            }
            return 0;
        }
        WM_CLOSE => {
            event("closed-by-user");
            close();
            return 0;
        }
        _ => {}
    }
    // SAFETY: default handling for a message not finished above.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn close() {
    let Some(editor) = with_state(|s| s.editor.take()).flatten() else {
        return;
    };
    // SAFETY: the view and window this thread made; the view is told it is
    // gone before its window is.
    unsafe {
        editor.view.removed();
        editor.view.setFrame(null_mut());
        DestroyWindow(editor.hwnd as HWND);
    }
}

// ---------------------------------------------------------- the commands --

struct Job {
    run: Box<dyn FnOnce() -> String + Send>,
    reply: mpsc::Sender<String>,
}

static DISPATCHER: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn dispatch_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_APP {
        // SAFETY: the pointer was made by `ask` with `Box::into_raw`.
        let job = unsafe { Box::from_raw(lparam as *mut Job) };
        let answer = (job.run)();
        let _ = job.reply.send(answer);
        return 0;
    }
    // SAFETY: default handling.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Runs `run` on the window thread and waits for its answer, up to `WAIT`.
fn ask(run: impl FnOnce() -> String + Send + 'static) -> String {
    let (tx, rx) = mpsc::channel();
    let job = Box::into_raw(Box::new(Job {
        run: Box::new(run),
        reply: tx,
    }));
    let target = DISPATCHER.load(Ordering::Relaxed) as HWND;
    // SAFETY: posting to the dispatcher window; on failure the box is ours.
    if unsafe { PostMessageW(target, WM_APP, 0, job as LPARAM) } == 0 {
        // SAFETY: not delivered, so still owned here.
        drop(unsafe { Box::from_raw(job) });
        return "err could not reach the window thread".into();
    }
    rx.recv_timeout(WAIT)
        .unwrap_or_else(|_| "unresponsive".to_string())
}

fn numbers(parts: &[&str]) -> Option<Vec<i32>> {
    parts.iter().map(|p| p.parse().ok()).collect()
}

fn open(placement: Option<(i32, i32)>, client: Option<(i32, i32)>) -> String {
    let Some((owner, topmost, view)) = with_state(|s| {
        s.editor
            .is_none()
            .then(|| (s.owner, s.topmost, s.instance.create_editor_view()))
    })
    .flatten() else {
        return "err an editor is already open".into();
    };
    let Some(view) = view else {
        return "err the plugin has no editor".into();
    };
    // SAFETY: a live view; a constant platform name.
    if unsafe { view.isPlatformTypeSupported(kPlatformTypeHWND) } != kResultTrue {
        return "err the editor does not support HWND".into();
    }
    static CLASS: Once = Once::new();
    register(FRAME_CLASS, frame_proc, &CLASS);
    let class = wide(FRAME_CLASS);
    let title = wide("Plugin editor (spike host)");
    let ex = if topmost { WS_EX_TOPMOST } else { 0 };
    let (x, y) = placement.unwrap_or((CW_USEDEFAULT, CW_USEDEFAULT));
    // SAFETY: a top-level window on this (the window) thread. With an owner
    // from another process this is the "owner at creation" route.
    let make = |parent: HWND| unsafe {
        CreateWindowExW(
            ex,
            class.as_ptr(),
            title.as_ptr(),
            STYLE,
            x,
            y,
            400,
            300,
            parent,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    let mut route = "unowned";
    let mut hwnd = null_mut();
    if let Some(owner) = owner {
        hwnd = make(owner as HWND);
        route = "owner-at-create";
        if hwnd.is_null() {
            hwnd = make(null_mut());
            if !hwnd.is_null() {
                // SAFETY: re-parenting our own window to the owner given.
                unsafe { SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner) };
                route = "owner-set-after";
            }
        }
    }
    if hwnd.is_null() {
        hwnd = make(null_mut());
        if owner.is_some() {
            route = "owner-failed";
        }
    }
    if hwnd.is_null() {
        return "err could not create the frame window".into();
    }
    // SAFETY: a window this thread made.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let scale = dpi as f32 / 96.0;
    let support = view.cast::<IPlugViewContentScaleSupport>();
    if let Some(support) = &support {
        // SAFETY: a live interface, before the view is attached.
        unsafe { support.setContentScaleFactor(scale) };
    }
    let frame = ComWrapper::new(FrameObj);
    let Some(frame_ptr) = frame.to_com_ptr::<IPlugFrame>() else {
        // SAFETY: our window.
        unsafe { DestroyWindow(hwnd) };
        return "err no frame interface".into();
    };
    // SAFETY: a live view and frame.
    unsafe { view.setFrame(frame_ptr.as_ptr()) };
    let wanted = client.unwrap_or_else(|| plugin_size(&view));
    let (ow, oh) = outer_for(wanted, dpi, topmost);
    // SAFETY: our window.
    unsafe {
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            ow,
            oh,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    // The editor is registered before it is attached: the plugin may call
    // the frame, and the window may message, while `attached` runs.
    with_state(|s| {
        s.editor = Some(Editor {
            view: view.clone(),
            scale: support,
            _frame: frame,
            hwnd: hwnd as isize,
            route,
            attached: false,
        });
    });
    // SAFETY: a live view and our window as its parent.
    let result = unsafe { view.attached(hwnd.cast::<c_void>(), kPlatformTypeHWND) };
    if result != kResultOk {
        close();
        return format!("err attached failed ({result})");
    }
    with_state(|s| {
        if let Some(e) = &mut s.editor {
            e.attached = true;
        }
    });
    // Size the plugin to what the frame really is (the frame's client area
    // was set before, so this is the first `onSize`).
    let (cw, ch) = client_size(hwnd);
    let mut r = ViewRect {
        left: 0,
        top: 0,
        right: cw,
        bottom: ch,
    };
    // SAFETY: a live, attached view.
    unsafe { view.onSize(&mut r) };
    // SAFETY: our window; the stand-in allowed this process to take focus.
    unsafe {
        ShowWindow(hwnd, SW_SHOWNORMAL);
        SetForegroundWindow(hwnd);
    }
    describe("opened")
}

/// What the open window looks like, as one answer line.
fn describe(word: &str) -> String {
    let Some(live) = live() else {
        return format!("err {word}: no editor is open");
    };
    let route = with_state(|s| s.editor.as_ref().map_or("", |e| e.route)).unwrap_or("");
    let r = window_rect(live.hwnd);
    let (cw, ch) = client_size(live.hwnd);
    let (pw, ph) = plugin_size(&live.view);
    // SAFETY: a window this thread made.
    let dpi = unsafe { GetDpiForWindow(live.hwnd) };
    format!(
        "ok {word} route={route} hwnd={} dpi={dpi} outer={},{},{},{} client={cw}x{ch} plugin={pw}x{ph}",
        live.hwnd as isize, r.left, r.top, r.right, r.bottom
    )
}

fn edit_text() -> String {
    let Some(live) = live() else {
        return "err no editor is open".into();
    };
    let Some(edit) = find_descendant(live.hwnd, "Edit") else {
        return "err no text field".into();
    };
    // SAFETY: a window this thread owns; a buffer of the length asked for.
    let text = unsafe {
        let n = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buffer = vec![0_u16; n + 1];
        GetWindowTextW(edit, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..n])
    };
    format!("ok text={}", text.replace("\r\n", "\\n").replace(' ', "_"))
}

fn run_command(line: &str) -> String {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let Some((&name, args)) = parts.split_first() else {
        return "err empty command".into();
    };
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    match name {
        "ping" => ask(|| "ok pong".into()),
        "open" => {
            let nums = numbers(&args.iter().map(String::as_str).collect::<Vec<_>>());
            let (placement, client) = match nums.as_deref() {
                Some(&[x, y, w, h]) => (Some((x, y)), Some((w, h))),
                _ => (None, None),
            };
            ask(move || open(placement, client))
        }
        "close" => ask(|| {
            close();
            "ok closed".into()
        }),
        "bounds" => ask(|| describe("bounds")),
        "resize" => {
            let nums = numbers(&args.iter().map(String::as_str).collect::<Vec<_>>());
            let Some(&[w, h]) = nums.as_deref() else {
                return "err resize needs width and height".into();
            };
            ask(move || {
                set_client_size(w, h);
                describe("resized")
            })
        }
        "grow" => ask(|| {
            let Some(live) = live() else {
                return "err no editor is open".into();
            };
            let Some(button) = find_descendant(live.hwnd, "Button") else {
                return "err no button".into();
            };
            // SAFETY: a window this thread owns; runs the plugin's handler.
            unsafe { SendMessageW(button, BM_CLICK, 0, 0) };
            describe("grown")
        }),
        "scale" => {
            let Some(factor) = args.first().and_then(|a| a.parse::<f32>().ok()) else {
                return "err scale needs a number".into();
            };
            ask(move || {
                let Some(live) = live() else {
                    return "err no editor is open".into();
                };
                let Some(support) = &live.scale else {
                    return "err the editor has no scale support".into();
                };
                // SAFETY: a live interface.
                unsafe { support.setContentScaleFactor(factor) };
                let (w, h) = plugin_size(&live.view);
                set_client_size(w, h);
                describe("scaled")
            })
        }
        "text" => ask(edit_text),
        "top" => {
            let on = args.first().is_none_or(|a| a == "on");
            ask(move || {
                let Some(live) = live() else {
                    return "err no editor is open".into();
                };
                let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
                // SAFETY: our window.
                unsafe {
                    SetWindowPos(
                        live.hwnd,
                        after,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    )
                };
                "ok top".into()
            })
        }
        "focus" => ask(|| {
            let Some(live) = live() else {
                return "err no editor is open".into();
            };
            // SAFETY: our window.
            let got = unsafe {
                AllowSetForegroundWindow(u32::MAX);
                SetForegroundWindow(live.hwnd) != 0
            };
            format!("ok focus requested={got}")
        }),
        "state" => ask(|| {
            let Some(live) = live() else {
                return "err no editor is open".into();
            };
            // SAFETY: windows of this thread; read-only queries.
            unsafe {
                let owner = GetWindow(live.hwnd, GW_OWNER) as isize;
                let top = GetWindowLongPtrW(live.hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0;
                let focus = class_name(GetFocus());
                format!(
                    "ok state visible={} foreground={} owner={owner} topmost={top} focus={focus} hung={}",
                    IsWindowVisible(live.hwnd) != 0,
                    GetForegroundWindow() == live.hwnd,
                    IsHungAppWindow(live.hwnd) != 0,
                )
            }
        }),
        "quit" => {
            ask(|| {
                // SAFETY: ends this thread's message loop.
                unsafe { PostQuitMessage(0) };
                "ok bye".into()
            })
        }
        other => format!("err unknown command {other}"),
    }
}

fn read_commands() {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let (id, command) = line.split_once(' ').unwrap_or((&line, ""));
        let answer = run_command(command);
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{id} {answer}");
        let _ = out.flush();
        if command == "quit" {
            break;
        }
    }
    // The stand-in went away: so does the host.
    // SAFETY: posting a quit to the window thread.
    unsafe {
        let target = DISPATCHER.load(Ordering::Relaxed) as HWND;
        PostMessageW(target, WM_APP + 1, 0, 0);
    }
}

pub fn run(args: &[String]) -> ExitCode {
    let args = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("lumen-editor-spike host: {e}");
            return ExitCode::from(2);
        }
    };
    per_monitor_dpi();
    // SAFETY: once, on this thread, before any plugin code.
    unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
    let module = match Module::load(&args.plugin) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("lumen-editor-spike host: load: {e}");
            return ExitCode::from(1);
        }
    };
    let wanted = args.class.to_lowercase();
    let class = module
        .classes()
        .into_iter()
        .filter(|c| c.category == AUDIO_CLASS)
        .find(|c| wanted == "first" || c.name.to_lowercase().contains(&wanted));
    let Some(class) = class else {
        eprintln!(
            "lumen-editor-spike host: no audio class matches `{}`",
            args.class
        );
        return ExitCode::from(1);
    };
    let instance = match Instance::create(&module, Some(class.id)) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("lumen-editor-spike host: create: {e}");
            return ExitCode::from(1);
        }
    };
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            instance,
            _module: module,
            owner: args.owner,
            topmost: args.topmost,
            editor: None,
        });
    });

    static DISPATCH: Once = Once::new();
    register(DISPATCH_CLASS, dispatch_proc, &DISPATCH);
    let name = wide(DISPATCH_CLASS);
    // SAFETY: a message-only window for commands, on this thread.
    let dispatcher = unsafe {
        CreateWindowExW(
            0,
            name.as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    DISPATCHER.store(dispatcher as isize, Ordering::Relaxed);
    event(&format!("ready class={}", class.name.replace(' ', "_")));
    std::thread::spawn(read_commands);

    let mut msg = MSG {
        hwnd: null_mut(),
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: POINT { x: 0, y: 0 },
    };
    // SAFETY: the standard message loop of this thread.
    unsafe {
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.message == WM_APP + 1 && msg.hwnd == dispatcher {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // Leave without running the plugin's or the module's shutdown code: a
    // plugin stuck in its window code must not keep the process alive.
    lumen_plugin_host::process::end_now(0)
}
