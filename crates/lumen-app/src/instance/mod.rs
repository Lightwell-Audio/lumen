//! One Lumen per settings file (M9 R8, ADR-0117, owner SI A).
//!
//! A start first takes the instance lock, a file in the settings folder
//! locked with `File::try_lock`, before it writes settings, opens audio or
//! starts autosave. The holder is the instance: the system frees the lock
//! when a process ends, however it ends, so a free lock proves the old one
//! is gone, and two starts at once elect exactly one. The holder then
//! listens on a named pipe (Windows) or a socket (Unix) for later starts.
//!
//! A later start does not touch settings, audio or autosave. It sends its
//! request (open a project, or only come to the front) and exits once the
//! running Lumen's window has taken it. For up to 10 s it alternates
//! between connecting and taking the lock again, which covers a holder
//! still starting and one that closes meanwhile. A holder that does not
//! answer is never replaced: its lock proves it is alive.

mod protocol;
pub(crate) mod start;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

pub(crate) use protocol::{Reply, Request};
pub(crate) use start::start;

#[cfg(unix)]
use unix as transport;
#[cfg(windows)]
use windows as transport;

/// How long a later start keeps trying before it says Lumen is not answering.
pub(crate) const PATIENCE: Duration = Duration::from_secs(10);
/// How long the listener waits for the window to take a request.
const WINDOW_TAKES: Duration = Duration::from_secs(5);
/// How long one read or write on a connection may take.
const IO_TIMEOUT: Duration = Duration::from_secs(2);
/// Requests waiting for the window, at most; more are turned away as busy.
const QUEUE: usize = 8;
/// Between tries of a later start.
const RETRY: Duration = Duration::from_millis(100);

/// Where an instance lives: its lock file and its endpoint's name, both
/// from one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) key: String,
    pub(crate) lock: PathBuf,
    /// The settings folder (the Unix socket's fallback folder).
    pub(crate) folder: PathBuf,
}

impl Place {
    /// The instance for the settings file at `settings`: its folder made
    /// canonical (made first if missing) and its file name, lowercased on
    /// Windows, so equivalent paths give one instance. `None` if the folder
    /// cannot be made.
    pub(crate) fn for_settings(settings: &Path) -> Option<Self> {
        let folder = settings.parent().filter(|p| !p.as_os_str().is_empty());
        let folder = folder.unwrap_or(Path::new("."));
        std::fs::create_dir_all(folder).ok()?;
        let folder = std::fs::canonicalize(folder).ok()?;
        let name = settings.file_name()?.to_string_lossy().into_owned();
        let identity = format!("{}|{name}", folder.display());
        let identity = if cfg!(windows) {
            identity.to_lowercase()
        } else {
            identity
        };
        let key = format!("{:016x}", fnv1a(identity.as_bytes()));
        Some(Self {
            lock: folder.join(format!(
                "{}-{key}.instance",
                lumen_core::product::PRODUCT_NAME
            )),
            key,
            folder,
        })
    }
}

/// FNV-1a, 64 bits: stable across builds and toolchains, unlike std's
/// hasher, so a debug and a release build agree on the key.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// What a start found.
pub(crate) enum Claim {
    /// This start is the instance.
    Owner(Instance),
    /// The running Lumen took the request: this start ends.
    Handed,
    /// The running Lumen refused it, saying why.
    Refused(String),
    /// A Lumen holds the lock but did not answer in time.
    NotAnswering,
}

/// The running instance: its lock, and the requests later starts send.
pub(crate) struct Instance {
    /// Held for the run; a fake instance (tests) has none.
    _lock: Option<File>,
    requests: Option<Receiver<Incoming>>,
    waker: Waker,
    /// A fake instance's way in (tests).
    injector: Option<SyncSender<Incoming>>,
}

/// A request as the window gets it, with the way to answer it.
pub(crate) struct Incoming {
    pub(crate) request: Request,
    answer: SyncSender<Reply>,
}

impl Incoming {
    /// Answers the later start. `false` if it stopped waiting (the window
    /// took too long), in which case the request must be dropped: the
    /// later start already told the person Lumen did not answer.
    pub(crate) fn answer(self, reply: Reply) -> bool {
        self.answer.send(reply).is_ok()
    }
}

/// Wakes the window from the listener's thread once it exists.
#[derive(Clone, Default)]
pub(crate) struct Waker(Arc<Mutex<Option<egui::Context>>>);

impl Waker {
    fn wake(&self) {
        if let Some(ctx) = self.0.lock().ok().and_then(|c| c.clone()) {
            ctx.request_repaint();
        }
    }
}

impl Instance {
    /// Called once the window exists, so requests wake it.
    pub(crate) fn attach(&self, ctx: &egui::Context) {
        if let Ok(mut slot) = self.waker.0.lock() {
            *slot = Some(ctx.clone());
        }
    }

    /// The next request waiting, if any.
    pub(crate) fn next(&self) -> Option<Incoming> {
        self.requests.as_ref()?.try_recv().ok()
    }

    /// An instance with no lock and no endpoint, whose requests come from
    /// [`Self::inject`] (in-process tests).
    pub(crate) fn fake() -> Self {
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        Instance {
            _lock: None,
            requests: Some(rx),
            waker: Waker::default(),
            injector: Some(tx),
        }
    }

    /// Sends `request` as a later start would (a fake instance only); the
    /// answer arrives on the returned receiver.
    pub(crate) fn inject(&self, request: Request) -> Receiver<Reply> {
        let (answer, answered) = mpsc::sync_channel(1);
        if let Some(tx) = &self.injector {
            let _ = tx.try_send(Incoming { request, answer });
        }
        answered
    }
}

/// What a start tries, and for how long (tests shorten it).
pub(crate) struct Attempt<'a> {
    pub(crate) place: &'a Place,
    pub(crate) request: &'a Request,
    pub(crate) patience: Duration,
    /// How the listener answers (tests change it).
    pub(crate) listener: Listener,
}

/// How a holder's listener answers: normally, or as tests need it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Listener {
    #[default]
    Normal,
    /// Takes requests and never answers (`--instance-deaf`).
    Deaf,
    /// Turns every request away as busy (`--instance-busy`).
    Busy,
}

/// Takes the instance, or hands `request` to the one running.
pub(crate) fn claim(attempt: &Attempt<'_>) -> Claim {
    let deadline = Instant::now() + attempt.patience;
    loop {
        if let Some(lock) = take_lock(&attempt.place.lock) {
            return Claim::Owner(listen(attempt.place, lock, attempt.listener));
        }
        // A path the protocol cannot carry would never get through.
        if attempt.request.encode().is_none() {
            return Claim::Refused(format!(
                "The file's path cannot be passed to the running {}. Open it from File › Open.",
                lumen_core::product::PRODUCT_NAME
            ));
        }
        let left = deadline.saturating_duration_since(Instant::now());
        match transport::send(attempt.place, attempt.request, left) {
            Ok(Reply::Accepted) => return Claim::Handed,
            Ok(Reply::Rejected(reason)) => return Claim::Refused(reason),
            Ok(Reply::NotAnswering) | Err(_) => {}
        }
        if Instant::now() >= deadline {
            return Claim::NotAnswering;
        }
        std::thread::sleep(RETRY);
    }
}

/// The lock, if no other process holds it.
fn take_lock(path: &Path) -> Option<File> {
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// Starts the listener for the instance that holds `lock`. If the endpoint
/// cannot be made, the instance runs without one: later starts then say it
/// is not answering, and never start a second engine.
fn listen(place: &Place, lock: File, listener: Listener) -> Instance {
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    let waker = Waker::default();
    let wake = waker.clone();
    let deliver = move |request: Request| deliver(&tx, &wake, request, listener);
    let requests = match transport::serve(place, Box::new(deliver)) {
        Ok(()) => Some(rx),
        Err(e) => {
            crate::trace::log(|| format!("instance: no listener ({e})"));
            None
        }
    };
    Instance {
        _lock: Some(lock),
        requests,
        waker,
        injector: None,
    }
}

/// Hands a request to the window and waits for its answer.
fn deliver(
    tx: &SyncSender<Incoming>,
    waker: &Waker,
    request: Request,
    listener: Listener,
) -> Reply {
    let (answer, answered) = mpsc::sync_channel(1);
    let incoming = Incoming { request, answer };
    let sent = match listener {
        Listener::Normal => tx.try_send(incoming),
        Listener::Deaf => {
            std::thread::sleep(WINDOW_TAKES);
            return Reply::NotAnswering;
        }
        Listener::Busy => Err(TrySendError::Full(incoming)),
    };
    match sent {
        Ok(()) => waker.wake(),
        Err(TrySendError::Full(_)) => {
            return Reply::Rejected(format!(
                "{} is busy with other requests. Try again.",
                lumen_core::product::PRODUCT_NAME
            ));
        }
        Err(TrySendError::Disconnected(_)) => {
            return Reply::Rejected(format!(
                "{} is closing. Try again when it has closed.",
                lumen_core::product::PRODUCT_NAME
            ));
        }
    }
    match answered.recv_timeout(WINDOW_TAKES) {
        Ok(reply) => reply,
        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => Reply::NotAnswering,
    }
}

/// A listener's handler: a request in, a reply out.
pub(crate) type Deliver = Box<dyn Fn(Request) -> Reply + Send>;

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::testing::TempDir;

    fn place(dir: &TempDir, name: &str) -> Place {
        Place::for_settings(&dir.path().join(name)).unwrap()
    }

    #[test]
    fn equivalent_settings_paths_give_one_key_and_two_files_two() {
        let dir = TempDir::new("instance-key");
        let a = place(&dir, "settings.json");
        let b =
            Place::for_settings(&dir.path().join("sub").join("..").join("settings.json")).unwrap();
        assert_eq!(a.key, b.key);
        assert_ne!(a.key, place(&dir, "other.json").key);
        if cfg!(windows) {
            assert_eq!(a.key, place(&dir, "SETTINGS.JSON").key);
        }
    }

    #[test]
    fn a_folder_that_does_not_exist_yet_is_made() {
        let dir = TempDir::new("instance-new-folder");
        let p = Place::for_settings(&dir.path().join("a").join("b").join("s.json")).unwrap();
        assert!(p.folder.is_dir());
        assert!(p.lock.starts_with(&p.folder));
    }

    #[test]
    fn the_key_is_stable() {
        // FNV-1a's published value for "a": builds must agree on keys.
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn racing_starts_elect_exactly_one_and_a_dropped_holder_frees_it() {
        let dir = TempDir::new("instance-race");
        let lock = place(&dir, "settings.json").lock;
        let winners: usize = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let lock = lock.clone();
                    s.spawn(move || {
                        take_lock(&lock)
                            .map(|f| (f, std::thread::sleep(Duration::from_millis(200))))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| usize::from(h.join().unwrap().is_some()))
                .sum()
        });
        assert_eq!(winners, 1);
        // Every holder has been dropped: the lock is free again.
        assert!(take_lock(&lock).is_some());
    }

    fn attempt<'a>(place: &'a Place, request: &'a Request) -> Attempt<'a> {
        Attempt {
            place,
            request,
            patience: Duration::from_secs(5),
            listener: Listener::Normal,
        }
    }

    #[test]
    fn a_second_start_hands_its_file_to_the_first() {
        let dir = TempDir::new("instance-hand");
        let place = place(&dir, "settings.json");
        let Claim::Owner(first) = claim(&attempt(&place, &Request::Show)) else {
            panic!("the first start owns the instance");
        };
        let song = dir.path().join("Night Drive.lumen");
        let request = Request::Open(song.clone());
        let second = std::thread::spawn({
            let place = place.clone();
            move || matches!(claim(&attempt(&place, &request)), Claim::Handed)
        });
        let incoming = wait_for(&first);
        assert_eq!(incoming.request, Request::Open(song));
        assert!(incoming.answer(Reply::Accepted));
        assert!(second.join().unwrap(), "the second start was handed over");
    }

    #[test]
    fn a_refusal_reaches_the_second_start() {
        let dir = TempDir::new("instance-refuse");
        let place = place(&dir, "settings.json");
        let Claim::Owner(first) = claim(&attempt(&place, &Request::Show)) else {
            panic!("owner");
        };
        let second = std::thread::spawn({
            let place = place.clone();
            move || match claim(&attempt(&place, &Request::Show)) {
                Claim::Refused(reason) => reason,
                _ => String::new(),
            }
        });
        wait_for(&first).answer(Reply::Rejected("no".into()));
        assert_eq!(second.join().unwrap(), "no");
    }

    #[test]
    fn a_holder_that_never_answers_is_not_replaced() {
        let dir = TempDir::new("instance-deaf");
        let place = place(&dir, "settings.json");
        let Claim::Owner(_first) = claim(&Attempt {
            listener: Listener::Deaf,
            ..attempt(&place, &Request::Show)
        }) else {
            panic!("owner");
        };
        let started = Instant::now();
        let second = claim(&Attempt {
            patience: Duration::from_secs(1),
            ..attempt(&place, &Request::Show)
        });
        assert!(matches!(second, Claim::NotAnswering));
        assert!(started.elapsed() < Duration::from_secs(8));
    }

    #[test]
    fn a_holder_that_closes_lets_the_next_start_take_over() {
        let dir = TempDir::new("instance-close");
        let place = place(&dir, "settings.json");
        let Claim::Owner(first) = claim(&attempt(&place, &Request::Show)) else {
            panic!("owner");
        };
        drop(first);
        assert!(matches!(
            claim(&attempt(&place, &Request::Show)),
            Claim::Owner(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_stale_socket_left_by_a_killed_holder_is_replaced() {
        let dir = TempDir::new("instance-stale");
        let place = place(&dir, "settings.json");
        // A holder killed outright leaves its socket file behind.
        let stale = transport::socket(&place);
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        drop(std::os::unix::net::UnixListener::bind(&stale).unwrap());
        assert!(stale.exists());
        let Claim::Owner(first) = claim(&attempt(&place, &Request::Show)) else {
            panic!("the lock is free, so this start owns the instance");
        };
        let second = std::thread::spawn({
            let place = place.clone();
            move || matches!(claim(&attempt(&place, &Request::Show)), Claim::Handed)
        });
        assert!(wait_for(&first).answer(Reply::Accepted));
        assert!(second.join().unwrap(), "the new socket answers");
    }

    #[test]
    fn a_full_queue_is_turned_away_as_busy() {
        let (tx, _rx) = mpsc::sync_channel(QUEUE);
        let waker = Waker::default();
        for _ in 0..QUEUE {
            let (answer, _) = mpsc::sync_channel(1);
            tx.try_send(Incoming {
                request: Request::Show,
                answer,
            })
            .unwrap();
        }
        let reply = deliver(&tx, &waker, Request::Show, Listener::Normal);
        assert!(matches!(reply, Reply::Rejected(r) if r.contains("busy")));
    }

    #[test]
    fn an_answer_after_the_start_stopped_waiting_is_dropped() {
        let (answer, answered) = mpsc::sync_channel(1);
        drop(answered);
        let incoming = Incoming {
            request: Request::Show,
            answer,
        };
        assert!(!incoming.answer(Reply::Accepted));
    }

    fn wait_for(instance: &Instance) -> Incoming {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(i) = instance.next() {
                return i;
            }
            assert!(Instant::now() < until, "no request arrived");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
