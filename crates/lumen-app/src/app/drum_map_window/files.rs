//! A drum map as a file (M8 slice D1): the `.lumendrums` format, Lumen's own
//! folder of saved maps, and reading that folder.
//!
//! A file is JSON with a `version` at the top, so a map written by a later
//! copy of Lumen is refused with a message that says so rather than read
//! wrongly. A map saved here comes back exactly: every field of every sound,
//! in the map's order (the lanes').
//!
//! Reading and writing run off the interface's thread (`App::drum_map_file`):
//! a file dialog, a parse or a save never holds a frame.

use std::path::{Path, PathBuf};

use lumen_project::model::{DrumMap, Sound};
use serde::{Deserialize, Serialize};

/// The format's version: 1 for the first (M8 D1). A file with a higher one is
/// from a later copy of Lumen, and is refused.
pub const VERSION: u32 = 1;
/// The extension every drum map file has.
pub const EXTENSION: &str = "lumendrums";
/// The folder saved maps are kept in, inside the user's settings folder.
pub const FOLDER: &str = "Drum Maps";

/// A map as a file holds it. The `id` is the project's business, not the
/// file's: an imported map gets the project's own.
#[derive(Debug, Serialize, Deserialize)]
struct File {
    version: u32,
    name: String,
    sounds: Vec<Sound>,
}

/// `map` as the bytes of a `.lumendrums` file.
pub fn encode(map: &DrumMap) -> Result<Vec<u8>, String> {
    let file = File {
        version: VERSION,
        name: map.name.clone(),
        sounds: map.sounds.clone(),
    };
    let mut bytes = serde_json::to_vec_pretty(&file)
        .map_err(|e| format!("the drum map could not be written: {e}"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// The map in `bytes`, or why a person cannot use the file, in a sentence read
/// in full. `what` names the file in the message (a path or a name).
pub fn decode(bytes: &[u8], what: &str) -> Result<DrumMap, String> {
    let file: File = serde_json::from_slice(bytes).map_err(|error| {
        format!(
            "{what} is not a drum map {product} can read: {error}",
            product = lumen_core::product_name!()
        )
    })?;
    if file.version > VERSION {
        return Err(format!(
            "{what} was saved by a later {product} (version {}); this copy reads drum maps up to version {VERSION}",
            file.version,
            product = lumen_core::product_name!()
        ));
    }
    if file.version < 1 {
        return Err(format!("{what} does not say which version it is"));
    }
    check(&file.name, &file.sounds).map_err(|why| format!("{what} cannot be used: {why}"))?;
    Ok(DrumMap::new(
        lumen_project::model::Id(0),
        file.name,
        file.sounds,
    ))
}

/// Why a file's map cannot be a project's map, in a few words, or `Ok`. The
/// same rules the project keeps (`lumen_project::validate`): a hundred and
/// twenty-eight notes, unique, each a note; names that fit; no choke group 0.
fn check(name: &str, sounds: &[Sound]) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("it has no name".to_owned());
    }
    if name.chars().count() > 64 {
        return Err("its name is too long".to_owned());
    }
    if sounds.is_empty() {
        return Err("it has no sounds".to_owned());
    }
    if sounds.len() > 256 {
        return Err("it has more sounds than a map can hold".to_owned());
    }
    let mut seen = std::collections::BTreeSet::new();
    for sound in sounds {
        if sound.name.chars().count() > 64 {
            return Err(format!("a sound's name is too long ({:?})", sound.name));
        }
        if sound.input > 127 || sound.output > 127 {
            return Err(format!("{:?} is not a note", sound.name));
        }
        if !seen.insert(sound.input) {
            return Err(format!(
                "two sounds answer to note {} ({:?})",
                sound.input, sound.name
            ));
        }
        if sound.channel.is_some_and(|c| c > 15) {
            return Err(format!("{:?} plays on a channel there is not", sound.name));
        }
        if sound.choke == Some(0) {
            return Err(format!("{:?} is in choke group 0", sound.name));
        }
    }
    Ok(())
}

/// A map saved in the user's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// The map's name, as the file says (the file's name when it cannot be
    /// read).
    pub name: String,
    pub path: PathBuf,
}

/// The folder saved maps live in: `<settings folder>/Drum Maps`.
pub fn user_folder(settings: Option<&Path>) -> Option<PathBuf> {
    let settings = settings?;
    let folder = settings.parent()?.join(FOLDER);
    Some(folder)
}

/// The maps in `folder`, by name, quietly skipping what is not one: one bad
/// file must not hide the others. A folder that is not there is empty.
pub fn list(folder: &Path) -> Vec<Saved> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<Saved> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(EXTENSION))
        })
        .filter_map(|path| {
            let bytes = std::fs::read(&path).ok()?;
            let map = decode(&bytes, &file_name(&path)).ok()?;
            Some(Saved {
                name: map.name,
                path,
            })
        })
        .collect();
    found.sort_by_key(|saved| saved.name.to_lowercase());
    found
}

/// Saves `map` in `folder` and gives the file it wrote. A name already taken
/// is kept: the new file is "Name 2.lumendrums", then 3 and so on, so saving
/// never loses an earlier map. The folder is made if it is not there.
pub fn save_user(map: &DrumMap, folder: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("the folder {} could not be made: {e}", folder.display()))?;
    let bytes = encode(map)?;
    let stem = file_stem(&map.name, "Drum map");
    let mut path = folder.join(format!("{stem}.{EXTENSION}"));
    let mut n = 2;
    while path.exists() {
        path = folder.join(format!("{stem} {n}.{EXTENSION}"));
        n += 1;
        if n > 999 {
            return Err(format!(
                "there are too many maps called \u{201c}{}\u{201d} in {}",
                map.name,
                folder.display()
            ));
        }
    }
    std::fs::write(&path, bytes)
        .map_err(|e| format!("{} could not be written: {e}", path.display()))?;
    Ok(path)
}

/// A file name for a map called `name`: letters, digits, spaces, dashes and
/// underscores kept, runs of spaces folded, and nothing longer than 64
/// characters.
pub fn file_stem(name: &str, fallback: &str) -> String {
    let mut stem: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect();
    if let Some(cut) = stem.char_indices().nth(64) {
        stem.truncate(cut.0);
    }
    let stem = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    if stem.is_empty() {
        fallback.to_owned()
    } else {
        stem
    }
}

/// A path's file name, for a message.
pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use lumen_core::time::Ticks;
    use lumen_project::drums::presets::Preset;
    use lumen_project::model::{Color, DrumIcon, Id, Quantize};
    use lumen_project::testing::TempDir;

    use super::*;

    /// A map with one of everything in it.
    fn busy() -> DrumMap {
        let mut map = Preset::EIGHT_OH_EIGHT.map(Id(7));
        map.name = "Desk kit".into();
        map.sounds[0].output = 40;
        map.sounds[0].channel = Some(9);
        map.sounds[0].mute = true;
        map.sounds[0].length = Some(Ticks(240));
        map.sounds[0].choke = Some(2);
        map.sounds[0].color = Color(0x123456);
        map.sounds[0].icon = DrumIcon::Cowbell;
        map.sounds[0].quantize = Some(Quantize {
            size: lumen_core::grid::Size::Note(16),
            feel: lumen_core::grid::Feel::Triplet,
        });
        map.sounds[1].solo = true;
        map.sounds[2].name = "A name with spaces".into();
        map
    }

    #[test]
    fn a_map_saved_and_read_again_is_the_same_in_every_field() {
        let map = busy();
        let bytes = encode(&map).expect("it writes");
        let read = decode(&bytes, "Desk kit.lumendrums").expect("it reads");
        assert_eq!(read.name, map.name);
        assert_eq!(read.sounds, map.sounds, "every field of every sound");
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("\"version\": 1"), "{text}");
    }

    #[test]
    fn a_damaged_file_is_refused_with_a_reason_read_in_full() {
        let broken = b"{ \"version\": 1, \"name\": \"Kit\", \"sounds\": [ {";
        let why = decode(broken, "Broken.lumendrums").expect_err("refused");
        assert!(
            why.starts_with("Broken.lumendrums is not a drum map"),
            "{why}"
        );
        assert!(why.contains("can read:"), "the reader's own words: {why}");
        assert!(!why.ends_with("\u{2026}"), "the message is in full");
    }

    #[test]
    fn a_file_from_a_later_copy_is_refused_by_version() {
        let mut map = busy();
        map.sounds.truncate(1);
        let bytes = encode(&map).unwrap();
        let text =
            String::from_utf8(bytes)
                .unwrap()
                .replacen("\"version\": 1", "\"version\": 7", 1);
        let why = decode(text.as_bytes(), "Future.lumendrums").expect_err("refused");
        assert!(why.contains("later"), "{why}");
        assert!(why.contains("version 7"), "{why}");
        assert!(why.contains("up to version 1"), "{why}");
        // A file that does not say which version it is is damaged, not new.
        let bare = "{\"name\": \"K\", \"sounds\": []}";
        assert!(decode(bare.as_bytes(), "Old.lumendrums").is_err());
    }

    #[test]
    fn a_map_that_breaks_the_projects_rules_is_refused_by_name() {
        let mut map = busy();
        map.sounds[1].input = map.sounds[0].input;
        let why = decode(&encode(&map).unwrap(), "Twice.lumendrums").expect_err("refused");
        assert!(why.contains("two sounds answer to note"), "{why}");
        let mut empty = busy();
        empty.sounds.clear();
        let why = decode(&encode(&empty).unwrap(), "Empty.lumendrums").expect_err("refused");
        assert!(why.contains("no sounds"), "{why}");
        let mut choke = busy();
        choke.sounds[0].choke = Some(0);
        let why = decode(&encode(&choke).unwrap(), "Choke.lumendrums").expect_err("refused");
        assert!(why.contains("choke group 0"), "{why}");
    }

    #[test]
    fn saving_into_the_users_folder_keeps_every_map_of_one_name() {
        let dir = TempDir::new("drum-maps-folder");
        let map = busy();
        let first = save_user(&map, dir.path()).expect("saved");
        assert_eq!(
            first.file_name().unwrap().to_string_lossy(),
            "Desk kit.lumendrums"
        );
        let second = save_user(&map, dir.path()).expect("saved again");
        assert_eq!(
            second.file_name().unwrap().to_string_lossy(),
            "Desk kit 2.lumendrums",
            "a map is never overwritten"
        );
        let saved = list(dir.path());
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].name, "Desk kit");
        // What is not a drum map is left out of the list.
        std::fs::write(dir.path().join("notes.txt"), b"hello").unwrap();
        std::fs::write(dir.path().join("broken.lumendrums"), b"{").unwrap();
        assert_eq!(list(dir.path()).len(), 2);
        assert!(list(&dir.path().join("not there")).is_empty());
    }

    #[test]
    fn file_names_are_safe_and_short() {
        assert_eq!(file_stem("Kit / Live", "x"), "Kit Live");
        assert_eq!(file_stem("  ", "Drum map"), "Drum map");
        assert_eq!(file_stem("a".repeat(90).as_str(), "x").chars().count(), 64);
        let long = format!("{}: v2?", "K".repeat(10));
        assert_eq!(file_stem(&long, "x"), format!("{} v2", "K".repeat(10)));
    }

    #[test]
    fn the_users_folder_sits_beside_the_settings() {
        let folder = user_folder(Some(Path::new("/home/me/AppData/Lumen/settings.json")));
        assert_eq!(
            folder,
            Some(PathBuf::from("/home/me/AppData/Lumen/Drum Maps"))
        );
        assert_eq!(user_folder(None), None);
    }
}
