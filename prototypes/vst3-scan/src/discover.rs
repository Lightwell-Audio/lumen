use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Layout {
    Bundle,
    SingleFile,
}

#[derive(Debug, Clone, Serialize)]
pub struct Module {
    pub path: PathBuf,
    pub layout: Layout,
    pub binary: Option<PathBuf>,
    pub moduleinfo: Option<PathBuf>,
}

pub fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(common) = std::env::var_os("CommonProgramFiles") {
        roots.push(PathBuf::from(common).join("VST3"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(
            PathBuf::from(local)
                .join("Programs")
                .join("Common")
                .join("VST3"),
        );
    }
    roots
}

pub fn discover(roots: &[PathBuf]) -> Vec<Module> {
    let mut modules = Vec::new();
    for root in roots {
        walk(root, &mut modules);
    }
    modules.sort_by(|a, b| a.path.cmp(&b.path));
    modules
}

fn is_vst3(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("vst3"))
}

fn walk(dir: &Path, out: &mut Vec<Module>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let is_dir = kind.is_dir() || (kind.is_symlink() && path.is_dir());
        if is_vst3(&path) {
            if is_dir {
                out.push(bundle(&path));
            } else {
                out.push(Module {
                    binary: Some(path.clone()),
                    path,
                    layout: Layout::SingleFile,
                    moduleinfo: None,
                });
            }
        } else if is_dir {
            walk(&path, out);
        }
    }
}

fn bundle(path: &Path) -> Module {
    let arch_dir = path.join("Contents").join("x86_64-win");
    let expected = path.file_name().map(|name| arch_dir.join(name));
    let binary = expected.filter(|p| p.is_file()).or_else(|| {
        std::fs::read_dir(&arch_dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| is_vst3(p) && p.is_file())
    });
    let moduleinfo = [
        path.join("Contents")
            .join("Resources")
            .join("moduleinfo.json"),
        path.join("Contents").join("moduleinfo.json"),
    ]
    .into_iter()
    .find(|p| p.is_file());
    Module {
        path: path.to_path_buf(),
        layout: Layout::Bundle,
        binary,
        moduleinfo,
    }
}
