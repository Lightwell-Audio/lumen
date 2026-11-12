//! Opening `.lumen` files with Lumen (M9 R8, FA A, ADR-0117): the values
//! under the current user's `Software\Classes` that make a project open
//! from the file manager with its own icon.
//!
//! - `.lumen` names the ProgID (`Lumen.Project`) and lists it under
//!   `OpenWithProgids`;
//! - `Lumen.Project` holds the friendly type name, the icon (the
//!   executable's resource 2) and the open command;
//! - `Applications\lumen.exe` lists the type it opens.
//!
//! The copy whose path is in the ProgID's open command owns the
//! association. Turning it off removes only what names this copy, and
//! never another program's values or the `.lumen` key itself. Windows
//! keeps the person's own "Open with" choice (`UserChoice`) protected:
//! Lumen reads it to say what will happen, and never writes it.
//!
//! Every read and write goes through a [`Hive`]: tests and isolated runs
//! use a map, and only a normal start writes the registry.

#[cfg(windows)]
mod registry;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lumen_core::product::PRODUCT_NAME;

/// The extension's key.
pub(crate) const EXT: &str = r"Software\Classes\.lumen";
/// The person's choice in "Open with", which Windows protects.
pub(crate) const USER_CHOICE: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.lumen\UserChoice";

/// The ProgID: `Lumen.Project`.
pub(crate) fn prog_id() -> String {
    format!("{PRODUCT_NAME}.Project")
}

fn prog_key() -> String {
    format!(r"Software\Classes\{}", prog_id())
}

fn app_key(exe: &Path) -> String {
    // The registry's paths are Windows paths: split on either slash, the
    // same on every system the tests run on.
    let path = exe.to_string_lossy();
    let name = path
        .rsplit(['\\', '/'])
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or("lumen.exe");
    format!(r"Software\Classes\Applications\{name}")
}

fn command_for(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

/// The registry under the current user, or a stand-in for it. Keys are
/// paths below `HKEY_CURRENT_USER`; the value `""` is a key's default.
pub(crate) trait Hive: Send {
    fn get(&self, key: &str, name: &str) -> Option<String>;
    fn set(&mut self, key: &str, name: &str, value: &str) -> io::Result<()>;
    /// Removes a value; a value that is not there is not an error.
    fn remove_value(&mut self, key: &str, name: &str) -> io::Result<()>;
    /// Removes a key and everything under it; a missing key is not an error.
    fn remove_key(&mut self, key: &str) -> io::Result<()>;
    /// Tells the file manager the associations changed.
    fn announce(&mut self) {}
}

/// Who opens `.lumen` files now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum State {
    /// No copy of Lumen is registered.
    Off,
    /// This copy opens them.
    This,
    /// Another copy of Lumen, at this path, is registered.
    Other(PathBuf),
}

/// What the row shows: who is registered, and the person's own choice in
/// "Open with" when it names another program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Status {
    pub(crate) state: State,
    /// The ProgID the person chose in "Open with", when it is not Lumen's.
    pub(crate) chosen_elsewhere: Option<String>,
    /// The person's choice still names Lumen's ProgID while nothing is
    /// registered: Windows will ask which program to use.
    pub(crate) will_ask: bool,
}

/// Reads who opens `.lumen` files.
pub(crate) fn status(hive: &dyn Hive, exe: &Path) -> Status {
    let command = hive.get(&format!(r"{}\shell\open\command", prog_key()), "");
    let state = match command.as_deref().and_then(command_exe) {
        None => State::Off,
        Some(owner) if same_exe(&owner, exe) => State::This,
        Some(owner) => State::Other(owner),
    };
    let choice = hive.get(USER_CHOICE, "ProgId");
    let ours = prog_id();
    Status {
        chosen_elsewhere: choice.clone().filter(|c| !c.eq_ignore_ascii_case(&ours)),
        will_ask: state == State::Off && choice.is_some_and(|c| c.eq_ignore_ascii_case(&ours)),
        state,
    }
}

/// The status when the hive cannot be read at all (a poisoned lock): as far
/// as the interface can say, nothing is registered.
pub(crate) fn unreadable() -> Status {
    Status {
        state: State::Off,
        chosen_elsewhere: None,
        will_ask: false,
    }
}

/// Registers this copy (`exe`) to open `.lumen` files. The handler is
/// written before the extension points to it, and a write that fails puts
/// back every value written before it: a failure never leaves `.lumen`
/// half pointed at this copy.
pub(crate) fn register(hive: &mut dyn Hive, exe: &Path) -> io::Result<()> {
    let id = prog_id();
    let prog = prog_key();
    let command = command_for(exe);
    let app = app_key(exe);
    let writes = [
        (
            prog.clone(),
            String::new(),
            format!("{PRODUCT_NAME} project"),
        ),
        (
            format!(r"{prog}\DefaultIcon"),
            String::new(),
            format!("\"{}\",-2", exe.display()),
        ),
        (
            format!(r"{prog}\shell\open\command"),
            String::new(),
            command.clone(),
        ),
        (
            format!(r"{app}\SupportedTypes"),
            ".lumen".into(),
            String::new(),
        ),
        (format!(r"{app}\shell\open\command"), String::new(), command),
        (format!(r"{EXT}\OpenWithProgids"), id.clone(), String::new()),
        (EXT.to_owned(), String::new(), id),
    ];
    let mut done: Vec<(&str, &str, Option<String>)> = Vec::new();
    for (key, name, value) in &writes {
        let before = hive.get(key, name);
        if let Err(e) = hive.set(key, name, value) {
            for (key, name, before) in done.into_iter().rev() {
                let undone = match before {
                    Some(v) => hive.set(key, name, &v),
                    None => hive.remove_value(key, name),
                };
                if let Err(u) = undone {
                    crate::trace::log(|| format!("association: {key} not put back ({u})"));
                }
            }
            return Err(e);
        }
        done.push((key, name, before));
    }
    hive.announce();
    Ok(())
}

/// Removes this copy's registration; does nothing when another copy owns
/// it. Never touches another program's values or the `.lumen` key. The
/// ProgID's open command, which says this copy owns the association, goes
/// last: a removal that fails part way leaves it, so turning it off again
/// finishes the job.
pub(crate) fn unregister(hive: &mut dyn Hive, exe: &Path) -> io::Result<()> {
    if status(hive, exe).state != State::This {
        return Ok(());
    }
    let done = remove_ours(hive, exe);
    hive.announce();
    done
}

fn remove_ours(hive: &mut dyn Hive, exe: &Path) -> io::Result<()> {
    let id = prog_id();
    if hive
        .get(EXT, "")
        .is_some_and(|v| v.eq_ignore_ascii_case(&id))
    {
        hive.remove_value(EXT, "")?;
    }
    hive.remove_value(&format!(r"{EXT}\OpenWithProgids"), &id)?;
    let app = app_key(exe);
    let names_us = hive
        .get(&format!(r"{app}\shell\open\command"), "")
        .as_deref()
        .and_then(command_exe)
        .is_some_and(|e| same_exe(&e, exe));
    if names_us {
        hive.remove_key(&app)?;
    }
    hive.remove_key(&prog_key())
}

/// The program an open command runs: its first argument, quoted or not.
fn command_exe(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    let exe = match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => command.split_whitespace().next()?,
    };
    (!exe.is_empty()).then(|| PathBuf::from(exe))
}

/// Whether two paths name one program: compared as written (in any case,
/// and with either slash, on Windows), never on disk. The status is read
/// while the row is drawn, and another copy's path may be on a drive that
/// is slow or gone. The path this copy writes is its own, as it started;
/// one spelled another way shows as another copy, and Use this copy
/// writes it again.
fn same_exe(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        let plain = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
        plain(a) == plain(b)
    } else {
        a == b
    }
}

/// A hive in memory: for tests, and for isolated runs, which log what they
/// would write and never touch the registry.
#[derive(Default)]
pub(crate) struct MapHive {
    values: BTreeMap<(String, String), String>,
    /// Tests: every write fails with this reason.
    pub(crate) fail: Option<String>,
    /// Tests: the write after this many more is refused, once.
    pub(crate) fail_after: Option<usize>,
    /// Isolated runs: log each write.
    pub(crate) log: bool,
}

impl MapHive {
    fn at(key: &str, name: &str) -> (String, String) {
        (key.to_lowercase(), name.to_lowercase())
    }

    fn refuse(&mut self) -> io::Result<()> {
        if let Some(left) = &mut self.fail_after {
            if *left == 0 {
                self.fail_after = None;
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "refused"));
            }
            *left -= 1;
        }
        match &self.fail {
            Some(why) => Err(io::Error::new(io::ErrorKind::PermissionDenied, why.clone())),
            None => Ok(()),
        }
    }

    fn note(&self, what: impl FnOnce() -> String) {
        if self.log {
            crate::trace::log(|| format!("association (not written): {}", what()));
        }
    }
}

impl Hive for MapHive {
    fn get(&self, key: &str, name: &str) -> Option<String> {
        self.values.get(&Self::at(key, name)).cloned()
    }

    fn set(&mut self, key: &str, name: &str, value: &str) -> io::Result<()> {
        self.refuse()?;
        self.note(|| format!(r"set {key} [{name}] = {value}"));
        self.values.insert(Self::at(key, name), value.to_owned());
        Ok(())
    }

    fn remove_value(&mut self, key: &str, name: &str) -> io::Result<()> {
        self.refuse()?;
        self.note(|| format!(r"remove {key} [{name}]"));
        self.values.remove(&Self::at(key, name));
        Ok(())
    }

    fn remove_key(&mut self, key: &str) -> io::Result<()> {
        self.refuse()?;
        self.note(|| format!(r"remove {key}"));
        let key = key.to_lowercase();
        let below = format!("{key}\\");
        self.values
            .retain(|(k, _), _| *k != key && !k.starts_with(&below));
        Ok(())
    }
}

/// Which hive a run uses.
pub(crate) enum Source {
    /// The registry (a normal start on Windows).
    System,
    /// A map that logs what it would write (tests, isolated runs).
    Isolated,
}

/// The association as the app holds it: the hive, this copy's path, and
/// the last write's failure.
pub(crate) struct Association {
    /// Shared with the worker that reads the status (sweep M4b): the
    /// interface never waits on the registry.
    hive: Arc<Mutex<Box<dyn Hive>>>,
    pub(crate) exe: PathBuf,
    /// The last write's failure, shown on the row until the next try.
    pub(crate) error: Option<String>,
    /// The registry is real: Default apps can be opened.
    system: bool,
    /// Times Default apps was asked for (tests read it).
    pub(crate) default_apps_asked: usize,
}

impl Association {
    pub(crate) fn new(source: Source) -> Self {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("lumen.exe"));
        let system = matches!(source, Source::System) && cfg!(windows);
        let hive: Box<dyn Hive> = match source {
            #[cfg(windows)]
            Source::System => Box::new(registry::Registry),
            #[cfg(not(windows))]
            Source::System => Box::new(MapHive::default()),
            Source::Isolated => Box::new(MapHive {
                log: true,
                ..MapHive::default()
            }),
        };
        Self {
            system,
            ..Self::with(hive, exe)
        }
    }

    pub(crate) fn with(hive: Box<dyn Hive>, exe: PathBuf) -> Self {
        Self {
            hive: Arc::new(Mutex::new(hive)),
            exe,
            error: None,
            system: false,
            default_apps_asked: 0,
        }
    }

    /// The hive, for reading the status on a worker (sweep M4b): drawing
    /// reads the answer instead (`app::association::Reported`).
    pub(crate) fn hive(&self) -> Arc<Mutex<Box<dyn Hive>>> {
        Arc::clone(&self.hive)
    }

    /// Opens the system's Default apps page, where the person's own
    /// choice is changed.
    pub(crate) fn open_default_apps(&mut self) {
        self.default_apps_asked += 1;
        if self.system {
            let opened = std::process::Command::new("explorer.exe")
                .arg("ms-settings:defaultapps")
                .spawn();
            if let Err(e) = opened {
                crate::trace::log(|| format!("association: Default apps did not open ({e})"));
            }
        }
    }

    /// Whether this is the system's registry (a normal start).
    pub(crate) fn is_system(&self) -> bool {
        self.system
    }

    /// Whether this system's file manager is set up here (Windows); other
    /// systems get their file types from their packages.
    pub(crate) fn supported() -> bool {
        cfg!(windows)
    }

    /// Who opens `.lumen` files, read now. Never call this while drawing
    /// (sweep M4b): the app draws the answer a worker brought
    /// (`app::association::Reported`). A write, or a test seeding the hive,
    /// reads it here.
    pub(crate) fn status(&self) -> Status {
        match self.hive.lock() {
            Ok(hive) => status(hive.as_ref(), &self.exe),
            Err(_) => unreadable(),
        }
    }

    /// Registers this copy (`on`) or removes its registration; a failure
    /// is kept for the row.
    pub(crate) fn set(&mut self, on: bool) {
        let exe = self.exe.clone();
        let done = match self.hive.lock() {
            Ok(mut hive) if on => register(hive.as_mut(), &exe),
            Ok(mut hive) => unregister(hive.as_mut(), &exe),
            Err(_) => Err(io::Error::other("the association is unavailable")),
        };
        self.error = done.err().map(|e| {
            crate::trace::log(|| format!("association: {e}"));
            format!("Windows did not take the change: {e}.")
        });
    }

    /// Tests: a fresh map hive, made ready by `seed`.
    pub(crate) fn seed(&mut self, seed: impl FnOnce(&mut MapHive)) {
        let mut hive = MapHive::default();
        seed(&mut hive);
        if let Ok(mut slot) = self.hive.lock() {
            *slot = Box::new(hive);
        }
        self.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Apps\Lumen\lumen.exe")
    }

    #[test]
    fn registering_writes_the_type_its_icon_and_command() {
        let mut hive = MapHive::default();
        register(&mut hive, &exe()).unwrap();
        let prog = prog_key();
        assert_eq!(hive.get(EXT, "").as_deref(), Some("Lumen.Project"));
        assert_eq!(
            hive.get(&format!(r"{EXT}\OpenWithProgids"), "Lumen.Project")
                .as_deref(),
            Some("")
        );
        assert_eq!(hive.get(&prog, "").as_deref(), Some("Lumen project"));
        assert_eq!(
            hive.get(&format!(r"{prog}\DefaultIcon"), "").as_deref(),
            Some(r#""C:\Apps\Lumen\lumen.exe",-2"#)
        );
        assert_eq!(
            hive.get(&format!(r"{prog}\shell\open\command"), "")
                .as_deref(),
            Some(r#""C:\Apps\Lumen\lumen.exe" "%1""#)
        );
        assert!(
            hive.get(
                r"Software\Classes\Applications\lumen.exe\SupportedTypes",
                ".lumen"
            )
            .is_some()
        );
        assert_eq!(status(&hive, &exe()).state, State::This);
    }

    #[test]
    fn a_write_that_fails_part_way_puts_everything_back() {
        // Another copy owns the association; taking it over fails at each
        // write in turn, and each time the hive is as it was.
        let other = PathBuf::from(r"D:\Old\lumen.exe");
        for after in 0..7 {
            let mut hive = MapHive::default();
            register(&mut hive, &other).unwrap();
            hive.set(&format!(r"{EXT}\OpenWithProgids"), "Other.Thing", "")
                .unwrap();
            let before = hive.values.clone();
            hive.fail_after = Some(after);
            assert!(register(&mut hive, &exe()).is_err(), "write {after} fails");
            hive.fail_after = None;
            assert_eq!(hive.values, before, "put back after write {after}");
            assert_eq!(status(&hive, &exe()).state, State::Other(other.clone()));
        }
    }

    #[test]
    fn a_removal_that_fails_part_way_can_be_finished_by_trying_again() {
        for after in 0..4 {
            let mut hive = MapHive::default();
            hive.set(&format!(r"{EXT}\OpenWithProgids"), "Other.Thing", "")
                .unwrap();
            let before = hive.values.clone();
            register(&mut hive, &exe()).unwrap();
            hive.fail_after = Some(after);
            assert!(
                unregister(&mut hive, &exe()).is_err(),
                "removal {after} fails"
            );
            assert_eq!(status(&hive, &exe()).state, State::This, "still ours");
            unregister(&mut hive, &exe()).unwrap();
            assert_eq!(hive.values, before, "finished after removal {after}");
        }
    }

    #[test]
    fn turning_it_off_removes_only_this_copys_values() {
        let mut hive = MapHive::default();
        // Another program also opens .lumen files.
        hive.set(&format!(r"{EXT}\OpenWithProgids"), "Other.Thing", "")
            .unwrap();
        hive.set(r"Software\Classes\Other.Thing", "", "Other")
            .unwrap();
        register(&mut hive, &exe()).unwrap();
        unregister(&mut hive, &exe()).unwrap();
        assert_eq!(status(&hive, &exe()).state, State::Off);
        assert_eq!(hive.get(EXT, ""), None);
        assert_eq!(
            hive.get(&format!(r"{EXT}\OpenWithProgids"), "Lumen.Project"),
            None
        );
        assert!(
            hive.get(&format!(r"{EXT}\OpenWithProgids"), "Other.Thing")
                .is_some()
        );
        assert!(hive.get(r"Software\Classes\Other.Thing", "").is_some());
        assert!(
            hive.get(
                r"Software\Classes\Applications\lumen.exe\SupportedTypes",
                ".lumen"
            )
            .is_none()
        );
    }

    #[test]
    fn another_programs_default_is_kept() {
        let mut hive = MapHive::default();
        register(&mut hive, &exe()).unwrap();
        hive.set(EXT, "", "Other.Thing").unwrap();
        unregister(&mut hive, &exe()).unwrap();
        assert_eq!(hive.get(EXT, "").as_deref(), Some("Other.Thing"));
    }

    #[test]
    fn another_copy_is_named_and_left_alone() {
        let mut hive = MapHive::default();
        let other = PathBuf::from(r"D:\Old\lumen.exe");
        register(&mut hive, &other).unwrap();
        assert_eq!(status(&hive, &exe()).state, State::Other(other.clone()));
        unregister(&mut hive, &exe()).unwrap();
        assert_eq!(
            status(&hive, &other).state,
            State::This,
            "not ours to remove"
        );
        // "Use this copy".
        register(&mut hive, &exe()).unwrap();
        assert_eq!(status(&hive, &exe()).state, State::This);
    }

    #[test]
    fn the_persons_own_choice_is_reported_and_never_written() {
        let mut hive = MapHive::default();
        hive.set(USER_CHOICE, "ProgId", "Other.Thing").unwrap();
        register(&mut hive, &exe()).unwrap();
        let s = status(&hive, &exe());
        assert_eq!(s.chosen_elsewhere.as_deref(), Some("Other.Thing"));
        assert_eq!(
            hive.get(USER_CHOICE, "ProgId").as_deref(),
            Some("Other.Thing")
        );

        hive.set(USER_CHOICE, "ProgId", "Lumen.Project").unwrap();
        unregister(&mut hive, &exe()).unwrap();
        let s = status(&hive, &exe());
        assert!(s.will_ask && s.chosen_elsewhere.is_none());
    }

    #[test]
    fn a_failed_write_is_kept_for_the_row() {
        let mut a = Association::with(
            Box::new(MapHive {
                fail: Some("access is denied".into()),
                ..MapHive::default()
            }),
            exe(),
        );
        a.set(true);
        assert!(a.error.as_deref().unwrap().contains("access is denied"));
        assert_eq!(a.status().state, State::Off);
    }

    #[test]
    fn open_commands_name_their_program() {
        assert_eq!(
            command_exe(r#""C:\A B\lumen.exe" "%1""#),
            Some(PathBuf::from(r"C:\A B\lumen.exe"))
        );
        assert_eq!(
            command_exe(r"C:\lumen.exe %1"),
            Some(PathBuf::from(r"C:\lumen.exe"))
        );
        assert_eq!(command_exe("  "), None);
    }
}
