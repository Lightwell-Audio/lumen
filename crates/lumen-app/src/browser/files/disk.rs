use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};

use eframe::egui;
use lumen_media::stream::cache::{self, Cache, Small};

use super::thumbs::{self, Thumb};
use super::walk::{self, Entry, Index, Outcome, Wanted};

const THREADS: usize = 2;
const ANSWERS: usize = 64;
const FULL_WAIT: std::time::Duration = std::time::Duration::from_micros(5_000);
const PICTURES: usize = 128;

#[derive(Debug, Clone)]
pub(crate) enum Job {
    Folder { dir: PathBuf, wanted: Wanted },
    Walk { roots: Vec<PathBuf>, wanted: Wanted },
    Project(PathBuf),
    Picture(PathBuf),
    Present(Vec<PathBuf>),
    Computer,
}

#[derive(Debug)]
pub(crate) enum Answer {
    Rows {
        ticket: u64,
        rows: Vec<Entry>,
    },
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
    urgent: VecDeque<Request>,
    walks: VecDeque<Request>,
    projects: VecDeque<Request>,
    pictures: VecDeque<Request>,
    dropped: Vec<PathBuf>,
    closed: bool,
}

impl Queues {
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

pub(crate) struct Disk {
    shared: Shared,
    answers: Receiver<Answer>,
    next: u64,
}

impl Disk {
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

    pub(crate) fn last_ticket(&self) -> u64 {
        self.next
    }

    pub(crate) fn forget_pictures(&self) {
        let (lock, _) = &*self.shared;
        let mut queues = lock.lock().unwrap_or_else(|e| e.into_inner());
        queues.pictures.clear();
        queues.dropped.clear();
    }

    pub(crate) fn dropped_pictures(&self) -> Vec<PathBuf> {
        let (lock, _) = &*self.shared;
        std::mem::take(&mut lock.lock().unwrap_or_else(|e| e.into_inner()).dropped)
    }

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
            return;
        }
    }
}

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

fn unreadable(count: usize, first: &str) -> String {
    if count == 1 {
        format!("A folder could not be read, so its files are not listed: {first}.")
    } else {
        format!(
            "{count} folders could not be read, so their files are not listed. The first: {first}."
        )
    }
}

fn index_key(root: &Path, wanted: Wanted) -> String {
    cache::hash(&format!(
        "lumen browse index\n{}\n{wanted:?}\n",
        root.display()
    ))
}

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
