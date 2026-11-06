//! Builds an [`Index`] by walking folders on disk.
use crate::{DirId, Index, Kind};
use jwalk::{DirEntry, Parallelism, WalkDirGeneric};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Folder names skipped by default (case-insensitive): machine-only contents nobody searches
/// for. Everything else stays findable; noisy folders rank lower instead of being hidden.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "__pycache__",
    "$Recycle.Bin",
    "System Volume Information",
];

/// Folders whose contents rank below normal results, wherever they appear. So does any folder
/// starting with a dot, which holds tool data such as `.rustup` or `.cache`.
const LOW_PRIORITY_ANYWHERE: &[&str] = &["appdata", "node_modules"];
/// Same, but only directly under a drive root: `Documents\Windows` is a normal folder.
const LOW_PRIORITY_AT_ROOT: &[&str] = &[
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
];

fn is_low_priority(name: &str, depth: usize) -> bool {
    let name = name.to_lowercase();
    name.starts_with('.')
        || LOW_PRIORITY_ANYWHERE.contains(&name.as_str())
        || (depth == 1 && LOW_PRIORITY_AT_ROOT.contains(&name.as_str()))
}

/// Whether any folder on `path` is low priority.
pub fn is_low_priority_path(path: &Path) -> bool {
    normal_parts(path)
        .enumerate()
        .any(|(i, name)| is_low_priority(name, i + 1))
}

/// Whether any part of `path` is an excluded folder name.
pub fn is_excluded_path(path: &Path, excludes: &[String]) -> bool {
    normal_parts(path).any(|name| excludes.iter().any(|e| e.eq_ignore_ascii_case(name)))
}

fn normal_parts(path: &Path) -> impl Iterator<Item = &str> {
    path.components().filter_map(|c| match c {
        Component::Normal(name) => name.to_str(),
        _ => None,
    })
}

/// Every fixed drive. Removable and network drives are left out so that an unreachable one
/// cannot stall indexing.
#[cfg(windows)]
fn fixed_drives() -> Vec<PathBuf> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    const DRIVE_FIXED: u32 = 3;
    // SAFETY: GetLogicalDrives takes no input; GetDriveTypeW gets a NUL-terminated string
    // that outlives the call.
    let mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter(|bit| mask & (1 << bit) != 0)
        .map(|bit| format!("{}:\\", (b'A' + bit as u8) as char))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain([0]).collect();
            unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) == DRIVE_FIXED }
        })
        .map(PathBuf::from)
        .collect()
}

#[cfg(not(windows))]
fn fixed_drives() -> Vec<PathBuf> {
    vec![PathBuf::from("/")]
}

#[derive(Debug, Clone)]
pub struct WalkConfig {
    pub roots: Vec<PathBuf>,
    /// Folder names to skip entirely, matched case-insensitively.
    pub exclude_dirs: Vec<String>,
    /// Walker threads. Kept small so indexing does not compete with searches.
    pub threads: usize,
}

impl WalkConfig {
    /// Every fixed drive on this PC with the default excludes.
    pub fn whole_pc() -> Self {
        WalkConfig {
            roots: fixed_drives(),
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
                let dir = if is_low_priority(name, e.depth()) {
                    ix.add_low_priority_dir(Some(parent), name)
                } else {
                    ix.add_dir(Some(parent), name)
                };
                dirs.insert(e.path(), dir);
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
        for dir in [
            "Docs/تقارير",
            "node_modules/pkg",
            "APPDATA/Local",
            "build",
            ".git/refs",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            "Docs/تقارير/résumé.pdf",
            "node_modules/pkg/index.js",
            "APPDATA/Local/report.txt",
            "build/old_report_copy.txt",
            "node_modules.txt",
            ".git/refs/HEAD",
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
                "APPDATA",
                "Docs",
                "Local",
                "build",
                "index.js",
                "node_modules",
                "node_modules.txt",
                "old_report_copy.txt",
                "pkg",
                "report.txt",
                "résumé.pdf",
                "تقارير",
            ]
        );
        assert_eq!(stats, WalkStats::default());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn noisy_folders_stay_findable_but_rank_lower() {
        let root = make_tree("rank");
        let (ix, _) = build_index(&config(&root));
        let hits = ix.search("report", 10);
        let found: Vec<&str> = hits.iter().map(|h| ix.name(h.id)).collect();
        assert_eq!(found, ["old_report_copy.txt", "report.txt"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn system_folder_names_only_count_at_the_drive_root() {
        assert!(is_low_priority("Windows", 1));
        assert!(!is_low_priority("Windows", 2));
        assert!(is_low_priority("AppData", 3));
        assert!(is_low_priority(".rustup", 3));
        assert!(!is_low_priority("Documents", 1));
    }

    #[cfg(windows)]
    #[test]
    fn whole_pc_includes_the_system_drive() {
        let system = std::env::var("SystemDrive").unwrap() + "\\";
        assert!(WalkConfig::whole_pc()
            .roots
            .contains(&PathBuf::from(system)));
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
