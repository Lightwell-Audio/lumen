//! New, open, save and save as; the unsaved-changes prompt and the file
//! banner (M3 slice 3c). Saving and opening run in the background
//! (`crate::files`); file dialogs run on their own thread, owned by the main
//! window (`crate::owner`).
//!
//! By concern (audit M10): the system dialogs and their answers (`dialogs`),
//! the folders projects and templates are kept in (`folders`), the save
//! (`save`), opening, new and quitting (`open`), importing files
//! (`import`), the results of all of them (`poll`) and the unsaved-changes
//! question (`prompt`).

mod dialogs;
mod folders;
mod import;
mod open;
mod poll;
mod prompt;
mod save;

pub(crate) use dialogs::{FileDialog, made};
pub(crate) use folders::{TEMPLATES, documents_folder, user_folder};
pub(crate) use open::Next;
