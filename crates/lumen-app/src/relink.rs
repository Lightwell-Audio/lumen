//! Relinking missing media (M9 slice R4b, ADR-0106): checking a file against
//! what the project knows of the one it replaces, and searching a folder for
//! the files a project lost.
//!
//! A file is trusted when its name and size are the project's; otherwise its
//! header must agree on channels and sample rate, and a different length is
//! never taken without the user's say. Everything here reads the disk and
//! belongs on a worker.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use lumen_project::model::{Id, Media};

use crate::files::Control;
use crate::media::is_audio_file;

/// Entries a folder search looks at before it stops and says so.
pub const MAX_ENTRIES: usize = 200_000;

/// What a candidate file is, measured against the media it would replace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The size the project recorded.
    Exact,
    /// The same channels, rate and length.
    SameAudio,
    /// The same channels and rate, another length (`None` when the format
    /// does not say). Taken only when the user confirms.
    DifferentLength(Option<u64>),
    /// Cannot stand in for it; the reason, ready to show.
    Refused(String),
}

impl Verdict {
    fn rank(&self) -> Option<u8> {
        match self {
            Verdict::Exact => Some(0),
            Verdict::SameAudio => Some(1),
            Verdict::DifferentLength(_) => Some(2),
            Verdict::Refused(_) => None,
        }
    }
}

/// How `file` measures against `media`. Reads the file's size and header.
pub fn verdict(media: &Media, file: &Path) -> Verdict {
    let size = match std::fs::metadata(file) {
        Ok(meta) if meta.is_file() => meta.len(),
        Ok(_) => return Verdict::Refused("That is not a file".into()),
        Err(e) => return Verdict::Refused(format!("Could not read the file: {e}")),
    };
    if media.size_bytes > 0 && size == media.size_bytes {
        return Verdict::Exact;
    }
    let header = match lumen_media::probe(file) {
        Ok(header) => header,
        Err(e) => return Verdict::Refused(e.to_string()),
    };
    if let Some(channels) = header.channels
        && channels != usize::from(media.channels)
    {
        return Verdict::Refused(format!(
            "It has {channels} channels, the project's file has {}",
            media.channels
        ));
    }
    if let Some(rate) = header.sample_rate
        && rate != media.sample_rate
    {
        return Verdict::Refused(format!(
            "It is at {rate} Hz, the project's file is at {} Hz",
            media.sample_rate
        ));
    }
    match header.frames {
        Some(frames) if frames == media.frames => Verdict::SameAudio,
        other => Verdict::DifferentLength(other),
    }
}

/// A file a search found for a missing item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub id: Id,
    pub path: PathBuf,
    pub verdict: Verdict,
}

/// What a folder search came to. Nothing is applied by the search itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Safe to apply: the right size, or the same audio.
    pub found: Vec<Found>,
    /// Found with another length: left for the user to relink one by one.
    pub different: Vec<Found>,
    /// More than one file fits equally well, so none is chosen.
    pub ambiguous: Vec<(Id, Vec<PathBuf>)>,
    /// No file of the name, or none that can stand in for it.
    pub not_found: Vec<Id>,
    /// The search stopped at [`MAX_ENTRIES`].
    pub truncated: bool,
}

/// The search was stopped.
#[derive(Debug, PartialEq, Eq)]
pub struct Stopped;

/// Starts a search of `folder` for `wanted` in the background.
pub fn start(
    folder: PathBuf,
    wanted: Vec<Media>,
    control: Control,
) -> Receiver<Result<Report, Stopped>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("lumen-relink".into())
        .spawn(move || {
            let _ = tx.send(search(&folder, &wanted, &control));
        })
        .expect("spawn relink thread");
    rx
}

/// Looks through `folder` and its subfolders (not following links) for a file
/// with the name of each of `wanted`, and measures each against its media.
pub fn search(folder: &Path, wanted: &[Media], control: &Control) -> Result<Report, Stopped> {
    let names: HashMap<String, Vec<usize>> = {
        let mut names: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, media) in wanted.iter().enumerate() {
            if let Some(name) = file_name(&media.path) {
                names.entry(name).or_default().push(index);
            }
        }
        names
    };
    let mut candidates: Vec<Vec<PathBuf>> = vec![Vec::new(); wanted.len()];
    let mut report = Report::default();
    let mut pending = vec![folder.to_path_buf()];
    let (mut seen, mut visited) = (0_usize, 0_usize);
    while let Some(dir) = pending.pop() {
        if control.stopped() {
            return Err(Stopped);
        }
        visited += 1;
        control.set(0.9 * visited as f32 / (visited + pending.len()) as f32);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                report.truncated = true;
                pending.clear();
                break;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                let path = entry.path();
                if is_audio_file(&path)
                    && let Some(indexes) =
                        file_name(&path.to_string_lossy()).and_then(|n| names.get(&n))
                {
                    for &index in indexes {
                        candidates[index].push(path.clone());
                    }
                }
            }
        }
    }

    for (index, media) in wanted.iter().enumerate() {
        if control.stopped() {
            return Err(Stopped);
        }
        control.set(0.9 + 0.1 * index as f32 / wanted.len().max(1) as f32);
        let mut paths = std::mem::take(&mut candidates[index]);
        paths.sort();
        let mut measured: Vec<(PathBuf, Verdict, u8)> = paths
            .into_iter()
            .filter_map(|path| {
                let verdict = verdict(media, &path);
                verdict.rank().map(|rank| (path, verdict, rank))
            })
            .collect();
        let Some(best) = measured.iter().map(|(_, _, rank)| *rank).min() else {
            report.not_found.push(media.id);
            continue;
        };
        measured.retain(|(_, _, rank)| *rank == best);
        // Copies of one file in several folders are all the right file.
        if measured.len() > 1 && best != 0 {
            report
                .ambiguous
                .push((media.id, measured.into_iter().map(|m| m.0).collect()));
            continue;
        }
        let (path, verdict, _) = measured.swap_remove(0);
        let found = Found {
            id: media.id,
            path,
            verdict,
        };
        if best == 2 {
            report.different.push(found);
        } else {
            report.found.push(found);
        }
    }
    control.set(1.0);
    Ok(report)
}

/// The lower-cased last part of a path (`/` or `\` separated), for matching
/// the name of a file wherever it moved.
fn file_name(path: &str) -> Option<String> {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_lowercase)
}

#[cfg(test)]
mod tests {
    use lumen_media::wav::{Sample, Spec, Writer};
    use lumen_project::testing::TempDir;

    use super::*;

    fn write(path: &Path, rate: u32, channels: u16, frames: usize) -> u64 {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut writer = Writer::create(
            path,
            Spec {
                sample_rate: rate,
                channels,
                sample: Sample::F32,
            },
        )
        .unwrap();
        let channel = vec![0.25_f32; frames];
        let block: Vec<&[f32]> = (0..channels).map(|_| channel.as_slice()).collect();
        writer.write(&block).unwrap();
        writer.finish().unwrap();
        std::fs::metadata(path).unwrap().len()
    }

    fn media(id: u64, path: &str, rate: u32, channels: u16, frames: u64, size: u64) -> Media {
        Media {
            id: Id(id),
            path: path.into(),
            original_path: None,
            frames,
            channels,
            sample_rate: rate,
            size_bytes: size,
        }
    }

    #[test]
    fn the_size_the_project_recorded_is_exact() {
        let dir = TempDir::new("relink-exact");
        let file = dir.path().join("kick.wav");
        let size = write(&file, 48_000, 2, 1000);
        let m = media(1, "Samples/kick.wav", 48_000, 2, 1000, size);
        assert_eq!(verdict(&m, &file), Verdict::Exact);
    }

    #[test]
    fn another_size_is_judged_by_the_header() {
        let dir = TempDir::new("relink-header");
        let file = dir.path().join("a.wav");
        write(&file, 48_000, 2, 1000);
        let same = media(1, "a.wav", 48_000, 2, 1000, 1);
        assert_eq!(verdict(&same, &file), Verdict::SameAudio);
        let longer = media(1, "a.wav", 48_000, 2, 2000, 1);
        assert_eq!(
            verdict(&longer, &file),
            Verdict::DifferentLength(Some(1000))
        );
        let mono = media(1, "a.wav", 48_000, 1, 1000, 1);
        assert!(matches!(verdict(&mono, &file), Verdict::Refused(m) if m.contains("2 channels")));
        let rate = media(1, "a.wav", 44_100, 2, 1000, 1);
        assert!(matches!(verdict(&rate, &file), Verdict::Refused(m) if m.contains("48000 Hz")));
    }

    #[test]
    fn something_that_is_not_audio_is_refused() {
        let dir = TempDir::new("relink-text");
        let file = dir.path().join("a.wav");
        std::fs::write(&file, b"not audio").unwrap();
        let m = media(1, "a.wav", 48_000, 2, 1000, 1);
        assert!(matches!(verdict(&m, &file), Verdict::Refused(_)));
        assert!(matches!(
            verdict(&m, &dir.path().join("none.wav")),
            Verdict::Refused(_)
        ));
    }

    #[test]
    fn a_moved_project_folder_is_found_by_name_in_one_search() {
        let dir = TempDir::new("relink-moved");
        let kick = write(&dir.path().join("old/drums/Kick.wav"), 48_000, 2, 1000);
        let pad = write(&dir.path().join("old/pads/pad.wav"), 48_000, 2, 3000);
        let wanted = vec![
            media(1, "Samples/Imported/kick.wav", 48_000, 2, 1000, kick),
            media(2, "C:/Music/pad.wav", 48_000, 2, 3000, pad),
            media(3, "Samples/Imported/gone.wav", 48_000, 2, 10, 5),
        ];
        let report = search(dir.path(), &wanted, &Control::default()).unwrap();
        let ids: Vec<_> = report.found.iter().map(|f| f.id).collect();
        assert_eq!(ids, [Id(1), Id(2)]);
        assert!(
            report.found[0].path.ends_with("Kick.wav"),
            "any case matches"
        );
        assert_eq!(report.not_found, [Id(3)]);
        assert!(report.ambiguous.is_empty() && report.different.is_empty());
        assert!(!report.truncated);
    }

    #[test]
    fn two_equal_matches_are_left_alone_unless_they_are_the_same_size() {
        let dir = TempDir::new("relink-ambiguous");
        // Two takes with one name and the project's header, neither its size.
        write(&dir.path().join("a/snare.wav"), 48_000, 1, 500);
        write(&dir.path().join("b/snare.wav"), 48_000, 1, 500);
        let wanted = vec![media(1, "snare.wav", 48_000, 1, 500, 1)];
        let report = search(dir.path(), &wanted, &Control::default()).unwrap();
        assert!(report.found.is_empty());
        assert_eq!(report.ambiguous.len(), 1);
        assert_eq!(report.ambiguous[0].1.len(), 2);

        // Two copies of the exact file are the same file: the first is taken.
        let size = std::fs::metadata(dir.path().join("a/snare.wav"))
            .unwrap()
            .len();
        let wanted = vec![media(1, "snare.wav", 48_000, 1, 500, size)];
        let report = search(dir.path(), &wanted, &Control::default()).unwrap();
        assert_eq!(report.found.len(), 1);
        assert!(report.found[0].path.ends_with("a/snare.wav"));
        assert!(report.ambiguous.is_empty());
    }

    #[test]
    fn a_file_of_another_length_is_listed_but_not_among_the_found() {
        let dir = TempDir::new("relink-different");
        write(&dir.path().join("loop.wav"), 48_000, 2, 1000);
        let wanted = vec![media(1, "loop.wav", 48_000, 2, 5000, 1)];
        let report = search(dir.path(), &wanted, &Control::default()).unwrap();
        assert!(report.found.is_empty());
        assert_eq!(report.different.len(), 1);
        assert_eq!(
            report.different[0].verdict,
            Verdict::DifferentLength(Some(1000))
        );
    }

    #[test]
    fn a_wrong_format_of_the_right_name_is_not_found() {
        let dir = TempDir::new("relink-refused");
        write(&dir.path().join("mono.wav"), 48_000, 1, 1000);
        let wanted = vec![media(1, "mono.wav", 48_000, 2, 1000, 1)];
        let report = search(dir.path(), &wanted, &Control::default()).unwrap();
        assert_eq!(report.not_found, [Id(1)]);
    }

    #[test]
    fn a_stopped_search_gives_nothing() {
        let dir = TempDir::new("relink-stop");
        write(&dir.path().join("a.wav"), 48_000, 1, 100);
        let control = Control::default();
        control.stop();
        let wanted = vec![media(1, "a.wav", 48_000, 1, 100, 1)];
        assert_eq!(search(dir.path(), &wanted, &control), Err(Stopped));
    }

    #[test]
    fn file_names_match_whatever_the_separator() {
        assert_eq!(
            file_name("Samples/Imported/Kick.WAV").as_deref(),
            Some("kick.wav")
        );
        assert_eq!(file_name(r"C:\Music\Kick.wav").as_deref(), Some("kick.wav"));
        assert_eq!(file_name("dir/"), None);
    }
}
