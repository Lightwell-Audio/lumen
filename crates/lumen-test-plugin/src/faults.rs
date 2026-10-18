//! Faults the test plugins inject on purpose, chosen by the
//! `LUMEN_TEST_PLUGIN_FAULT` environment variable so that a host process
//! inherits them. The value is a comma-separated list:
//!
//! | Entry | Effect |
//! |---|---|
//! | `factory_null` | `GetPluginFactory` returns no factory |
//! | `factory_crash` | `GetPluginFactory` writes through a null pointer |
//! | `factory_hang` | `GetPluginFactory` never returns |
//! | `process_crash@N` | the Nth process call after activation crashes |
//! | `process_hang@N` | the Nth process call never returns |
//! | `process_error@N` | process calls from the Nth on return an error |
//! | `process_nan@N` | process calls from the Nth on write NaN |
//! | `process_slow=US` | every process call busy-waits `US` microseconds |
//! | `state_fail` | saving state reports failure |
//! | `state_garbage` | saving state writes bytes loading rejects |
//! | `state_hang` | saving state never returns |
//! | `bad_buses` | the output bus reports an absurd channel count |
//! | `bad_params` | the controller reports millions of duplicate parameters |
//! | `needs_message` | `initialize` asks the host for a message object (and its attribute list) and crashes if it gets none: what plugins with a separate controller do |
//! | `reports_params` | `process` reports a parameter change through the host's output list and crashes if the host gives it no queue to put it in: what plugins that report their own edits do |
//! | `aux_buses` | the instrument declares one main and three auxiliary output buses (the auxiliary ones off by default) and crashes in `process` unless the host passes all four, each with a buffer list that is not null even when the bus is off |
//! | `editor` | the controller offers an editor view (Windows): a text field to type in, a resize button, user resizing and a scale readout; without it there is no view |
//! | `editor_hang=MS` | as `editor`, and the editor's window thread stops answering `MS` milliseconds after it is attached |
//! | `set_dirty` | the instrument lists the trigger, and stepping it on makes the plugin tell the host `setDirty(true)` and nothing else: a change inside it that no parameter shows |
//! | `values_changed` | as `set_dirty`, but the plugin asks the host to restart with `kParamValuesChanged`, as a plugin does when a patch loads |
//! | `headless_editor` | no effect on the plugin: the host (not this plugin) stands in for an editor window that is only a record of where it would be, for tests where there are no windows (`LUMEN_HOST_HEADLESS_EDITOR`) |
//! | `headless_hang=MS` | as `headless_editor`, and opening it blocks the host's main thread for `MS` milliseconds |
//! | `headless_close=MS` | as `headless_editor`, and the "user" closes it `MS` milliseconds after it opens |
//! | `headless_press=CODE` | as `headless_editor`, and once Lumen asks for the key `CODE` (a Windows virtual-key code) while the window is open, the "user" presses and lets go of it there |
//! | `presets` | the instrument offers a program list (three programs, selected by a program-change parameter, id 91, that sets its level and wave as edits of its own) and both plugins keep an edit-controller state (a view value) that `getState` writes and `setState` restores |
//! | `release=MS` | the instrument's released notes fade out over `MS` milliseconds while it reports no tail: what a synth with a long release does |
//!
//! `@N` counts process calls from 0 and defaults to 0.

use std::time::{Duration, Instant};

pub const ENV: &str = "LUMEN_TEST_PLUGIN_FAULT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    FactoryNull,
    FactoryCrash,
    FactoryHang,
    ProcessCrash,
    ProcessHang,
    ProcessError,
    ProcessNan,
    ProcessSlow,
    StateFail,
    StateGarbage,
    /// The component refuses its first `arg` `setState` calls (a plugin still
    /// loading its content).
    StateRefused,
    StateHang,
    BadBuses,
    BadParams,
    NeedsMessage,
    ReportsParams,
    AuxBuses,
    Editor,
    EditorHang,
    Release,
    ManyParams,
    Presets,
    WindowEdits,
    SetDirty,
    ValuesChanged,
    HeadlessEditor,
    HeadlessHang,
    HeadlessClose,
    HeadlessPress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault {
    pub kind: Kind,
    /// The process call it starts at (`@N`), or the busy-wait in
    /// microseconds for `process_slow`.
    pub arg: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Faults(Vec<Fault>);

impl Faults {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut list = Vec::new();
        for entry in text.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (name, arg) = match entry.split_once(['@', '=']) {
                Some((name, arg)) => {
                    let arg = arg
                        .parse::<u64>()
                        .map_err(|_| format!("`{entry}`: `{arg}` is not a number"))?;
                    (name, Some(arg))
                }
                None => (entry, None),
            };
            let kind = match name {
                "factory_null" => Kind::FactoryNull,
                "factory_crash" => Kind::FactoryCrash,
                "factory_hang" => Kind::FactoryHang,
                "process_crash" => Kind::ProcessCrash,
                "process_hang" => Kind::ProcessHang,
                "process_error" => Kind::ProcessError,
                "process_nan" => Kind::ProcessNan,
                "process_slow" => Kind::ProcessSlow,
                "state_fail" => Kind::StateFail,
                "state_garbage" => Kind::StateGarbage,
                "state_refused" => Kind::StateRefused,
                "state_hang" => Kind::StateHang,
                "bad_buses" => Kind::BadBuses,
                "bad_params" => Kind::BadParams,
                "needs_message" => Kind::NeedsMessage,
                "reports_params" => Kind::ReportsParams,
                "aux_buses" => Kind::AuxBuses,
                "editor" => Kind::Editor,
                "editor_hang" => Kind::EditorHang,
                "release" => Kind::Release,
                "many_params" => Kind::ManyParams,
                "presets" => Kind::Presets,
                "window_edits" => Kind::WindowEdits,
                "set_dirty" => Kind::SetDirty,
                "values_changed" => Kind::ValuesChanged,
                "headless_editor" => Kind::HeadlessEditor,
                "headless_hang" => Kind::HeadlessHang,
                "headless_close" => Kind::HeadlessClose,
                "headless_press" => Kind::HeadlessPress,
                _ => return Err(format!("`{entry}`: unknown fault")),
            };
            if matches!(kind, Kind::ProcessSlow) && arg.is_none() {
                return Err(format!("`{entry}`: needs `=microseconds`"));
            }
            if matches!(kind, Kind::StateRefused) && arg.is_none() {
                return Err(format!("`{entry}`: needs `=count`"));
            }
            if matches!(kind, Kind::ManyParams) && arg.is_none() {
                return Err(format!("`{entry}`: needs `=count`"));
            }
            if matches!(kind, Kind::HeadlessPress) && arg.is_none() {
                return Err(format!("`{entry}`: needs `=key code`"));
            }
            if matches!(
                kind,
                Kind::EditorHang | Kind::Release | Kind::HeadlessHang | Kind::HeadlessClose
            ) && arg.is_none()
            {
                return Err(format!("`{entry}`: needs `=milliseconds`"));
            }
            list.push(Fault {
                kind,
                arg: arg.unwrap_or(0),
            });
        }
        Ok(Self(list))
    }

    /// The faults the environment asks for. A malformed value is reported on
    /// stderr and injects nothing: a typo must not look like a plugin bug.
    pub fn from_env() -> Self {
        let Ok(text) = std::env::var(ENV) else {
            return Self::default();
        };
        Self::parse(&text).unwrap_or_else(|why| {
            eprintln!("{ENV}: {why}");
            Self::default()
        })
    }

    pub fn has(&self, kind: Kind) -> bool {
        self.0.iter().any(|f| f.kind == kind)
    }

    /// Whether `kind` applies to process call number `call` (from its
    /// start call onward).
    pub fn from_call(&self, kind: Kind, call: u64) -> bool {
        self.0.iter().any(|f| f.kind == kind && call >= f.arg)
    }

    /// Whether `kind` applies exactly at process call number `call`.
    pub fn at_call(&self, kind: Kind, call: u64) -> bool {
        self.0.iter().any(|f| f.kind == kind && call == f.arg)
    }

    /// Whether the controller offers an editor view (`editor` or
    /// `editor_hang`).
    pub fn editor(&self) -> bool {
        self.has(Kind::Editor) || self.has(Kind::EditorHang)
    }

    /// How long after attaching the editor stops answering, if it should.
    pub fn editor_hang(&self) -> Option<Duration> {
        self.0
            .iter()
            .find(|f| f.kind == Kind::EditorHang)
            .map(|f| Duration::from_millis(f.arg))
    }

    /// How many `setState` calls are refused first (`state_refused`).
    pub fn state_refusals(&self) -> u64 {
        self.0
            .iter()
            .find(|f| f.kind == Kind::StateRefused)
            .map_or(0, |f| f.arg)
    }

    /// How long released notes take to fade out (`release`), if asked.
    pub fn release(&self) -> Option<Duration> {
        self.0
            .iter()
            .find(|f| f.kind == Kind::Release)
            .map(|f| Duration::from_millis(f.arg))
    }

    /// How many extra parameters `many_params` adds (0 when not asked).
    pub fn many_params(&self) -> u32 {
        self.0
            .iter()
            .find(|f| f.kind == Kind::ManyParams)
            .map_or(0, |f| f.arg.min(20_000) as u32)
    }

    /// The busy-wait asked for by `process_slow`, if any.
    pub fn slow(&self) -> Option<Duration> {
        self.0
            .iter()
            .find(|f| f.kind == Kind::ProcessSlow)
            .map(|f| Duration::from_micros(f.arg))
    }
}

/// Crashes the way a plugin with a wild pointer does.
pub fn crash() -> ! {
    // SAFETY: deliberately not safe: this simulates a plugin crashing.
    unsafe { std::ptr::null_mut::<u32>().write_volatile(0xDEAD) };
    std::process::abort()
}

/// Never returns.
pub fn hang() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

/// Burns CPU for `d`, as a plugin doing real work does.
pub fn busy_wait(d: Duration) {
    let start = Instant::now();
    while start.elapsed() < d {
        std::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_form() {
        let f = Faults::parse("process_crash@7, process_slow=250,state_fail,bad_params").unwrap();
        assert!(f.at_call(Kind::ProcessCrash, 7));
        assert!(!f.at_call(Kind::ProcessCrash, 8));
        assert_eq!(f.slow(), Some(Duration::from_micros(250)));
        assert!(f.has(Kind::StateFail) && f.has(Kind::BadParams));
        assert!(!f.has(Kind::StateHang));
    }

    #[test]
    fn the_editor_switches_parse_and_imply_the_view() {
        assert!(!Faults::parse("process_slow=5").unwrap().editor());
        assert!(Faults::parse("editor").unwrap().editor());
        let hung = Faults::parse("editor_hang=250").unwrap();
        assert!(hung.editor());
        assert_eq!(hung.editor_hang(), Some(Duration::from_millis(250)));
        assert!(Faults::parse("editor_hang").is_err());
    }

    #[test]
    fn a_release_is_in_milliseconds() {
        let f = Faults::parse("release=4000").unwrap();
        assert_eq!(f.release(), Some(Duration::from_millis(4000)));
        assert_eq!(Faults::default().release(), None);
        assert!(Faults::parse("release").is_err());
    }

    #[test]
    fn from_call_starts_at_the_named_call() {
        let f = Faults::parse("process_nan@3").unwrap();
        assert!(!f.from_call(Kind::ProcessNan, 2));
        assert!(f.from_call(Kind::ProcessNan, 3));
        assert!(f.from_call(Kind::ProcessNan, 1_000));
        let always = Faults::parse("process_error").unwrap();
        assert!(always.from_call(Kind::ProcessError, 0));
    }

    #[test]
    fn rejects_typos_rather_than_injecting_nothing_silently() {
        assert!(Faults::parse("process_crash@x").is_err());
        assert!(Faults::parse("prcess_crash").is_err());
        assert!(Faults::parse("process_slow").is_err());
        assert_eq!(Faults::parse("").unwrap(), Faults::default());
    }
}
