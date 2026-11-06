//! Keeps the current index on disk and swaps in new ones while searches continue.
//!
//! A rebuild replaces the snapshot file by rename. Searches still holding the old index keep
//! reading the old data through their mapping, which the tests below verify.
use crate::walk::{build_index, WalkConfig, WalkStats};
use crate::Index;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::{fs, io, thread};

const FILE_NAME: &str = "index.glint";

pub struct Store {
    path: PathBuf,
    current: RwLock<Arc<Index>>,
}

impl Store {
    /// Open the snapshot in `dir`, or start empty if it is missing or unreadable (for example
    /// from an older format). The next [`Store::replace`] writes a good one.
    pub fn open(dir: &Path) -> io::Result<Store> {
        fs::create_dir_all(dir)?;
        let path = dir.join(FILE_NAME);
        let current = Index::open(&path).unwrap_or_default();
        Ok(Store {
            path,
            current: RwLock::new(Arc::new(current)),
        })
    }

    /// The index to search. Searches holding it are unaffected by a later replace.
    pub fn current(&self) -> Arc<Index> {
        self.current.read().unwrap().clone()
    }

    /// Persist `ix` and make it current. On failure the current index stays in use.
    pub fn replace(&self, ix: &Index) -> io::Result<()> {
        ix.save(&self.path)?;
        let mapped = Arc::new(Index::open(&self.path)?);
        *self.current.write().unwrap() = mapped;
        Ok(())
    }
}

/// Rebuild the index on a background thread and swap it in when done.
pub fn refresh_in_background(
    store: Arc<Store>,
    cfg: WalkConfig,
) -> thread::JoinHandle<io::Result<WalkStats>> {
    thread::spawn(move || {
        let (ix, stats) = build_index(&cfg);
        store.replace(&ix)?;
        Ok(stats)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kind;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glint-store-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn index(name: &str) -> Index {
        let mut ix = Index::default();
        let root = ix.add_dir(None, "root");
        ix.push(name, root, Kind::File);
        ix
    }

    #[test]
    fn starts_empty_and_persists_across_reopen() {
        let dir = temp_dir("persist");
        let store = Store::open(&dir).unwrap();
        assert!(store.current().is_empty());
        store.replace(&index("alpha.txt")).unwrap();
        assert_eq!(store.current().name(0), "alpha.txt");
        drop(store);
        assert_eq!(Store::open(&dir).unwrap().current().name(0), "alpha.txt");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replaces_while_a_search_still_holds_the_old_index() {
        let dir = temp_dir("held");
        let store = Store::open(&dir).unwrap();
        store.replace(&index("old-name.txt")).unwrap();
        let held = store.current();
        store.replace(&index("new-name-longer.txt")).unwrap();
        assert_eq!(held.name(0), "old-name.txt");
        assert_eq!(held.search("old-name", 5).len(), 1);
        assert_eq!(store.current().name(0), "new-name-longer.txt");
        drop((held, store));
        assert_eq!(
            Store::open(&dir).unwrap().current().name(0),
            "new-name-longer.txt"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_snapshot_starts_empty_and_recovers() {
        let dir = temp_dir("corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(FILE_NAME), b"not a snapshot").unwrap();
        let store = Store::open(&dir).unwrap();
        assert!(store.current().is_empty());
        store.replace(&index("good.txt")).unwrap();
        assert_eq!(store.current().name(0), "good.txt");
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn background_refresh_swaps_in_a_walked_index() {
        let dir = temp_dir("bg");
        let tree = temp_dir("bg-tree");
        fs::create_dir_all(&tree).unwrap();
        fs::write(tree.join("found-me.txt"), b"x").unwrap();
        let store = Arc::new(Store::open(&dir).unwrap());
        let cfg = WalkConfig {
            roots: vec![tree.clone()],
            exclude_dirs: Vec::new(),
            threads: 1,
        };
        let stats = refresh_in_background(store.clone(), cfg)
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(stats, WalkStats::default());
        let ix = store.current();
        let hits = ix.search("found", 5);
        assert_eq!(ix.path(hits[0].id), tree.join("found-me.txt"));
        drop((ix, store));
        fs::remove_dir_all(dir).unwrap();
        fs::remove_dir_all(tree).unwrap();
    }
}
