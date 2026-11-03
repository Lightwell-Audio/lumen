//! The browser's disk workers (M9 R7c, ADR-0116): two threads, apart from
//! R6's read-ahead, taking the open folder first, then walks (Samples,
//! Projects, search), then project details, then row pictures (the newest
//! asked first). Answers come back in batches of at most
//! [`walk::BATCH`] rows through a bounded channel, each with the ticket of
//! its request, so the browser drops a stale one. The UI never waits on a
//! worker: a cancelled request is abandoned and its late answers ignored,
//! and the threads are never joined (one blocked on a network drive that
//! hangs costs that thread until it returns; the next request waits behind
//! it on the other).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};

use eframe::egui;
use lumen_media::stream::cache::{self, Cache, Small};

use super::thumbs::{self, Thumb};
use super::walk::{self, Entry, Index, Outcome, Wanted};

/// Worker threads.
const THREADS: usize = 2;
/// Answers waiting for the UI, at most: a worker waits when it is full.
const ANSWERS: usize = 64;
/// How long a worker waits for room in a full answer channel: a worker's
/// poll, not a design timing (no `motion` token applies).
const FULL_WAIT: std::time::Duration = std::time::Duration::from_micros(5_000);
/// Row pictures waiting, at most: the oldest asked give way.
const PICTURES: usize = 128;

/// What a worker is asked to do.
#[derive(Debug, Clone)]
pub(crate) enum Job {
    /// One folder's rows (Places).
    Folder { dir: PathBuf, wanted: Wanted },
    /// Every file under some folders (Samples, Projects, search).
    Walk { roots: Vec<PathBuf>, wanted: Wanted },
    /// A project's tempo, metre and tracks.
    Project(PathBuf),
    /// A row's picture.
    Picture(PathBuf),
    /// Which of these folders are there (Places' top level).
    Present(Vec<PathBuf>),
    /// What Computer lists.
    Computer,
}

/// What came back.
#[derive(Debug)]
pub(crate) enum Answer {
    Rows {
        ticket: u64,
        rows: Vec<Entry>,
    },
    /// A folder or walk ended; `error` when the folder could not be read.
    Done {
        ticket: u64,
        outcome: Outcome,
        error: Option<String>,
    },
    Picture {
        ticket: u64,
        path: PathBuf,
        thumb: Result<Thumb, String>,
    },
    Project {
        ticket: u64,
        path: PathBuf,
        about: Result<ProjectAbout, String>,
    },
    Present {
        ticket: u64,
        present: Vec<(PathBuf, bool)>,
    },
}

/// What the Projects strip says of a project.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProjectAbout {
    pub bpm: f64,
    pub meter: String,
    pub tracks: usize,
}

struct Request {
    ticket: u64,
    job: Job,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct Queues {
    /// Folders, walks and projects, in that order of priority.
    urgent: VecDeque<Request>,
    walks: VecDeque<Request>,
    projects: VecDeque<Request>,
    /// Newest at the back, taken from the back.
    pictures: VecDeque<Request>,
    /// Pictures that gave way to newer ones before a worker took them: the
    /// browser forgets it asked, so a row back in view asks again.
    dropped: Vec<PathBuf>,
    closed: bool,
}

impl Queues {
    /// Queues a picture; the oldest asked give way past [`PICTURES`].
    fn push_picture(&mut self, request: Request) {
        self.pictures.push_back(request);
        while self.pictures.len() > PICTURES {
            if let Some(Request {
                job: Job::Picture(path),
                ..
            }) = self.pictures.pop_front()
            {
                self.dropped.push(path);
            }
        }
    }

    fn next(&mut self) -> Option<Request> {
        self.urgent
            .pop_front()
            .or_else(|| self.walks.pop_front())
            .or_else(|| self.projects.pop_front())
            .or_else(|| self.pictures.pop_back())
    }
}

type Shared = Arc<(Mutex<Queues>, Condvar)>;

/// The pool, as the UI holds it.
pub(crate) struct Disk {
    shared: Shared,
    answers: Receiver<Answer>,
    next: u64,
}

impl Disk {
    /// Starts the workers. `cache` is the cache folder, for indexes and
    /// row pictures (`None`: nothing is kept).
    pub(crate) fn start(cache: Option<PathBuf>, ctx: &egui::Context) -> Self {
        let shared: Shared = Arc::default();
        let (tx, answers) = sync_channel(ANSWERS);
        let cache = cache.map(|dir| Arc::new(Cache::new(dir)));
        for n in 0..THREADS {
            let (shared, tx, ctx, cache) =
                (Arc::clone(&shared), tx.clone(), ctx.clone(), cache.clone());
            let spawned = std::thread::Builder::new()
                .name(format!("lumen-browse-{n}"))
                .spawn(move || work(&shared, &tx, &ctx, cache.as_deref()));
            if spawned.is_err() {
                crate::trace::log(|| "browser: a disk worker did not start".into());
            }
        }
        Self {
            shared,
            answers,
            next: 0,
        }
    }

    /// Asks for `job`; returns its ticket and the flag that cancels it.
    pub(crate) fn ask(&mut self, job: Job) -> (u64, Arc<AtomicBool>) {
        self.next += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        let request = Request {
            ticket: self.next,
            job,
            cancel: Arc::clone(&cancel),
        };
        let (lock, wake) = &*self.shared;
        let mut queues = lock.lock().unwrap_or_else(|e| e.into_inner());
        match request.job {
            Job::Folder { .. } | Job::Computer | Job::Present(_) => {
                queues.urgent.push_back(request)
            }
            Job::Walk { .. } => queues.walks.push_back(request),
            Job::Project(_) => queues.projects.push_back(request),
            Job::Picture(_) => queues.push_picture(request),
        }
        drop(queues);
        wake.notify_one();
        (self.next, cancel)
    }

    /// The ticket of the last request asked.
    pub(crate) fn last_ticket(&self) -> u64 {
        self.next
    }

    /// Forgets the row pictures not started yet (a tab was left).
    pub(crate) fn forget_pictures(&self) {
        let (lock, _) = &*self.shared;
        let mut queues = lock.lock().unwrap_or_else(|e| e.into_inner());
        queues.pictures.clear();
        queues.dropped.clear();
    }

    /// The pictures that gave way before they were made (see
    /// [`Queues::dropped`]).
    pub(crate) fn dropped_pictures(&self) -> Vec<PathBuf> {
        let (lock, _) = &*self.shared;
        std::mem::take(&mut lock.lock().unwrap_or_else(|e| e.into_inner()).dropped)
    }

    /// The answers that came since the last call (at most a channel's
    /// worth, so a flood is spread over frames).
    pub(crate) fn answers(&self) -> Vec<Answer> {
        self.answers.try_iter().take(ANSWERS).collect()
    }
}

impl Drop for Disk {
    fn drop(&mut self) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        wake.notify_all();
    }
}

fn work(shared: &Shared, tx: &SyncSender<Answer>, ctx: &egui::Context, cache: Option<&Cache>) {
    loop {
        let request = {
            let (lock, wake) = &**shared;
            let mut queues = lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if queues.closed {
                    return;
                }
                if let Some(request) = queues.next() {
                    break request;
                }
                queues = wake.wait(queues).unwrap_or_else(|e| e.into_inner());
            }
        };
        if request.cancel.load(Ordering::Relaxed) {
            continue;
        }
        let sent = |answer: Answer| -> bool {
            let ok = send(tx, answer);
            ctx.request_repaint();
            ok
        };
        if !run(&request, cache, &sent) {
            // The browser is gone.
            return;
        }
    }
}

/// Sends `answer`, waiting while the channel is full; false once the UI
/// has gone.
fn send(tx: &SyncSender<Answer>, answer: Answer) -> bool {
    let mut answer = answer;
    loop {
        match tx.try_send(answer) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(back)) => {
                answer = back;
                std::thread::sleep(FULL_WAIT);
            }
        }
    }
}

/// Does one request. False once the UI has gone.
fn run(request: &Request, cache: Option<&Cache>, sent: &dyn Fn(Answer) -> bool) -> bool {
    let ticket = request.ticket;
    let cancel = &*request.cancel;
    let mut alive = true;
    match &request.job {
        Job::Folder { dir, wanted } => {
            let result = walk::read_folder(dir, *wanted, cancel, &mut |rows| {
                alive &= sent(Answer::Rows { ticket, rows });
            });
            let (outcome, error) = match result {
                Ok(outcome) => (outcome, None),
                Err(error) => (Outcome::Done, Some(error)),
            };
            alive
                && sent(Answer::Done {
                    ticket,
                    outcome,
                    error,
                })
        }
        Job::Walk { roots, wanted } => {
            let mut outcome = Outcome::Done;
            let mut errors = Vec::new();
            let mut found = 0;
            for root in roots {
                if !root.is_dir() {
                    errors.push(format!("{} is not there.", root.display()));
                    continue;
                }
                let key = index_key(root, *wanted);
                let old = cache
                    .and_then(|c| c.load(Small::Index, &key, 1))
                    .and_then(|bytes| Index::from_bytes(&bytes, root));
                let walked = walk::walk(
                    root,
                    *wanted,
                    old.as_ref(),
                    walk::MOST - found,
                    cancel,
                    &mut |rows| {
                        alive &= sent(Answer::Rows { ticket, rows });
                    },
                );
                found += walked.found;
                if let Some(first) = walked.first_error {
                    errors.push(unreadable(walked.unreadable, &first));
                }
                if let Some(cache) = cache {
                    let _ = cache.store(Small::Index, &key, 1, &walked.index.to_bytes());
                }
                if walked.outcome != Outcome::Done {
                    outcome = walked.outcome;
                }
                if outcome != Outcome::Done || !alive {
                    break;
                }
            }
            alive
                && sent(Answer::Done {
                    ticket,
                    outcome,
                    error: (!errors.is_empty()).then(|| errors.join(" ")),
                })
        }
        Job::Present(paths) => sent(Answer::Present {
            ticket,
            present: paths.iter().map(|p| (p.clone(), p.is_dir())).collect(),
        }),
        Job::Computer => {
            let rows = walk::computer()
                .into_iter()
                .map(|(name, path)| Entry::named(path, name, walk::Kind::Folder))
                .collect();
            sent(Answer::Rows { ticket, rows })
                && sent(Answer::Done {
                    ticket,
                    outcome: Outcome::Done,
                    error: None,
                })
        }
        Job::Project(path) => sent(Answer::Project {
            ticket,
            path: path.clone(),
            about: about(path),
        }),
        Job::Picture(path) => sent(Answer::Picture {
            ticket,
            path: path.clone(),
            thumb: thumbs::thumb(path, cache, cancel),
        }),
    }
}

/// What the browser says of folders a walk could not read.
fn unreadable(count: usize, first: &str) -> String {
    if count == 1 {
        format!("A folder could not be read, so its files are not listed: {first}.")
    } else {
        format!(
            "{count} folders could not be read, so their files are not listed. The first: {first}."
        )
    }
}

/// The index's name in the cache: the folder and what was looked for.
fn index_key(root: &Path, wanted: Wanted) -> String {
    cache::hash(&format!(
        "lumen browse index\n{}\n{wanted:?}\n",
        root.display()
    ))
}

/// What a project file says of itself.
fn about(path: &Path) -> Result<ProjectAbout, String> {
    let project = lumen_project::load(path).map_err(|e| e.to_string())?;
    let meter = project.meter.first().map_or_else(
        || "4/4".to_owned(),
        |m| format!("{}/{}", m.meter.numerator, m.meter.denominator),
    );
    Ok(ProjectAbout {
        bpm: project.tempo.first().map_or(120.0, |t| t.bpm),
        meter,
        tracks: project.tracks.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(n: usize) -> Request {
        Request {
            ticket: n as u64,
            job: Job::Picture(PathBuf::from(format!("{n}.wav"))),
            cancel: Arc::default(),
        }
    }

    #[test]
    fn a_picture_that_gives_way_is_handed_back_and_the_newest_go_first() {
        let mut queues = Queues::default();
        for n in 0..PICTURES + 2 {
            queues.push_picture(picture(n));
        }
        assert_eq!(queues.pictures.len(), PICTURES);
        assert_eq!(
            queues.dropped,
            [PathBuf::from("0.wav"), PathBuf::from("1.wav")]
        );
        let next = queues.next().expect("a picture waits");
        assert!(
            matches!(next.job, Job::Picture(p) if p == Path::new(&format!("{}.wav", PICTURES + 1)))
        );
    }
}
