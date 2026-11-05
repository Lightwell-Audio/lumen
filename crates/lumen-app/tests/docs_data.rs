//! Writes the tables the user documentation's reference is generated from
//! (docs/USER_DOCS.md §5.1) as JSON: every command with its keys on both
//! systems, the command areas, every Preferences row and page, and the
//! transport bar's and app menu's controls. `site/docs/build.sh` runs it
//! before the site is built; the files are not committed.
//!
//! ```text
//! DOCS_DATA=site/docs/data cargo test -p lumen-app --test docs_data -- --ignored
//! ```

#[path = "shared/controls.rs"]
mod controls;

use std::path::PathBuf;

use lumen_app::testing::docs;
use serde_json::{Value, json};

use controls::{BAR, Has, MENU};

#[test]
#[ignore = "writes the docs' data files; needs DOCS_DATA"]
fn export() {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).expect("the data folder can be made");
    let commands = docs::commands();
    write(
        &dir,
        "commands.json",
        json!({ "commands": commands, "areas": docs::areas() }),
    );
    write(
        &dir,
        "preferences.json",
        json!({ "pages": docs::pages(), "rows": docs::preferences() }),
    );
    let id_of = |name: &str| {
        commands
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| {
                panic!("the controls list names a command, {name}, that is not in the table")
            })
            .id
    };
    let controls = |place: &str, list: &[(&str, Has)]| -> Vec<Value> {
        list.iter()
            .map(|(name, has)| match has {
                Has::Command(command) => {
                    json!({ "name": name, "place": place, "command": id_of(command) })
                }
                Has::Without(why) => json!({ "name": name, "place": place, "why": why }),
            })
            .collect()
    };
    let mut all = controls("bar", BAR);
    all.extend(controls("menu", MENU));
    write(&dir, "controls.json", json!({ "controls": all }));
}

/// `DOCS_DATA`, read from the repository root when relative.
fn data_dir() -> PathBuf {
    let dir = PathBuf::from(
        std::env::var_os("DOCS_DATA").expect("set DOCS_DATA to the site's data folder"),
    );
    if dir.is_absolute() {
        dir
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir)
    }
}

fn write(dir: &std::path::Path, name: &str, value: Value) {
    let text = serde_json::to_string_pretty(&value).expect("the tables serialise");
    std::fs::write(dir.join(name), text + "\n").expect("the data file can be written");
}
