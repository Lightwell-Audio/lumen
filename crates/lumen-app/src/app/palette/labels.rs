//! What a row will act on, in words: "Duplicate 2 clips", "Undo: Move clip",
//! "Rename track “Bass”".

use crate::App;
use crate::commands::Command;

/// "2 clips", "clip “kick”".
fn count(n: usize, one: &str, many: &str, name: Option<&str>) -> String {
    match (n, name) {
        (1, Some(name)) => format!("{one} \u{201C}{name}\u{201D}"),
        (1, None) => one.to_owned(),
        _ => format!("{n} {many}"),
    }
}

impl App {
    /// What `command` will act on, in words: "Duplicate 2 clips", "Undo:
    /// Move clip", "Rename track “Bass”". Plain names otherwise.
    pub(crate) fn command_label(&self, command: Command) -> String {
        let project = self.document.project();
        let clips: Vec<_> = project
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| self.arrangement.clips.contains(&c.id))
            .collect();
        let tracks: Vec<_> = project
            .tracks
            .iter()
            .filter(|t| self.arrangement.selected.contains(&t.id))
            .collect();
        let locator = self
            .locators
            .selected
            .and_then(|id| project.markers.iter().find(|m| m.id == id));
        let first_clip = clips.first().map(|c| c.name.as_str());
        let first_track = tracks.first().map(|t| t.name.as_str());
        let what = || {
            if self.arrangement.range.is_some() {
                Some("time range".to_owned())
            } else if !clips.is_empty() {
                Some(count(clips.len(), "clip", "clips", first_clip))
            } else if !tracks.is_empty() {
                Some(count(tracks.len(), "track", "tracks", first_track))
            } else {
                locator.map(|m| count(1, "locator", "locators", Some(&m.name)))
            }
        };
        let name = command.name();
        let history = self.document.history();
        match command {
            Command::Undo => history.undo_label().map(|l| format!("Undo: {l}")),
            Command::Redo => history.redo_label().map(|l| format!("Redo: {l}")),
            Command::Cut | Command::Copy | Command::Duplicate | Command::Delete => {
                what().map(|w| format!("{name} {w}"))
            }
            Command::Rename => what().map(|w| format!("Rename {w}")),
            Command::MuteClips if !clips.is_empty() => {
                let verb = if clips.iter().all(|c| c.muted) {
                    "Unmute"
                } else {
                    "Mute"
                };
                Some(format!(
                    "{verb} {}",
                    count(clips.len(), "clip", "clips", first_clip)
                ))
            }
            Command::ClipProperties => {
                first_clip.map(|n| format!("Properties of \u{201C}{n}\u{201D}"))
            }
            // The arrows follow the selection (M3 review H2).
            Command::FoldTracks if !clips.is_empty() => Some("Previous clip on the track".into()),
            Command::UnfoldTracks if !clips.is_empty() => Some("Next clip on the track".into()),
            Command::SelectTrackAbove if !clips.is_empty() => Some("Select the clip above".into()),
            Command::SelectTrackBelow if !clips.is_empty() => Some("Select the clip below".into()),
            Command::ExtendSelectionUp if !clips.is_empty() => {
                Some("Add the clip above to the selection".into())
            }
            Command::ExtendSelectionDown if !clips.is_empty() => {
                Some("Add the clip below to the selection".into())
            }
            Command::FoldTracks | Command::UnfoldTracks if !tracks.is_empty() => {
                let verb = if command == Command::FoldTracks {
                    "Fold"
                } else {
                    "Unfold"
                };
                Some(format!(
                    "{verb} {}",
                    count(tracks.len(), "track", "tracks", first_track)
                ))
            }
            Command::MoveTracksUp | Command::MoveTracksDown if !tracks.is_empty() => {
                let way = if command == Command::MoveTracksUp {
                    "up"
                } else {
                    "down"
                };
                Some(format!(
                    "Move {} {way}",
                    count(tracks.len(), "track", "tracks", first_track)
                ))
            }
            _ => None,
        }
        .unwrap_or_else(|| name.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_naturally() {
        assert_eq!(
            count(1, "clip", "clips", Some("kick")),
            "clip \u{201C}kick\u{201D}"
        );
        assert_eq!(count(3, "clip", "clips", Some("kick")), "3 clips");
    }
}
