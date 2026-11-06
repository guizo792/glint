//! Builds an [`Index`] by walking folders on disk.
use crate::{DirId, Index, Kind};
use jwalk::{DirEntry, Parallelism, WalkDirGeneric};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

/// Folder names skipped by default (case-insensitive). Only names that are almost never
/// something a person wants to find: generic names like `build` or `dist` stay searchable
/// because they can be real user folders.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    "AppData",
    "node_modules",
    ".git",
    ".svn",
    ".hg",
    ".cache",
    ".venv",
    "__pycache__",
    "$Recycle.Bin",
    "System Volume Information",
];

#[derive(Debug, Clone)]
pub struct WalkConfig {
    pub roots: Vec<PathBuf>,
    /// Folder names to skip entirely, matched case-insensitively.
    pub exclude_dirs: Vec<String>,
    /// Walker threads. Kept small so indexing does not compete with searches.
    pub threads: usize,
}

impl WalkConfig {
    /// The current user's home folder with the default excludes.
    pub fn for_current_user() -> Self {
        WalkConfig {
            roots: std::env::home_dir().into_iter().collect(),
            exclude_dirs: DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect(),
            threads: 2,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WalkStats {
    /// Entries dropped because their name, or an ancestor's, is not valid UTF-8.
    pub skipped_names: usize,
    /// Folders or entries that could not be read, usually for lack of permission.
    pub unreadable: usize,
}

/// Walk every root into a new index.
pub fn build_index(cfg: &WalkConfig) -> (Index, WalkStats) {
    let excludes: Arc<HashSet<String>> =
        Arc::new(cfg.exclude_dirs.iter().map(|s| s.to_lowercase()).collect());
    let mut ix = Index::default();
    let mut stats = WalkStats::default();
    for root in &cfg.roots {
        let Some(root_name) = root.to_str() else {
            stats.skipped_names += 1;
            continue;
        };
        let excludes = excludes.clone();
        let walker = WalkDirGeneric::<((), ())>::new(root)
            .skip_hidden(false)
            .follow_links(false)
            .parallelism(Parallelism::RayonNewPool(cfg.threads.max(1)))
            .process_read_dir(move |_, _, _, children| {
                children.retain(|child| match child {
                    Ok(e) => !is_excluded(e, &excludes),
                    Err(_) => true,
                });
            });
        let mut dirs: HashMap<PathBuf, DirId> = HashMap::new();
        for entry in walker {
            let e = match entry {
                Ok(e) => e,
                Err(_) => {
                    stats.unreadable += 1;
                    continue;
                }
            };
            if e.read_children_error.is_some() {
                stats.unreadable += 1;
            }
            if e.depth() == 0 {
                dirs.insert(e.path(), ix.add_dir(None, root_name));
                continue;
            }
            // A missing parent means an ancestor's name was skipped.
            let (Some(name), Some(&parent)) = (e.file_name().to_str(), dirs.get(e.parent_path()))
            else {
                stats.skipped_names += 1;
                continue;
            };
            if e.file_type().is_dir() {
                ix.push(name, parent, Kind::Dir);
                dirs.insert(e.path(), ix.add_dir(Some(parent), name));
            } else {
                ix.push(name, parent, Kind::File);
            }
        }
    }
    (ix, stats)
}

fn is_excluded(e: &DirEntry<((), ())>, excludes: &HashSet<String>) -> bool {
    e.file_type().is_dir() && excludes.contains(&e.file_name().to_string_lossy().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sorted_names(ix: &Index) -> Vec<String> {
        let mut v: Vec<String> = (0..ix.len() as u32)
            .map(|i| ix.name(i).to_string())
            .collect();
        v.sort();
        v
    }

    fn make_tree(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("glint-walk-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for dir in ["Docs/تقارير", "node_modules/pkg", "APPDATA/Local", "build"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            "Docs/تقارير/résumé.pdf",
            "node_modules/pkg/index.js",
            "APPDATA/Local/cache.bin",
            "build/keep.txt",
            "node_modules.txt",
        ] {
            fs::write(root.join(file), b"x").unwrap();
        }
        root
    }

    fn config(root: &std::path::Path) -> WalkConfig {
        WalkConfig {
            roots: vec![root.to_path_buf()],
            exclude_dirs: DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect(),
            threads: 2,
        }
    }

    #[test]
    fn indexes_files_and_folders_and_skips_excluded_folders() {
        let root = make_tree("basic");
        let (ix, stats) = build_index(&config(&root));
        assert_eq!(
            sorted_names(&ix),
            [
                "Docs",
                "build",
                "keep.txt",
                "node_modules.txt",
                "résumé.pdf",
                "تقارير",
            ]
        );
        assert_eq!(stats, WalkStats::default());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rebuilt_paths_exist_with_the_right_kind() {
        let root = make_tree("paths");
        let (ix, _) = build_index(&config(&root));
        for id in 0..ix.len() as u32 {
            let m = fs::symlink_metadata(ix.path(id)).unwrap();
            assert_eq!(m.is_dir(), ix.kind(id) == Kind::Dir, "{:?}", ix.path(id));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_root_is_reported_not_fatal() {
        let root = std::env::temp_dir().join("glint-walk-does-not-exist");
        let (ix, stats) = build_index(&config(&root));
        assert!(ix.is_empty());
        assert_eq!(stats.unreadable, 1);
    }
}
