use std::fs;
use std::path::{Path, PathBuf};

use lumen_project::model::Project;
use lumen_project::{load, to_bytes};

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

pub fn golden(version: &str, expected: impl Fn() -> Project) -> Project {
    let path = fixture(&format!("{version}/golden.lumen"));
    if !path.exists() {
        assert!(
            std::env::var_os("LUMEN_WRITE_FIXTURES").is_some(),
            "{} is missing: a golden file is frozen and must not be deleted. \
             To write a new version's golden once, run with LUMEN_WRITE_FIXTURES=1, \
             then add its line to fixtures/golden_hashes.txt.",
            path.display()
        );
        fs::create_dir_all(path.parent().expect("a golden file has a folder")).unwrap();
        fs::write(&path, to_bytes(&expected()).unwrap()).unwrap();
    }
    let project =
        load(&path).unwrap_or_else(|error| panic!("{} must load: {error}", path.display()));
    assert_eq!(
        project,
        expected(),
        "{} must load as its model",
        path.display()
    );
    project
}
