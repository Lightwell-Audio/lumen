//! Publish all stems together. Old files are moved aside until every rename
//! succeeds; a failed publish restores them. Only this task's staging folder
//! is removed on drop.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) struct Stage(pub PathBuf);

impl Stage {
    pub fn new(parent: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        for _ in 0..100 {
            let path = parent.join(format!(
                ".lumen-export-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("Could not create a private export folder".into())
    }

    pub fn publish(&self, files: &[(PathBuf, PathBuf)]) -> Result<(), String> {
        let mut old = Vec::new();
        let mut made = Vec::new();
        let result = (|| {
            for (index, (source, destination)) in files.iter().enumerate() {
                if destination.exists() {
                    if !destination.is_file() {
                        return Err(format!("{} is not a file", destination.display()));
                    }
                    let backup = self.0.join(format!("previous-{index}"));
                    std::fs::rename(destination, &backup).map_err(|e| e.to_string())?;
                    old.push((destination.clone(), backup));
                }
                std::fs::rename(source, destination).map_err(|e| e.to_string())?;
                made.push(destination.clone());
            }
            Ok(())
        })();
        if let Err(why) = result {
            for path in made {
                let _ = std::fs::remove_file(path);
            }
            let mut errors = Vec::new();
            for (destination, backup) in old.into_iter().rev() {
                if let Err(error) = std::fs::rename(&backup, &destination) {
                    errors.push(format!(
                        "{} is kept at {}: {error}",
                        destination.display(),
                        backup.display()
                    ));
                }
            }
            if !errors.is_empty() {
                // Preserve the backups when restoration itself fails.
                return Err(format!("{why}. Recovery files: {}", errors.join("; ")));
            }
            return Err(why);
        }
        for (_, backup) in old {
            let _ = std::fs::remove_file(backup);
        }
        Ok(())
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        // A remaining previous-* file may be the only copy after a failed
        // rollback. Keep that folder and report its path rather than erase it.
        if std::fs::read_dir(&self.0).is_ok_and(|mut entries| {
            entries
                .any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("previous-")))
        }) {
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_project::testing::TempDir;

    #[test]
    fn a_failed_second_publish_restores_the_first_existing_file() {
        let dir = TempDir::new("export-rollback");
        let first = dir.path().join("first.wav");
        let second = dir.path().join("second.wav");
        std::fs::write(&first, b"old first").unwrap();
        std::fs::write(&second, b"old second").unwrap();
        let stage = Stage::new(dir.path()).unwrap();
        let ready = stage.0.join("ready.wav");
        std::fs::write(&ready, b"new first").unwrap();
        assert!(
            stage
                .publish(&[
                    (ready, first.clone()),
                    (stage.0.join("missing.wav"), second.clone())
                ])
                .is_err()
        );
        assert_eq!(std::fs::read(&first).unwrap(), b"old first");
        assert_eq!(std::fs::read(&second).unwrap(), b"old second");
        drop(stage);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn successful_publish_removes_backups_and_staging() {
        let dir = TempDir::new("export-publish");
        let file = dir.path().join("mix.wav");
        std::fs::write(&file, b"old").unwrap();
        let stage = Stage::new(dir.path()).unwrap();
        let ready = stage.0.join("ready.wav");
        std::fs::write(&ready, b"new").unwrap();
        stage.publish(&[(ready, file.clone())]).unwrap();
        drop(stage);
        assert_eq!(std::fs::read(&file).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
