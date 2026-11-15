use std::path::Path;

use eframe::egui;
use lumen_project::model::{ClipContent, Id};

use crate::App;
use crate::browser::Tab;
use crate::media::State;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClipFile {
    Ready,
    Reading,
    Failed(String),
    Missing,
}

impl ClipFile {
    pub(crate) fn is_there(&self) -> bool {
        !matches!(self, Self::Missing)
    }
}

impl App {
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

    pub(crate) fn show_in_file_manager(&mut self, path: &Path) {
        if self.templates.reveals {
            crate::crash::reveal(path);
        }
        self.media_pool.revealed.push(path.to_path_buf());
    }

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

    pub(crate) fn reveal_clip_file_unavailable(&self) -> Result<(), &'static str> {
        let Some(media) = self.first_selected_media() else {
            return Err("Select an audio clip first");
        };
        match self.clip_file_state(media) {
            ClipFile::Missing => Err("The file is missing. Relink it first"),
            _ => Ok(()),
        }
    }

    pub(crate) fn reveal_clip_file(&mut self) {
        if let Some(media) = self.first_selected_media() {
            self.reveal_media(media);
        }
    }

    pub(crate) fn reveal_media(&mut self, media: Id) {
        if let Some(path) = self.lanes.library.get(media.0).map(|i| i.path.clone()) {
            self.show_in_file_manager(&path);
        }
    }

    pub(crate) fn relink_clip_file(&mut self, ctx: &egui::Context, media: Id) {
        self.show_browser_tab(Tab::Media);
        self.media_pool.selected = Some(media);
        self.locate_media(ctx, media);
        self.media_pool.say_answer = self.media_pool.is_locating(media);
    }
}
