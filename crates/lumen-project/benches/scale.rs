//! Save and load at quality-bar scale: 200 tracks, 19,900 clips, 99,500
//! notes, 199 plugin states of 4 KiB.

use lumen_project::model::{ClipContent, Project};
use lumen_project::testing::{TempDir, synthetic};
use lumen_project::{SaveOptions, from_bytes, save, to_bytes};

fn quality_bar_project() -> Project {
    synthetic(42, 200, 100, 10, 4_096)
}

fn main() {
    let project = quality_bar_project();
    let clips: usize = project.tracks.iter().map(|t| t.clips.len()).sum();
    let notes: usize = project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .map(|c| match &c.content {
            ClipContent::Midi { notes } => notes.len(),
            ClipContent::Audio { .. } => 0,
        })
        .sum();
    let bytes = to_bytes(&project).expect("valid project");
    eprintln!(
        "project: {} tracks, {clips} clips, {notes} notes; file size {:.2} MB",
        project.tracks.len(),
        bytes.len() as f64 / 1e6
    );
    divan::main();
}

#[divan::bench(sample_count = 10)]
fn serialize(bencher: divan::Bencher<'_, '_>) {
    let project = quality_bar_project();
    bencher.bench_local(|| to_bytes(divan::black_box(&project)).unwrap());
}

#[divan::bench(sample_count = 10)]
fn parse_migrate_validate(bencher: divan::Bencher<'_, '_>) {
    let bytes = to_bytes(&quality_bar_project()).unwrap();
    bencher.bench_local(|| from_bytes(divan::black_box(&bytes)).unwrap());
}

#[divan::bench(sample_count = 10)]
fn atomic_save_with_fsync(bencher: divan::Bencher<'_, '_>) {
    let project = quality_bar_project();
    let dir = TempDir::new("bench-save");
    let path = dir.path().join("Big.lumen");
    let options = SaveOptions { keep_backups: 0 };
    bencher.bench_local(|| save(&project, &path, &options).unwrap());
}
