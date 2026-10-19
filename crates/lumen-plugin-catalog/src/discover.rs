//! Finding VST3 modules on disk, in both layouts seen in the wild: a bundle
//! folder holding the binary for each platform, and a bare library named
//! `Name.vst3`. Only reads folder listings; nothing is loaded.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use lumen_plugin_host::module::binary_path;

/// How far below a root a module may sit (vendor folders inside the root).
pub const MAX_DEPTH: usize = 4;
/// The most modules one discovery returns, so a runaway folder cannot make
/// a scan unbounded.
pub const MAX_MODULES: usize = 4096;

/// A module found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The `.vst3` bundle folder or file: what the user knows it by.
    pub path: PathBuf,
    /// The library that is loaded: `path` itself, or inside the bundle.
    pub binary: PathBuf,
}

/// What a discovery found and could not use.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub modules: Vec<Module>,
    /// Bundles with no binary for this platform (a bundle for another system).
    pub without_binary: Vec<PathBuf>,
    /// Roots that do not exist or cannot be read.
    pub missing_roots: Vec<PathBuf>,
    /// There were more than [`MAX_MODULES`] modules: the rest were left out.
    pub truncated: bool,
}

/// The folders VST3 plugins install to by default on this platform.
pub fn standard_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    if cfg!(windows) {
        if let Some(common) = var("CommonProgramFiles") {
            roots.push(common.join("VST3"));
        }
        if let Some(local) = var("LOCALAPPDATA") {
            roots.push(local.join("Programs").join("Common").join("VST3"));
        }
    } else if cfg!(target_os = "macos") {
        roots.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
        if let Some(home) = var("HOME") {
            roots.push(home.join("Library/Audio/Plug-Ins/VST3"));
        }
    } else {
        if let Some(home) = var("HOME") {
            roots.push(home.join(".vst3"));
        }
        roots.push(PathBuf::from("/usr/local/lib/vst3"));
        roots.push(PathBuf::from("/usr/lib/vst3"));
    }
    roots
}

/// Every module under `roots`, sorted by path, each library once even when
/// two roots (or a link) lead to it.
pub fn discover(roots: &[PathBuf]) -> Discovered {
    let mut found = Discovered::default();
    let mut visited = HashSet::new();
    for root in roots {
        if !root.is_dir() {
            found.missing_roots.push(root.clone());
            continue;
        }
        if let Ok(real) = root.canonicalize() {
            visited.insert(real);
        }
        walk(root, 0, &mut visited, &mut found);
    }
    found.modules.sort_by(|a, b| a.path.cmp(&b.path));
    let mut seen = HashSet::new();
    found
        .modules
        .retain(|m| seen.insert(m.binary.canonicalize().unwrap_or_else(|_| m.binary.clone())));
    if found.modules.len() > MAX_MODULES {
        found.truncated = true;
        found.modules.truncate(MAX_MODULES);
    }
    found.without_binary.sort();
    found
}

fn is_vst3(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("vst3"))
}

fn walk(dir: &Path, depth: usize, visited: &mut HashSet<PathBuf>, out: &mut Discovered) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if out.modules.len() >= MAX_MODULES {
            out.truncated = true;
            return;
        }
        // `is_dir` follows links, which some installers use for bundles.
        let is_dir = path.is_dir();
        if is_vst3(&path) {
            if is_dir {
                match binary_path(&path) {
                    Ok(binary) => out.modules.push(Module { path, binary }),
                    Err(_) => out.without_binary.push(path),
                }
            } else if path.is_file() {
                out.modules.push(Module {
                    binary: path.clone(),
                    path,
                });
            }
        } else if is_dir && depth < MAX_DEPTH {
            // A link back up the tree must not be followed forever.
            let Ok(real) = path.canonicalize() else {
                continue;
            };
            if visited.insert(real) {
                walk(&path, depth + 1, visited, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("lumen-discover-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Tree(dir)
        }

        fn file(&self, relative: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"x").unwrap();
            path
        }

        fn dir(&self, relative: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(&path).unwrap();
            path
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The path inside a bundle where this platform keeps its binary.
    fn inside(bundle: &str) -> String {
        let (arch, ext) = if cfg!(windows) {
            ("x86_64-win", "vst3")
        } else if cfg!(target_os = "linux") {
            ("x86_64-linux", "so")
        } else {
            ("MacOS", "")
        };
        let stem = bundle.trim_end_matches(".vst3");
        let name = if ext.is_empty() {
            stem.to_string()
        } else {
            format!("{stem}.{ext}")
        };
        format!("{bundle}/Contents/{arch}/{name}")
    }

    #[test]
    fn a_bundle_and_a_bare_library_are_both_found() {
        let tree = Tree::new("layouts");
        tree.file(&inside("Bundle.vst3"));
        tree.file("Bare.vst3");
        let found = discover(std::slice::from_ref(&tree.0));
        let names: Vec<_> = found
            .modules
            .iter()
            .map(|m| m.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Bare.vst3", "Bundle.vst3"]);
        assert!(found.modules[0].binary.ends_with("Bare.vst3"));
        assert!(found.modules[1].binary.starts_with(&found.modules[1].path));
        assert!(found.without_binary.is_empty());
    }

    #[test]
    fn vendor_folders_are_searched_but_not_beyond_the_depth_limit() {
        let tree = Tree::new("depth");
        tree.file("Vendor/Pack/Near.vst3");
        let mut deep = String::new();
        for i in 0..=MAX_DEPTH + 1 {
            deep.push_str(&format!("d{i}/"));
        }
        tree.file(&format!("{deep}Far.vst3"));
        let found = discover(std::slice::from_ref(&tree.0));
        let names: Vec<_> = found
            .modules
            .iter()
            .map(|m| m.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Near.vst3"]);
    }

    #[test]
    fn a_bundle_without_a_binary_for_this_platform_is_set_aside() {
        let tree = Tree::new("nobinary");
        tree.dir("Other.vst3/Contents/arm64-other");
        tree.file("Other.vst3/Contents/arm64-other/Other.so");
        let found = discover(std::slice::from_ref(&tree.0));
        assert!(found.modules.is_empty());
        assert_eq!(found.without_binary.len(), 1);
    }

    #[test]
    fn a_missing_root_is_reported_and_the_others_still_scan() {
        let tree = Tree::new("roots");
        tree.file("A.vst3");
        let missing = tree.0.join("nowhere");
        let found = discover(&[missing.clone(), tree.0.clone()]);
        assert_eq!(found.missing_roots, [missing]);
        assert_eq!(found.modules.len(), 1);
    }

    #[test]
    fn one_library_reached_through_two_roots_is_listed_once() {
        let tree = Tree::new("twice");
        tree.file("Vendor/A.vst3");
        let found = discover(&[tree.0.clone(), tree.0.join("Vendor")]);
        assert_eq!(found.modules.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_back_up_the_tree_does_not_loop() {
        let tree = Tree::new("loop");
        tree.file("Vendor/A.vst3");
        std::os::unix::fs::symlink(&tree.0, tree.0.join("Vendor/again")).unwrap();
        let found = discover(std::slice::from_ref(&tree.0));
        assert_eq!(found.modules.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_bundle_is_followed() {
        let tree = Tree::new("linked");
        let real = tree.file(&format!("store/{}", inside("Real.vst3")));
        let bundle = real.ancestors().nth(3).unwrap().to_path_buf();
        std::fs::create_dir_all(tree.0.join("plugins")).unwrap();
        std::os::unix::fs::symlink(&bundle, tree.0.join("plugins/Real.vst3")).unwrap();
        let found = discover(&[tree.0.join("plugins")]);
        assert_eq!(found.modules.len(), 1);
    }

    #[test]
    fn the_standard_roots_are_not_empty_and_are_never_read_by_tests() {
        assert!(!standard_roots().is_empty());
    }

    #[test]
    fn more_modules_than_the_cap_are_left_out_and_it_is_said() {
        let tree = Tree::new("cap");
        for i in 0..MAX_MODULES + 3 {
            tree.file(&format!("P{i:05}.vst3"));
        }
        let found = discover(std::slice::from_ref(&tree.0));
        assert_eq!(found.modules.len(), MAX_MODULES);
        assert!(found.truncated);

        let few = Tree::new("cap-few");
        few.file("A.vst3");
        let found = discover(std::slice::from_ref(&few.0));
        assert!(!found.truncated);
    }
}
