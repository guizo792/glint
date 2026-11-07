//! The searchable view of the disk: the snapshot plus files seen since it was built.
//!
//! The snapshot is immutable, so a file watcher feeds new files into small in-memory overlays,
//! and results are checked against the disk so deleted files never appear. A rebuild starts a
//! fresh overlay first and drops the old one when done, so a file created mid-walk is not lost.
use crate::store::Store;
use crate::walk::{build_index, is_excluded_path, is_low_priority_path, WalkConfig, WalkStats};
use crate::{DirId, Index, Kind};
use notify::event::{EventKind, ModifyKind, RenameMode};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// New files remembered between rebuilds before asking for a rebuild instead.
const MAX_OVERLAY: usize = 50_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveHit {
    pub name: String,
    pub path: PathBuf,
    pub kind: Kind,
    pub score: u32,
}

#[derive(Default)]
struct Overlay {
    index: Index,
    dirs: HashMap<PathBuf, DirId>,
}

pub struct Live {
    store: Arc<Store>,
    excludes: Vec<String>,
    /// Newest first; the second exists only while a rebuild runs.
    overlays: RwLock<Vec<Overlay>>,
    needs_rebuild: AtomicBool,
}

impl Live {
    pub fn new(store: Arc<Store>, excludes: Vec<String>) -> Live {
        Live {
            store,
            excludes,
            overlays: RwLock::new(vec![Overlay::default()]),
            needs_rebuild: AtomicBool::new(false),
        }
    }

    /// Best hits that exist on disk and pass `keep`, best first. With `fuzzy`, also finds
    /// scattered matches, which is slower.
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        fuzzy: bool,
        keep: impl Fn(&str, &Path, Kind) -> bool,
    ) -> Vec<LiveHit> {
        // Over-fetch: some hits may be deleted files or fail the filter.
        let fetch = limit.saturating_mul(25).max(100);
        let mut hits = Vec::new();
        let run = |ix: &Index, hits: &mut Vec<LiveHit>| {
            let found = if fuzzy {
                ix.search_fuzzy(query, fetch)
            } else {
                ix.search(query, fetch)
            };
            hits.extend(found.into_iter().map(|h| LiveHit {
                name: ix.name(h.id).to_string(),
                path: ix.path(h.id),
                kind: ix.kind(h.id),
                score: h.score,
            }));
        };
        run(&self.store.current(), &mut hits);
        for overlay in self.overlays.read().unwrap().iter() {
            run(&overlay.index, &mut hits);
        }
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.path.cmp(&b.path)));
        let mut seen = HashSet::new();
        hits.into_iter()
            .filter(|h| keep(&h.name, &h.path, h.kind))
            .filter(|h| seen.insert(h.path.clone()))
            .filter(|h| h.path.symlink_metadata().is_ok())
            .take(limit)
            .collect()
    }

    /// Add a path seen by the watcher.
    pub fn record(&self, path: &Path) {
        if is_excluded_path(path, &self.excludes) {
            return;
        }
        let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
        else {
            return;
        };
        let (Some(parent_name), Ok(meta)) = (parent.to_str(), path.symlink_metadata()) else {
            return;
        };
        let mut overlays = self.overlays.write().unwrap();
        let overlay = &mut overlays[0];
        if overlay.index.len() >= MAX_OVERLAY {
            self.needs_rebuild.store(true, Ordering::Relaxed);
            return;
        }
        let dir = match overlay.dirs.get(parent) {
            Some(dir) => *dir,
            None => {
                let dir = if is_low_priority_path(parent) {
                    overlay.index.add_low_priority_dir(None, parent_name)
                } else {
                    overlay.index.add_dir(None, parent_name)
                };
                overlay.dirs.insert(parent.to_path_buf(), dir);
                dir
            }
        };
        let kind = if meta.is_dir() { Kind::Dir } else { Kind::File };
        overlay.index.push(name, dir, kind);
    }

    /// Whether the watcher lost track of changes, so a rebuild is due.
    pub fn take_needs_rebuild(&self) -> bool {
        self.needs_rebuild.swap(false, Ordering::Relaxed)
    }

    /// Walk the disk and replace the snapshot. Blocks the calling thread, not searches.
    pub fn rebuild(&self, cfg: &WalkConfig) -> io::Result<WalkStats> {
        self.overlays.write().unwrap().insert(0, Overlay::default());
        let (ix, stats) = build_index(cfg);
        let result = self.store.replace(&ix);
        // On failure keep the overlays: they still hold what the old snapshot lacks.
        if result.is_ok() {
            self.overlays.write().unwrap().truncate(1);
        }
        result.map(|()| stats)
    }

    pub fn entry_count(&self) -> usize {
        self.store.current().len()
            + self
                .overlays
                .read()
                .unwrap()
                .iter()
                .map(|o| o.index.len())
                .sum::<usize>()
    }
}

/// Watch `roots` for new files until the returned watcher is dropped. A root that cannot be
/// watched is skipped: it stays searchable and is refreshed by rebuilds.
pub fn watch(live: Arc<Live>, roots: &[PathBuf]) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else {
            live.needs_rebuild.store(true, Ordering::Relaxed);
            return;
        };
        if event.need_rescan() {
            live.needs_rebuild.store(true, Ordering::Relaxed);
        }
        let added = match event.kind {
            EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                event.paths.as_slice()
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                &event.paths[event.paths.len().saturating_sub(1)..]
            }
            _ => &[],
        };
        for path in added {
            live.record(path);
        }
    })?;
    for root in roots {
        let _ = watcher.watch(root, RecursiveMode::Recursive);
    }
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glint-live-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn setup(tag: &str) -> (PathBuf, PathBuf, Arc<Live>, WalkConfig) {
        let tree = temp_dir(&format!("{tag}-tree"));
        let data = temp_dir(&format!("{tag}-data"));
        let store = Arc::new(Store::open(&data).unwrap());
        let excludes = vec![".git".to_string()];
        let cfg = WalkConfig {
            roots: vec![tree.clone()],
            exclude_dirs: excludes.clone(),
            threads: 1,
        };
        (tree, data, Arc::new(Live::new(store, excludes)), cfg)
    }

    fn found(live: &Live, q: &str) -> Vec<String> {
        live.search(q, 10, false, |_, _, _| true)
            .into_iter()
            .map(|h| h.name)
            .collect()
    }

    #[test]
    fn finds_snapshot_files_and_hides_deleted_ones() {
        let (tree, _data, live, cfg) = setup("deleted");
        fs::write(tree.join("kept-file.txt"), b"x").unwrap();
        fs::write(tree.join("gone-file.txt"), b"x").unwrap();
        live.rebuild(&cfg).unwrap();
        assert_eq!(found(&live, "file").len(), 2);
        fs::remove_file(tree.join("gone-file.txt")).unwrap();
        assert_eq!(found(&live, "file"), ["kept-file.txt"]);
    }

    #[test]
    fn recorded_files_are_searchable_once_and_skip_excluded_folders() {
        let (tree, _data, live, cfg) = setup("record");
        live.rebuild(&cfg).unwrap();
        fs::create_dir(tree.join(".git")).unwrap();
        for p in ["brand-new.txt", ".git/brand-new-hidden.txt"] {
            fs::write(tree.join(p), b"x").unwrap();
            live.record(&tree.join(p));
        }
        assert_eq!(found(&live, "brand-new"), ["brand-new.txt"]);
        live.rebuild(&cfg).unwrap();
        assert_eq!(found(&live, "brand-new"), ["brand-new.txt"]);
    }

    #[test]
    fn filter_can_reject_hits_without_shrinking_the_result_below_limit() {
        let (tree, _data, live, cfg) = setup("filter");
        for i in 0..30 {
            fs::write(tree.join(format!("doc-{i}.txt")), b"x").unwrap();
        }
        fs::write(tree.join("doc-z.pdf"), b"x").unwrap();
        live.rebuild(&cfg).unwrap();
        let hits = live.search("doc", 1, false, |n, _, _| n.ends_with(".pdf"));
        assert_eq!(hits[0].name, "doc-z.pdf");
    }

    #[test]
    fn watcher_makes_a_new_file_searchable_within_two_seconds() {
        let (tree, _data, live, cfg) = setup("watch");
        live.rebuild(&cfg).unwrap();
        let _watcher = watch(live.clone(), std::slice::from_ref(&tree)).unwrap();
        fs::create_dir(tree.join("sub")).unwrap();
        fs::write(tree.join("sub").join("watched-file.txt"), b"x").unwrap();
        let start = Instant::now();
        while found(&live, "watched-file").is_empty() {
            assert!(start.elapsed() < Duration::from_secs(2), "not seen in 2 s");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn watcher_sees_renames_under_the_new_name() {
        let (tree, _data, live, cfg) = setup("rename");
        fs::write(tree.join("before-name.txt"), b"x").unwrap();
        live.rebuild(&cfg).unwrap();
        let _watcher = watch(live.clone(), std::slice::from_ref(&tree)).unwrap();
        fs::rename(tree.join("before-name.txt"), tree.join("after-name.txt")).unwrap();
        let start = Instant::now();
        while found(&live, "after-name").is_empty() {
            assert!(start.elapsed() < Duration::from_secs(2), "not seen in 2 s");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(found(&live, "before-name").is_empty());
    }
}
