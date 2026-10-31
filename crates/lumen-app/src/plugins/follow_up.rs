//! A scan asked for while one runs (ADR-0114): adding or removing a folder,
//! Retry on a blocked plugin, or leaving safe mode during a scan. The running
//! scan has already read its folders, so the request waits and runs once
//! that scan ends or is cancelled, with the folders as they are then. Every
//! request made during one scan becomes this one follow-up.

use std::path::PathBuf;

use lumen_plugin_catalog::scan::Mode;

/// The scan that runs when the one under way is over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FollowUp {
    mode: Mode,
    /// Modules to forget before it runs, so they are loaded again (Retry).
    modules: Vec<String>,
    /// The folders it will check that the running scan does not.
    folders: Vec<PathBuf>,
}

impl FollowUp {
    /// Folds a request into what is `waiting`. `unread` is the folders the
    /// settings name now that the running scan does not read: the latest
    /// request's view replaces the earlier one, so a folder added and then
    /// removed during one scan is no longer waiting.
    pub(super) fn ask(
        waiting: Option<Self>,
        mode: Mode,
        module: Option<&str>,
        unread: Vec<PathBuf>,
    ) -> Self {
        let mut next = waiting.unwrap_or(Self {
            mode,
            modules: Vec::new(),
            folders: Vec::new(),
        });
        // Loading every module again includes looking for changed ones.
        if mode == Mode::All {
            next.mode = Mode::All;
        }
        if let Some(module) = module
            && !next.modules.iter().any(|m| m == module)
        {
            next.modules.push(module.to_owned());
        }
        next.folders = unread;
        next
    }

    pub(super) fn mode(&self) -> Mode {
        self.mode
    }

    /// The modules Retry asked for, to be loaded again.
    pub(crate) fn modules(&self) -> &[String] {
        &self.modules
    }

    /// The folders waiting to be checked ("Waiting to check D:\…\VST3").
    pub(crate) fn folders(&self) -> &[PathBuf] {
        &self.folders
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_request_keeps_its_mode_and_folders() {
        let next = FollowUp::ask(None, Mode::Changed, None, vec!["/a".into()]);
        assert_eq!(next.mode(), Mode::Changed);
        assert!(next.modules().is_empty());
        assert_eq!(next.folders(), [PathBuf::from("/a")]);
    }

    #[test]
    fn requests_fold_into_one_that_loads_all_when_any_asked_to() {
        let next = FollowUp::ask(None, Mode::All, None, Vec::new());
        let next = FollowUp::ask(Some(next), Mode::Changed, Some("/m/A.vst3"), Vec::new());
        assert_eq!(next.mode(), Mode::All, "a later Changed does not undo All");
        let next = FollowUp::ask(Some(next), Mode::Changed, Some("/m/A.vst3"), Vec::new());
        let next = FollowUp::ask(Some(next), Mode::Changed, Some("/m/B.vst3"), Vec::new());
        assert_eq!(next.modules(), ["/m/A.vst3", "/m/B.vst3"], "each once");
    }

    #[test]
    fn the_latest_request_says_which_folders_wait() {
        let added = FollowUp::ask(None, Mode::Changed, None, vec!["/a".into()]);
        let removed = FollowUp::ask(Some(added), Mode::Changed, None, Vec::new());
        assert!(removed.folders().is_empty(), "added and removed again");
    }
}
