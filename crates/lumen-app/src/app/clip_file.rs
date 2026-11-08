//! The file an audio clip plays (M9.5 S3, §10.5): whether it is there, and
//! the two things the audio editor's File chip does with it. Reveal shows it
//! in the file manager, by the one function the Project media tab's Reveal
//! uses. Relink makes the answer visible (the browser, the Project media tab,
//! the file's row) and then asks which file stands in for it, as that tab's
//! own Locate does.

use std::path::Path;

use eframe::egui;
use lumen_project::model::{ClipContent, Id};

use crate::App;
use crate::browser::Tab;
use crate::media::State;

/// How an audio clip's file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClipFile {
    /// Loaded.
    Ready,
    /// Still being read.
    Reading,
    /// Read, and could not be used: the reason.
    Failed(String),
    /// Not on this machine.
    Missing,
}

impl ClipFile {
    /// The file can be shown in the file manager.
    pub(crate) fn is_there(&self) -> bool {
        !matches!(self, Self::Missing)
    }
}

impl App {
    /// How the file of media `media` is.
    pub(crate) fn clip_file_state(&self, media: Id) -> ClipFile {
        if self.media_pool.is_absent(media) {
            return ClipFile::Missing;
        }
        match self.lanes.library.get(media.0) {
            None => ClipFile::Missing,
            Some(item) => match &item.state {
                State::Ready(_) => ClipFile::Ready,
                State::Loading => ClipFile::Reading,
                State::Failed(why) => ClipFile::Failed(why.clone()),
            },
        }
    }

    /// Shows `path` in the system's file manager; a test only notes it
    /// (`MediaPool::revealed`), as it does for the templates folder.
    pub(crate) fn show_in_file_manager(&mut self, path: &Path) {
        if self.templates.reveals {
            crate::crash::reveal(path);
        }
        self.media_pool.revealed.push(path.to_path_buf());
    }

    /// The media of the first selected audio clip.
    fn first_selected_media(&self) -> Option<Id> {
        self.document
            .project()
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| self.arrangement.clips.contains(&c.id))
            .find_map(|c| match &c.content {
                ClipContent::Audio { media, .. } => Some(*media),
                ClipContent::Midi { .. } => None,
            })
    }

    /// Why the selected clip's file cannot be shown, if it cannot.
    pub(crate) fn reveal_clip_file_unavailable(&self) -> Result<(), &'static str> {
        let Some(media) = self.first_selected_media() else {
            return Err("Select an audio clip first");
        };
        match self.clip_file_state(media) {
            ClipFile::Missing => Err("The file is missing. Relink it first"),
            _ => Ok(()),
        }
    }

    /// Show the clip's file: the first selected audio clip's.
    pub(crate) fn reveal_clip_file(&mut self) {
        if let Some(media) = self.first_selected_media() {
            self.reveal_media(media);
        }
    }

    /// Shows the file of media `media` in the file manager.
    pub(crate) fn reveal_media(&mut self, media: Id) {
        if let Some(path) = self.lanes.library.get(media.0).map(|i| i.path.clone()) {
            self.show_in_file_manager(&path);
        }
    }

    /// Relink from the editor: shows the Project media tab with the file's
    /// row selected, then asks for the file, as that tab's own button does.
    /// The answer is said as a toast as well, since a popped-out editor can
    /// stand on another monitor.
    pub(crate) fn relink_clip_file(&mut self, ctx: &egui::Context, media: Id) {
        self.show_browser_tab(Tab::Media);
        self.media_pool.selected = Some(media);
        self.locate_media(ctx, media);
        self.media_pool.say_answer = self.media_pool.is_locating(media);
    }
}
