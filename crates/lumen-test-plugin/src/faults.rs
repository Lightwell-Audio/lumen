use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

pub const ENV: &str = "LUMEN_TEST_PLUGIN_FAULT";

pub const LIFECYCLE_COMPONENT_CONNECTED: u32 = 1 << 0;
pub const LIFECYCLE_CONTROLLER_CONNECTED: u32 = 1 << 1;
pub const LIFECYCLE_COMPONENT_DISCONNECTED: u32 = 1 << 2;
pub const LIFECYCLE_CONTROLLER_DISCONNECTED: u32 = 1 << 3;
pub const LIFECYCLE_COMPONENT_TERMINATED: u32 = 1 << 4;
pub const LIFECYCLE_CONTROLLER_TERMINATED: u32 = 1 << 5;

static LIFECYCLE: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    FactoryNull,
    FactoryCrash,
    FactoryHang,
    Prints,
    EntryFalse,
    EntryRequired,
    LifecycleOrder,
    ExitCrash,
    ProcessCrash,
    ProcessHang,
    ProcessError,
    ProcessNan,
    ProcessSlow,
    StateFail,
    StateGarbage,
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
    InstrumentLatency,
    MultiOut,
    Sidechain,
    BusChange,
    ManyParams,
    Presets,
    WindowEdits,
    SetDirty,
    ValuesChanged,
    TitlesChanged,
    HeadlessEditor,
    HeadlessHang,
    HeadlessClose,
    HeadlessPress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault {
    pub kind: Kind,
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
                "entry_false" => Kind::EntryFalse,
                "entry_required" => Kind::EntryRequired,
                "lifecycle_order" => Kind::LifecycleOrder,
                "exit_crash" => Kind::ExitCrash,
                "prints" => Kind::Prints,
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
                "instrument_latency" => Kind::InstrumentLatency,
                "multi_out" => Kind::MultiOut,
                "sidechain" => Kind::Sidechain,
                "bus_change" => Kind::BusChange,
                "many_params" => Kind::ManyParams,
                "presets" => Kind::Presets,
                "window_edits" => Kind::WindowEdits,
                "set_dirty" => Kind::SetDirty,
                "values_changed" => Kind::ValuesChanged,
                "titles_changed" => Kind::TitlesChanged,
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
            if matches!(kind, Kind::MultiOut) && !arg.is_some_and(|n| (2..=256).contains(&n)) {
                return Err(format!("`{entry}`: needs `=buses` from 2 to 256"));
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

    pub fn from_call(&self, kind: Kind, call: u64) -> bool {
        self.0.iter().any(|f| f.kind == kind && call >= f.arg)
    }

    pub fn at_call(&self, kind: Kind, call: u64) -> bool {
        self.0.iter().any(|f| f.kind == kind && call == f.arg)
    }

    pub fn editor(&self) -> bool {
        self.has(Kind::Editor) || self.has(Kind::EditorHang)
    }

    pub fn editor_hang(&self) -> Option<Duration> {
        self.0
            .iter()
            .find(|f| f.kind == Kind::EditorHang)
            .map(|f| Duration::from_millis(f.arg))
    }

    pub fn state_refusals(&self) -> u64 {
        self.0
            .iter()
            .find(|f| f.kind == Kind::StateRefused)
            .map_or(0, |f| f.arg)
    }

    pub fn release(&self) -> Option<Duration> {
        self.0
            .iter()
            .find(|f| f.kind == Kind::Release)
            .map(|f| Duration::from_millis(f.arg))
    }

    pub fn multi_out(&self) -> Option<u32> {
        self.0
            .iter()
            .find(|f| f.kind == Kind::MultiOut)
            .map(|f| f.arg as u32)
    }

    pub fn instrument_latency(&self) -> Option<u32> {
        self.0
            .iter()
            .find(|f| f.kind == Kind::InstrumentLatency)
            .map(|f| f.arg.min(u64::from(crate::params::MAX_LATENCY)) as u32)
    }

    pub fn many_params(&self) -> u32 {
        self.0
            .iter()
            .find(|f| f.kind == Kind::ManyParams)
            .map_or(0, |f| f.arg.min(20_000) as u32)
    }

    pub fn slow(&self) -> Option<Duration> {
        self.0
            .iter()
            .find(|f| f.kind == Kind::ProcessSlow)
            .map(|f| Duration::from_micros(f.arg))
    }
}

pub fn lifecycle_reset(faults: &Faults) {
    if faults.has(Kind::LifecycleOrder) {
        LIFECYCLE.store(0, Ordering::Relaxed);
    }
}

pub fn lifecycle_record(faults: &Faults, event: u32) {
    if faults.has(Kind::LifecycleOrder) {
        LIFECYCLE.fetch_or(event, Ordering::Relaxed);
    }
}

pub fn lifecycle_has(faults: &Faults, events: u32) -> bool {
    !faults.has(Kind::LifecycleOrder) || LIFECYCLE.load(Ordering::Relaxed) & events == events
}

pub fn crash() -> ! {
    unsafe { std::ptr::null_mut::<u32>().write_volatile(0xDEAD) };
    std::process::abort()
}

pub fn hang() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

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
