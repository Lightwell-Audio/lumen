use std::sync::Arc;
use std::time::{Duration, Instant};

use vst3::ComPtr;
use vst3::Steinberg::IPlugView;

use crate::protocol::{EditorBounds, EditorStatus, Response};

#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(windows)]
pub mod win;

pub const HEADLESS_VAR: &str = "LUMEN_HOST_HEADLESS_EDITOR";

pub type Notify = Arc<dyn Fn(Response) + Send + Sync>;

pub trait Editors {
    fn open(
        &mut self,
        title: &str,
        view: Option<ComPtr<IPlugView>>,
        wanted: Option<EditorBounds>,
    ) -> Result<EditorStatus, String>;

    fn close(&mut self) -> EditorStatus;

    fn set_visible(&mut self, _visible: bool) -> EditorStatus {
        self.status()
    }

    fn status(&mut self) -> EditorStatus;

    fn capture_keys(&mut self, _keys: Vec<u8>) {}

    fn set_notify(&mut self, _notify: Notify) {}
}

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
    #[cfg(target_os = "macos")]
    {
        Box::new(mac::MacEditors::default())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Box::new(Unavailable)
    }
}

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

pub struct Headless {
    hang: Option<Duration>,
    close_after: Option<Duration>,
    press: Option<u8>,
    opened: Option<Instant>,
    hidden: bool,
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
            hidden: false,
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
        self.hidden = false;
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
            visible: false,
            bounds: self.bounds,
        }
    }

    fn set_visible(&mut self, visible: bool) -> EditorStatus {
        if self.opened.is_some() {
            self.hidden = !visible;
        }
        self.status()
    }

    fn status(&mut self) -> EditorStatus {
        if let (Some(opened), Some(after)) = (self.opened, self.close_after)
            && opened.elapsed() >= after
        {
            return self.close();
        }
        EditorStatus {
            open: self.opened.is_some(),
            visible: self.opened.is_some() && !self.hidden,
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
        assert!(editors.status().visible);
        assert!(!editors.set_visible(false).visible, "hidden");
        assert!(editors.status().open, "hidden is still open");
        assert!(editors.set_visible(true).visible, "shown again");
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
