//! A plugin's editor window (M6 slice 4). The window belongs to the host
//! process, never to Lumen's window (ADR-0067, "Editors"): a top-level window
//! made, pumped and destroyed on the host's main thread, which is also the
//! thread that made the plugin.
//!
//! [`Editors`] is what the host's request handler talks to. Windows has the
//! real thing ([`win`]); elsewhere [`Unavailable`] says so, and
//! [`Headless`], switched on by `LUMEN_HOST_HEADLESS_EDITOR`, stands in for a
//! window so the whole path (open, move, close by the user, a window that
//! stops answering) can be tested where there is no window system.

use std::sync::Arc;
use std::time::{Duration, Instant};

use vst3::ComPtr;
use vst3::Steinberg::IPlugView;

use crate::protocol::{EditorBounds, EditorStatus, Response};

#[cfg(windows)]
pub mod win;

/// The environment variable that switches the stand-in on. Its value is a
/// comma-separated list: `on`, `hang=MS` (opening blocks the main thread for
/// `MS` milliseconds, the way a plugin stuck in its own window code does),
/// `close=MS` (the "user" closes the window `MS` milliseconds after it opens),
/// `press=CODE` (once Lumen asks for the key `CODE` while the window is open,
/// the "user" presses and lets go of it there).
pub const HEADLESS_VAR: &str = "LUMEN_HOST_HEADLESS_EDITOR";

/// Where the host's news goes: written to Lumen between answers.
pub type Notify = Arc<dyn Fn(Response) + Send + Sync>;

/// What the host's handler does with an editor window. Every method runs on
/// the host's main thread.
pub trait Editors {
    /// Opens the editor of the plugin called `title` whose view is `view`
    /// (none when the plugin makes none) at `wanted`, or where the system
    /// puts it.
    fn open(
        &mut self,
        title: &str,
        view: Option<ComPtr<IPlugView>>,
        wanted: Option<EditorBounds>,
    ) -> Result<EditorStatus, String>;

    /// Closes the window, if there is one. The status says where it was.
    fn close(&mut self) -> EditorStatus;

    /// The window as it is now. A window the user closed reads as not open,
    /// still with the place it was last seen at.
    fn status(&mut self) -> EditorStatus;

    /// Takes `keys` (virtual-key codes) from the window when pressed alone,
    /// telling Lumen of each through the notifier; empty gives them back.
    /// Keys taken and still down are let go of (and Lumen told) when they
    /// stop being taken, the window loses the keyboard or closes.
    fn capture_keys(&mut self, _keys: Vec<u8>) {}

    /// Where keys taken from the window are told.
    fn set_notify(&mut self, _notify: Notify) {}
}

/// The editors of this platform: windows on Windows, the stand-in when asked
/// for, else none.
pub fn platform() -> Box<dyn Editors> {
    if let Ok(setting) = std::env::var(HEADLESS_VAR)
        && !setting.is_empty()
    {
        return Box::new(Headless::parse(&setting));
    }
    #[cfg(windows)]
    {
        Box::new(win::Windows)
    }
    #[cfg(not(windows))]
    {
        Box::new(Unavailable)
    }
}

/// A platform with no editor windows yet.
pub struct Unavailable;

impl Editors for Unavailable {
    fn open(
        &mut self,
        _title: &str,
        _view: Option<ComPtr<IPlugView>>,
        _wanted: Option<EditorBounds>,
    ) -> Result<EditorStatus, String> {
        Err("plugin editors are not available on this platform yet".into())
    }

    fn close(&mut self) -> EditorStatus {
        EditorStatus::default()
    }

    fn status(&mut self) -> EditorStatus {
        EditorStatus::default()
    }
}

/// A window that is only a record of where it would be (see the module's
/// note). The size is the plugin's own 320 by 200 when none is asked for.
pub struct Headless {
    hang: Option<Duration>,
    close_after: Option<Duration>,
    /// The key the "user" presses once it is asked for.
    press: Option<u8>,
    opened: Option<Instant>,
    bounds: Option<EditorBounds>,
    notify: Option<Notify>,
}

impl Headless {
    pub fn parse(setting: &str) -> Self {
        let number = |key: &str| {
            setting
                .split(',')
                .filter_map(|t| t.trim().strip_prefix(key)?.strip_prefix('='))
                .find_map(|v| v.parse::<u64>().ok())
        };
        Self {
            hang: number("hang").map(Duration::from_millis),
            close_after: number("close").map(Duration::from_millis),
            press: number("press").and_then(|c| u8::try_from(c).ok()),
            opened: None,
            bounds: None,
            notify: None,
        }
    }
}

impl Editors for Headless {
    fn open(
        &mut self,
        _title: &str,
        _view: Option<ComPtr<IPlugView>>,
        wanted: Option<EditorBounds>,
    ) -> Result<EditorStatus, String> {
        if self.opened.is_some() {
            return Err("an editor is already open".into());
        }
        if let Some(hang) = self.hang {
            std::thread::sleep(hang);
        }
        self.opened = Some(Instant::now());
        self.bounds = Some(wanted.unwrap_or(EditorBounds {
            x: 100,
            y: 100,
            width: 320,
            height: 200,
            dpi: 96,
        }));
        Ok(self.status())
    }

    fn close(&mut self) -> EditorStatus {
        self.opened = None;
        EditorStatus {
            open: false,
            bounds: self.bounds,
        }
    }

    fn status(&mut self) -> EditorStatus {
        if let (Some(opened), Some(after)) = (self.opened, self.close_after)
            && opened.elapsed() >= after
        {
            return self.close();
        }
        EditorStatus {
            open: self.opened.is_some(),
            bounds: self.bounds,
        }
    }

    fn capture_keys(&mut self, keys: Vec<u8>) {
        let (Some(code), Some(notify)) = (self.press, &self.notify) else {
            return;
        };
        if self.opened.is_some() && keys.contains(&code) {
            notify(Response::Key { code, down: true });
            notify(Response::Key { code, down: false });
        }
    }

    fn set_notify(&mut self, notify: Notify) {
        self.notify = Some(notify);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stand_in_opens_where_it_is_told_and_closes() {
        let mut editors = Headless::parse("on");
        assert!(!editors.status().open);
        let at = EditorBounds {
            x: -30,
            y: 40,
            width: 500,
            height: 400,
            dpi: 144,
        };
        let status = editors.open("Plugin", None, Some(at)).unwrap();
        assert!(status.open);
        assert_eq!(status.bounds, Some(at));
        assert!(
            editors.open("Plugin", None, None).is_err(),
            "one window at a time"
        );
        let closed = editors.close();
        assert!(!closed.open);
        assert_eq!(closed.bounds, Some(at), "where it was, to be remembered");
        assert!(!editors.status().open);
    }

    #[test]
    fn the_user_can_close_the_stand_in_by_itself() {
        let mut editors = Headless::parse("on,close=10");
        editors.open("Plugin", None, None).unwrap();
        assert!(editors.status().open);
        std::thread::sleep(Duration::from_millis(30));
        assert!(!editors.status().open, "closed after its time");
    }

    #[test]
    fn a_platform_with_no_windows_says_so() {
        let mut editors = Unavailable;
        assert!(
            editors
                .open("Plugin", None, None)
                .unwrap_err()
                .contains("platform")
        );
    }
}
