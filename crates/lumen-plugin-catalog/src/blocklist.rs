use std::path::Path;

use crate::cache::{Entry, Reason};

impl Reason {
    pub fn headline(&self) -> String {
        match self {
            Reason::Crashed { how, .. } => {
                format!("It crashed while loading ({how}).")
            }
            Reason::TimedOut { seconds } => {
                format!("It did not respond within {seconds} seconds.")
            }
            Reason::Failed { message } => format!("It could not be loaded: {message}"),
            Reason::Malformed { message } => {
                format!("It gave a reply Lumen could not read: {message}")
            }
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Reason::Crashed { detail, .. } if !detail.is_empty() => Some(detail),
            _ => None,
        }
    }
}

pub const WHEN_RETRIED: &str =
    "Lumen left it out of the list and will not try again until its file changes.";

pub fn display_name(entry: &Entry) -> String {
    let path = Path::new(&entry.path);
    path.file_stem()
        .or_else(|| path.file_name())
        .map_or_else(|| entry.path.clone(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::Result;
    use crate::stamp::Stamp;

    #[test]
    fn each_reason_reads_as_a_sentence_with_its_facts() {
        assert_eq!(
            Reason::Crashed {
                how: "signal 11".into(),
                detail: String::new()
            }
            .headline(),
            "It crashed while loading (signal 11)."
        );
        assert_eq!(
            Reason::TimedOut { seconds: 30 }.headline(),
            "It did not respond within 30 seconds."
        );
        assert!(
            Reason::Failed {
                message: "no factory".into()
            }
            .headline()
            .contains("no factory")
        );
        assert!(
            Reason::Malformed {
                message: "bad".into()
            }
            .headline()
            .contains("bad")
        );
    }

    #[test]
    fn detail_is_present_only_for_a_crash_that_printed_something() {
        let crash = |detail: &str| Reason::Crashed {
            how: "x".into(),
            detail: detail.into(),
        };
        assert_eq!(crash("boom").detail(), Some("boom"));
        assert_eq!(crash("").detail(), None);
        assert_eq!(Reason::TimedOut { seconds: 1 }.detail(), None);
    }

    #[test]
    fn a_blocked_module_is_named_by_its_file() {
        let entry = Entry {
            path: "/plugins/Vendor/Thing.vst3".into(),
            binary: String::new(),
            stamp: Stamp {
                len: 0,
                modified_ns: 0,
            },
            result: Result::Blocked {
                reason: Reason::TimedOut { seconds: 1 },
                at: 0,
            },
        };
        assert_eq!(display_name(&entry), "Thing");
    }
}
