//! The cache of converted audio and peaks in the app (M9 R7): where it is,
//! how much it holds, its limit and Clear. The folder is settled at start; a
//! folder chosen in Preferences is used from the next start, inside a
//! "Lumen cache" folder of its own, and the folder left behind is offered for
//! Clear once (never cleared unasked). Counting and clearing run on a worker,
//! since a cache can hold thousands of files. What may be cleared is the
//! library's rule (`lumen_media::stream::cache`): only the cache's own files,
//! in a folder it marked.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use eframe::egui;
use lumen_media::stream::cache::Cache;

use crate::App;
use crate::app::projects::FileDialog;
use crate::files::describe_size;

/// The folder Lumen keeps inside a folder the user chose.
pub(crate) const CACHE_FOLDER: &str = "Lumen cache";

/// The cache folder as Preferences shows it between frames.
#[derive(Default)]
pub(crate) struct CacheFolder {
    /// The folder in use this run.
    pub dir: PathBuf,
    /// Bytes the cache holds, once counted; `None` until then.
    pub size: Option<u64>,
    /// Bytes the folder left behind holds, once counted.
    pub previous_size: Option<u64>,
    work: Option<Receiver<Done>>,
}

impl CacheFolder {
    /// A count or a clear is under way.
    pub fn busy(&self) -> bool {
        self.work.is_some()
    }
}

/// What the worker did.
enum Done {
    /// The sizes of the cache in use and of the folder left behind.
    Counted { size: u64, previous: Option<u64> },
    /// A clear (or a trim to a lower limit) finished, with the bytes it
    /// cleared: in the folder left behind when `previous`.
    Cleared {
        bytes: u64,
        previous: bool,
        trim: bool,
    },
}

/// The cache folder for the setting `chosen`: a "Lumen cache" folder inside
/// it, or `standard` when nothing is chosen.
pub(crate) fn cache_dir_for(chosen: Option<&str>, standard: PathBuf) -> PathBuf {
    match chosen {
        Some(folder) => Path::new(folder).join(CACHE_FOLDER),
        None => standard,
    }
}

impl App {
    /// Where converted audio is kept this run.
    pub(crate) fn cache_dir(&self) -> PathBuf {
        self.cache.dir.clone()
    }

    /// The standard cache folder: beside the autosaves (the user's
    /// `Lumen\Cache`, the owner's C A), or the system's temporary folder.
    pub(crate) fn standard_cache_dir(&self) -> PathBuf {
        self.autosave
            .as_ref()
            .and_then(|a| a.dir())
            .and_then(|dir| dir.parent())
            .map_or_else(
                || std::env::temp_dir().join("lumen-cache"),
                |dir| dir.join("Cache"),
            )
    }

    /// The cache folder the settings ask for, used from the next start.
    pub(crate) fn chosen_cache_dir(&self) -> PathBuf {
        cache_dir_for(
            self.settings.cache.folder.as_deref(),
            self.standard_cache_dir(),
        )
    }

    /// The folder left behind by a change of folder, while it is not the
    /// one in use.
    pub(crate) fn previous_cache_dir(&self) -> Option<PathBuf> {
        let previous = PathBuf::from(self.settings.cache.previous.as_ref()?);
        (previous != self.cache.dir).then_some(previous)
    }

    /// Counts the cache on a worker, when the Files page first shows it.
    pub(crate) fn count_cache(&mut self, ctx: &egui::Context) {
        if self.cache.busy() {
            return;
        }
        let dir = self.cache.dir.clone();
        let previous = self.previous_cache_dir();
        self.cache_work(ctx, move || Done::Counted {
            size: Cache::new(dir).size(),
            previous: previous.map(|p| Cache::new(p).size()),
        });
    }

    /// Clears the cache in use (`previous` false) or the folder left behind,
    /// on a worker. What plays now is kept.
    pub(crate) fn clear_cache(&mut self, ctx: &egui::Context, previous: bool) {
        if self.cache.busy() {
            return;
        }
        let dir = if previous {
            match self.previous_cache_dir() {
                Some(dir) => dir,
                None => return,
            }
        } else {
            self.cache.dir.clone()
        };
        self.cache_work(ctx, move || Done::Cleared {
            bytes: Cache::new(dir).clear(&crate::media::streams::in_use()),
            previous,
            trim: false,
        });
    }

    /// The Cache limit preference changed: the streaming cache takes it, and
    /// a lower limit trims the folder now, on a worker.
    pub(crate) fn cache_limit_chosen(&mut self, ctx: &egui::Context, gb: u32) {
        if gb == self.settings.cache.limit_gb {
            return;
        }
        let lower = gb < self.settings.cache.limit_gb;
        self.settings.cache.limit_gb = gb;
        self.settings_changed = Some(Instant::now());
        let limit = self.settings.cache.limit();
        if let Some(streaming) = self.lanes.library.streaming() {
            streaming.set_cache_limit(limit);
        }
        if lower && !self.cache.busy() {
            let dir = self.cache.dir.clone();
            self.cache_work(ctx, move || {
                let cache = Cache::new(dir);
                cache.set_limit(limit);
                Done::Cleared {
                    bytes: cache.trim_to_limit(&crate::media::streams::in_use()),
                    previous: false,
                    trim: true,
                }
            });
        }
    }

    /// The folder Preferences asks for when Choose… is pressed.
    pub(crate) fn pick_cache_folder(&mut self, ctx: &egui::Context) {
        let owner = self.owner;
        let start = self
            .settings
            .cache
            .folder
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| self.cache.dir.parent().map(Path::to_path_buf));
        let show = move || {
            let mut dialog = rfd::FileDialog::new().set_title("Choose where the cache is kept");
            if let Some(dir) = start {
                dialog = dialog.set_directory(dir);
            }
            if let Some(owner) = &owner {
                dialog = dialog.set_parent(owner);
            }
            FileDialog::CacheFolder(dialog.pick_folder())
        };
        self.file_dialog(ctx, show, |paths| {
            FileDialog::CacheFolder(paths.into_iter().next())
        });
    }

    /// A cache folder was chosen (`None`: the standard one). It is used from
    /// the next start; the folder in use now is offered for Clear once the
    /// new one is in use. Nothing is copied.
    pub(crate) fn cache_folder_chosen(&mut self, folder: Option<PathBuf>) {
        let folder = folder.map(|f| f.to_string_lossy().into_owned());
        if folder == self.settings.cache.folder {
            return;
        }
        self.settings.cache.folder = folder;
        let next = self.chosen_cache_dir();
        self.settings.cache.previous =
            (next != self.cache.dir).then(|| self.cache.dir.to_string_lossy().into_owned());
        self.settings_changed = Some(Instant::now());
        self.toast = Some((
            if next == self.cache.dir {
                "The cache stays where it is".to_owned()
            } else {
                format!(
                    "The cache moves to {} when {} next starts. Nothing is copied; files \
                     are converted again as they are needed",
                    next.display(),
                    lumen_core::product::PRODUCT_NAME
                )
            },
            Instant::now(),
        ));
    }

    /// Keeps the folder left behind as it is, and stops offering it.
    pub(crate) fn keep_previous_cache(&mut self) {
        self.settings.cache.previous = None;
        self.cache.previous_size = None;
        self.settings_changed = Some(Instant::now());
    }

    fn cache_work(&mut self, ctx: &egui::Context, work: impl FnOnce() -> Done + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
        self.cache.work = Some(rx);
    }

    /// Per frame while the worker runs: takes its answer.
    pub(crate) fn poll_cache(&mut self, ctx: &egui::Context) {
        let Some(done) = self.cache.work.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.cache.work = None;
        match done {
            Done::Counted { size, previous } => {
                self.cache.size = Some(size);
                self.cache.previous_size = previous;
            }
            Done::Cleared {
                bytes,
                previous,
                trim,
            } => {
                let note = if trim {
                    (bytes > 0).then(|| {
                        format!(
                            "Cleared {} of the cache to keep within its limit",
                            describe_size(bytes)
                        )
                    })
                } else if bytes == 0 {
                    Some("The cache had nothing to clear".to_owned())
                } else {
                    Some(format!("Cleared {} from the cache", describe_size(bytes)))
                };
                if let Some(note) = note {
                    self.toast = Some((note, Instant::now()));
                }
                if previous {
                    // Offered once: cleared, it is not offered again.
                    self.keep_previous_cache();
                }
                self.count_cache(ctx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chosen_folder_gets_a_cache_folder_of_its_own() {
        let standard = PathBuf::from("/home/me/.local/state/lumen/Cache");
        assert_eq!(cache_dir_for(None, standard.clone()), standard);
        assert_eq!(
            cache_dir_for(Some("/scratch"), standard),
            Path::new("/scratch").join(CACHE_FOLDER)
        );
    }
}
