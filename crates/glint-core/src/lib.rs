//! Glint core: compact name index with two-stage search.
//!
//! Stage 1 finds *candidates* with a SIMD substring scan over a case-folded arena
//! (word-start matches only for 1–2 character queries). Stage 2 fuzzy-scores only
//! those candidates. A slower full subsequence scan, [`Index::search_fuzzy`], exists
//! for callers that want to stream in looser matches after the fast results.
use memchr::memmem;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use rayon::prelude::*;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(not(target_endian = "little"))]
compile_error!("snapshot format is little-endian only");

/// Names scanned per parallel task.
const CHUNK: usize = 32_768;

const MAGIC: &[u8; 8] = b"GLNTIDX2";
const HEADER: usize = 48;

/// `dir_parents` value for a root folder.
const NO_PARENT: u32 = u32::MAX;
/// Set in an `entry_dirs` value when the entry is itself a folder.
const IS_DIR_BIT: u32 = 1 << 31;

/// Byte storage: heap while building, or a read-only window into a memory-mapped snapshot.
enum Bytes {
    Owned(Vec<u8>),
    Mapped(Arc<memmap2::Mmap>, usize, usize),
}

impl Bytes {
    fn get(&self) -> &[u8] {
        match self {
            Bytes::Owned(v) => v,
            Bytes::Mapped(m, off, len) => &m[*off..*off + *len],
        }
    }
    fn owned_mut(&mut self) -> &mut Vec<u8> {
        match self {
            Bytes::Owned(v) => v,
            Bytes::Mapped(..) => panic!("index opened from a snapshot is read-only"),
        }
    }
    fn heap_bytes(&self) -> usize {
        match self {
            Bytes::Owned(v) => v.len(),
            Bytes::Mapped(..) => 0,
        }
    }
}

/// `u32` offset table, same two forms as [`Bytes`].
enum Words {
    Owned(Vec<u32>),
    Mapped(Arc<memmap2::Mmap>, usize, usize),
}

impl Words {
    fn get(&self) -> &[u32] {
        match self {
            Words::Owned(v) => v,
            Words::Mapped(m, off, count) => bytemuck::cast_slice(&m[*off..*off + 4 * *count]),
        }
    }
    fn owned_mut(&mut self) -> &mut Vec<u32> {
        match self {
            Words::Owned(v) => v,
            Words::Mapped(..) => panic!("index opened from a snapshot is read-only"),
        }
    }
    fn heap_bytes(&self) -> usize {
        match self {
            Words::Owned(v) => 4 * v.len(),
            Words::Mapped(..) => 0,
        }
    }
}

fn align8(n: usize) -> usize {
    (n + 7) & !7
}

/// Section offsets of a snapshot file; each section starts 8-byte aligned.
struct Layout {
    count: usize,
    dir_count: usize,
    name_ends: usize,
    fold_ends: usize,
    entry_dirs: usize,
    dir_parents: usize,
    dir_name_ends: usize,
    names: usize,
    folded: usize,
    dir_names: usize,
    names_len: usize,
    folded_len: usize,
    dir_names_len: usize,
    total: usize,
}

fn layout(
    count: usize,
    dir_count: usize,
    names_len: usize,
    folded_len: usize,
    dir_names_len: usize,
) -> Layout {
    let name_ends = HEADER;
    let fold_ends = align8(name_ends + 4 * count);
    let entry_dirs = align8(fold_ends + 4 * count);
    let dir_parents = align8(entry_dirs + 4 * count);
    let dir_name_ends = align8(dir_parents + 4 * dir_count);
    let names = align8(dir_name_ends + 4 * dir_count);
    let folded = align8(names + names_len);
    let dir_names = align8(folded + folded_len);
    Layout {
        count,
        dir_count,
        name_ends,
        fold_ends,
        entry_dirs,
        dir_parents,
        dir_name_ends,
        names,
        folded,
        dir_names,
        names_len,
        folded_len,
        dir_names_len,
        total: align8(dir_names + dir_names_len),
    }
}

fn corrupt(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("bad snapshot: {msg}"))
}

/// Validate the header (and total size) of snapshot bytes.
fn parse_header(bytes: &[u8]) -> io::Result<Layout> {
    if bytes.len() < HEADER || &bytes[..8] != MAGIC {
        return Err(corrupt("missing magic"));
    }
    let word = |i: usize| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap()) as usize;
    let (count, dirs, nl, fl, dl) = (word(8), word(16), word(24), word(32), word(40));
    // Reject absurd sizes before they can overflow the layout arithmetic.
    if [count, dirs, nl, fl, dl]
        .iter()
        .any(|&v| v > u32::MAX as usize)
    {
        return Err(corrupt("size out of range"));
    }
    let lay = layout(count, dirs, nl, fl, dl);
    if bytes.len() != lay.total {
        return Err(corrupt("length mismatch"));
    }
    Ok(lay)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub id: u32,
    pub score: u32,
}

/// A folder in the index's folder table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
}

/// A name index. Build with [`Index::push`], persist with [`Index::save`], and reopen
/// read-only with [`Index::open`] (memory-mapped) or [`Index::read`] (heap copy).
pub struct Index {
    /// Original names (UTF-8), concatenated.
    names: Bytes,
    /// End offset of each name in `names`.
    name_ends: Words,
    /// Lower-cased names, each followed by a NUL terminator.
    folded: Bytes,
    /// End offset (past the terminator) of each name in `folded`.
    fold_ends: Words,
    /// Containing folder of each entry, with [`IS_DIR_BIT`] set for folder entries.
    entry_dirs: Words,
    /// Parent of each folder, always a lower id, or [`NO_PARENT`] for a root.
    dir_parents: Words,
    /// End offset of each folder's own name in `dir_names`.
    dir_name_ends: Words,
    /// Folder names (a root's name is its full path, e.g. `C:\`), concatenated.
    dir_names: Bytes,
}

impl Default for Index {
    fn default() -> Self {
        Index {
            names: Bytes::Owned(Vec::new()),
            name_ends: Words::Owned(Vec::new()),
            folded: Bytes::Owned(Vec::new()),
            fold_ends: Words::Owned(Vec::new()),
            entry_dirs: Words::Owned(Vec::new()),
            dir_parents: Words::Owned(Vec::new()),
            dir_name_ends: Words::Owned(Vec::new()),
            dir_names: Bytes::Owned(Vec::new()),
        }
    }
}

impl Index {
    /// Add a folder to the folder table. This does not make it a search result; push it
    /// as a [`Kind::Dir`] entry for that. Panics on an unknown parent or a read-only index.
    pub fn add_dir(&mut self, parent: Option<DirId>, name: &str) -> DirId {
        let id = self.dir_parents.get().len() as u32;
        assert!(id < IS_DIR_BIT, "too many folders");
        let parent = parent.map_or(NO_PARENT, |p| {
            assert!(p.0 < id, "unknown parent folder");
            p.0
        });
        self.dir_parents.owned_mut().push(parent);
        let dir_names = self.dir_names.owned_mut();
        dir_names.extend_from_slice(name.as_bytes());
        self.dir_name_ends.owned_mut().push(dir_names.len() as u32);
        DirId(id)
    }

    /// Append a searchable entry. Panics on an unknown folder or a read-only index.
    pub fn push(&mut self, name: &str, dir: DirId, kind: Kind) {
        assert!(
            (dir.0 as usize) < self.dir_parents.get().len(),
            "unknown folder"
        );
        self.entry_dirs.owned_mut().push(match kind {
            Kind::File => dir.0,
            Kind::Dir => dir.0 | IS_DIR_BIT,
        });
        let names = self.names.owned_mut();
        names.extend_from_slice(name.as_bytes());
        self.name_ends.owned_mut().push(names.len() as u32);
        let lower = name.to_lowercase();
        let folded = self.folded.owned_mut();
        folded.extend_from_slice(lower.replace('\0', " ").as_bytes());
        folded.push(0);
        self.fold_ends.owned_mut().push(folded.len() as u32);
    }

    pub fn len(&self) -> usize {
        self.name_ends.get().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes held on the heap (zero for a memory-mapped index).
    pub fn heap_bytes(&self) -> usize {
        self.names.heap_bytes()
            + self.folded.heap_bytes()
            + self.dir_names.heap_bytes()
            + self.name_ends.heap_bytes()
            + self.fold_ends.heap_bytes()
            + self.entry_dirs.heap_bytes()
            + self.dir_parents.heap_bytes()
            + self.dir_name_ends.heap_bytes()
    }

    /// Number of folders in the folder table.
    pub fn dir_count(&self) -> usize {
        self.dir_parents.get().len()
    }

    /// Bytes backed by a mapped snapshot file (zero for a heap index).
    pub fn mapped_bytes(&self) -> usize {
        match &self.names {
            Bytes::Mapped(m, ..) => m.len(),
            Bytes::Owned(_) => 0,
        }
    }

    /// Write a snapshot atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let (names, folded, dir_names) =
            (self.names.get(), self.folded.get(), self.dir_names.get());
        let (ne, fe) = (self.name_ends.get(), self.fold_ends.get());
        let (ed, dp, dne) = (
            self.entry_dirs.get(),
            self.dir_parents.get(),
            self.dir_name_ends.get(),
        );
        let lay = layout(
            ne.len(),
            dp.len(),
            names.len(),
            folded.len(),
            dir_names.len(),
        );
        let tmp = path.with_extension("tmp");
        let mut w = io::BufWriter::new(std::fs::File::create(&tmp)?);
        w.write_all(MAGIC)?;
        for v in [
            ne.len(),
            dp.len(),
            names.len(),
            folded.len(),
            dir_names.len(),
        ] {
            w.write_all(&(v as u64).to_le_bytes())?;
        }
        let mut pos = HEADER;
        for (start, bytes) in [
            (lay.name_ends, bytemuck::cast_slice::<u32, u8>(ne)),
            (lay.fold_ends, bytemuck::cast_slice::<u32, u8>(fe)),
            (lay.entry_dirs, bytemuck::cast_slice::<u32, u8>(ed)),
            (lay.dir_parents, bytemuck::cast_slice::<u32, u8>(dp)),
            (lay.dir_name_ends, bytemuck::cast_slice::<u32, u8>(dne)),
            (lay.names, names),
            (lay.folded, folded),
            (lay.dir_names, dir_names),
        ] {
            w.write_all(&vec![0; start - pos])?; // alignment padding
            w.write_all(bytes)?;
            pos = start + bytes.len();
        }
        w.write_all(&vec![0; lay.total - pos])?;
        w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        std::fs::rename(tmp, path)
    }

    /// Open a snapshot memory-mapped and read-only: nothing is parsed or copied, and the OS
    /// pages data in on demand. The file must not be modified while open ([`Index::save`]
    /// replaces it atomically, which is safe).
    pub fn open(path: &Path) -> io::Result<Index> {
        let file = std::fs::File::open(path)?;
        // SAFETY: the snapshot is only ever replaced by rename, never modified in place.
        let map = Arc::new(unsafe { memmap2::Mmap::map(&file)? });
        let lay = parse_header(&map)?;
        let ix = Index {
            name_ends: Words::Mapped(map.clone(), lay.name_ends, lay.count),
            fold_ends: Words::Mapped(map.clone(), lay.fold_ends, lay.count),
            entry_dirs: Words::Mapped(map.clone(), lay.entry_dirs, lay.count),
            dir_parents: Words::Mapped(map.clone(), lay.dir_parents, lay.dir_count),
            dir_name_ends: Words::Mapped(map.clone(), lay.dir_name_ends, lay.dir_count),
            names: Bytes::Mapped(map.clone(), lay.names, lay.names_len),
            folded: Bytes::Mapped(map.clone(), lay.folded, lay.folded_len),
            dir_names: Bytes::Mapped(map.clone(), lay.dir_names, lay.dir_names_len),
        };
        ix.validate(&lay)?;
        Ok(ix)
    }

    /// Load a snapshot into heap memory (fallback where mapping is unsuitable).
    pub fn read(path: &Path) -> io::Result<Index> {
        let bytes = std::fs::read(path)?;
        let lay = parse_header(&bytes)?;
        let words = |off: usize, n: usize| -> Vec<u32> {
            let (chunks, _) = bytes[off..off + 4 * n].as_chunks::<4>();
            chunks.iter().map(|c| u32::from_le_bytes(*c)).collect()
        };
        let ix = Index {
            name_ends: Words::Owned(words(lay.name_ends, lay.count)),
            fold_ends: Words::Owned(words(lay.fold_ends, lay.count)),
            entry_dirs: Words::Owned(words(lay.entry_dirs, lay.count)),
            dir_parents: Words::Owned(words(lay.dir_parents, lay.dir_count)),
            dir_name_ends: Words::Owned(words(lay.dir_name_ends, lay.dir_count)),
            names: Bytes::Owned(bytes[lay.names..lay.names + lay.names_len].to_vec()),
            folded: Bytes::Owned(bytes[lay.folded..lay.folded + lay.folded_len].to_vec()),
            dir_names: Bytes::Owned(
                bytes[lay.dir_names..lay.dir_names + lay.dir_names_len].to_vec(),
            ),
        };
        ix.validate(&lay)?;
        Ok(ix)
    }

    /// Integrity checks: offset tables must end exactly at their data, and folder links must
    /// be in range and point backwards, so [`Index::path`] cannot loop or index out of range.
    fn validate(&self, lay: &Layout) -> io::Result<()> {
        let last = |t: &[u32]| t.last().copied().unwrap_or(0) as usize;
        if last(self.name_ends.get()) != lay.names_len
            || last(self.fold_ends.get()) != lay.folded_len
            || last(self.dir_name_ends.get()) != lay.dir_names_len
        {
            return Err(corrupt("offsets do not match data"));
        }
        let parents = self.dir_parents.get();
        if parents
            .iter()
            .enumerate()
            .any(|(i, &p)| p != NO_PARENT && p as usize >= i)
        {
            return Err(corrupt("bad folder parent"));
        }
        if self
            .entry_dirs
            .get()
            .iter()
            .any(|&d| (d & !IS_DIR_BIT) as usize >= parents.len())
        {
            return Err(corrupt("bad entry folder"));
        }
        Ok(())
    }

    /// Whether an entry is a file or a folder.
    pub fn kind(&self, id: u32) -> Kind {
        if self.entry_dirs.get()[id as usize] & IS_DIR_BIT != 0 {
            Kind::Dir
        } else {
            Kind::File
        }
    }

    /// Full path of an entry, rebuilt from the folder table.
    pub fn path(&self, id: u32) -> PathBuf {
        let parents = self.dir_parents.get();
        let mut parts = vec![self.name(id)];
        let mut dir = self.entry_dirs.get()[id as usize] & !IS_DIR_BIT;
        while dir != NO_PARENT {
            parts.push(self.dir_name(dir));
            dir = parents[dir as usize];
        }
        parts.iter().rev().collect()
    }

    fn dir_name(&self, dir: u32) -> &str {
        let (ends, names) = (self.dir_name_ends.get(), self.dir_names.get());
        let i = dir as usize;
        let start = if i == 0 { 0 } else { ends[i - 1] as usize };
        std::str::from_utf8(&names[start..ends[i] as usize]).unwrap_or("")
    }

    pub fn name(&self, id: u32) -> &str {
        let (ends, names) = (self.name_ends.get(), self.names.get());
        let i = id as usize;
        let start = if i == 0 { 0 } else { ends[i - 1] as usize };
        std::str::from_utf8(&names[start..ends[i] as usize]).unwrap_or("")
    }

    fn folded_name(&self, id: u32) -> &[u8] {
        let (ends, folded) = (self.fold_ends.get(), self.folded.get());
        let i = id as usize;
        let start = if i == 0 { 0 } else { ends[i - 1] as usize };
        &folded[start..ends[i] as usize - 1]
    }

    /// Ids whose folded name contains `needle`. With `word_start`, the match must begin a word.
    fn candidates(&self, needle: &str, word_start: bool) -> Vec<u32> {
        let finder = memmem::Finder::new(needle.as_bytes());
        let (folded, fold_ends) = (self.folded.get(), self.fold_ends.get());
        let n = self.len();
        (0..n.div_ceil(CHUNK))
            .into_par_iter()
            .flat_map_iter(|c| {
                let a = c * CHUNK;
                let b = ((c + 1) * CHUNK).min(n);
                let base = if a == 0 { 0 } else { fold_ends[a - 1] as usize };
                let hay = &folded[base..fold_ends[b - 1] as usize];
                let mut out = Vec::new();
                let mut last = u32::MAX;
                for pos in finder.find_iter(hay) {
                    let abs = base + pos;
                    if word_start && abs > 0 {
                        let prev = folded[abs - 1];
                        if prev.is_ascii_alphanumeric() || prev >= 0x80 {
                            continue;
                        }
                    }
                    let id = fold_ends.partition_point(|&e| (e as usize) <= abs) as u32;
                    if id != last {
                        out.push(id);
                        last = id;
                    }
                }
                out
            })
            .collect()
    }

    /// Fast path: substring candidates, fuzzy-scored. Best first, ties broken by id.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let folded_q = query.trim().to_lowercase();
        let terms: Vec<&str> = folded_q.split_whitespace().collect();
        let Some(anchor) = terms.iter().max_by_key(|t| t.chars().count()) else {
            return Vec::new();
        };
        let short = anchor.chars().count() < 3;
        let mut cands = self.candidates(anchor, short);
        if short && terms.len() == 1 {
            return self.score_cheap(&cands, anchor, limit);
        }
        if terms.len() > 1 {
            cands = cands
                .into_par_iter()
                .filter(|&id| {
                    let name = self.folded_name(id);
                    terms
                        .iter()
                        .all(|t| memmem::find(name, t.as_bytes()).is_some())
                })
                .collect();
        }
        self.score(&cands, query.trim(), limit)
    }

    /// Slow path: full subsequence scan of every name (the original brute-force behaviour).
    /// Intended to run after [`Index::search`] when it returned too few results.
    pub fn search_fuzzy(&self, query: &str, limit: usize) -> Vec<Hit> {
        let all: Vec<u32> = (0..self.len() as u32).collect();
        self.score(&all, query.trim(), limit)
    }

    /// 1–2 character queries match huge numbers of names, so skip the fuzzy matcher:
    /// name-prefix matches first, then shorter names.
    fn score_cheap(&self, ids: &[u32], folded_query: &str, limit: usize) -> Vec<Hit> {
        let q = folded_query.as_bytes();
        let mut hits: Vec<Hit> = ids
            .par_chunks(CHUNK)
            .map(|chunk| {
                let mut out: Vec<Hit> = chunk
                    .iter()
                    .map(|&id| {
                        let name = self.folded_name(id);
                        let base = if name.starts_with(q) { 100_000 } else { 50_000 };
                        Hit {
                            id,
                            score: base - name.len().min(255) as u32,
                        }
                    })
                    .collect();
                keep_top(&mut out, limit);
                out
            })
            .flatten()
            .collect();
        keep_top(&mut hits, limit);
        hits
    }

    fn score(&self, ids: &[u32], query: &str, limit: usize) -> Vec<Hit> {
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }
        let pat = Pattern::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut hits: Vec<Hit> = ids
            .par_chunks(CHUNK)
            .map_init(
                || (Matcher::new(Config::DEFAULT), Vec::<char>::new()),
                |(m, buf), chunk| {
                    let mut out: Vec<Hit> = chunk
                        .iter()
                        .filter_map(|&id| {
                            pat.score(Utf32Str::new(self.name(id), buf), m)
                                .map(|score| Hit { id, score })
                        })
                        .collect();
                    keep_top(&mut out, limit);
                    out
                },
            )
            .flatten()
            .collect();
        keep_top(&mut hits, limit);
        hits
    }
}

fn better(a: &Hit, b: &Hit) -> std::cmp::Ordering {
    b.score.cmp(&a.score).then(a.id.cmp(&b.id))
}

/// Reduce to the best `k` hits, sorted best first.
fn keep_top(v: &mut Vec<Hit>, k: usize) {
    if v.len() > k {
        v.select_nth_unstable_by(k, better);
        v.truncate(k);
    }
    v.sort_unstable_by(better);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(names: &[&str]) -> Index {
        let mut ix = Index::default();
        let root = ix.add_dir(None, "root");
        for n in names {
            ix.push(n, root, Kind::File);
        }
        ix
    }

    fn names(ix: &Index, hits: &[Hit]) -> Vec<String> {
        hits.iter().map(|h| ix.name(h.id).to_string()).collect()
    }

    #[test]
    fn ranks_prefix_over_inner_match() {
        let ix = index(&["interstellar.mp4", "Terminal", "notes.txt"]);
        let got = names(&ix, &ix.search("ter", 10));
        assert_eq!(got.first().map(String::as_str), Some("Terminal"));
        assert!(got.contains(&"interstellar.mp4".to_string()));
        assert!(!got.contains(&"notes.txt".to_string()));
    }

    #[test]
    fn is_case_insensitive() {
        let ix = index(&["README.md", "readme.txt"]);
        assert_eq!(ix.search("ReadMe", 10).len(), 2);
    }

    #[test]
    fn matches_non_ascii_names() {
        let ix = index(&["résumé.pdf", "تقرير.docx", "文档.txt", "plain.txt"]);
        assert_eq!(names(&ix, &ix.search("résu", 10)), ["résumé.pdf"]);
        assert_eq!(names(&ix, &ix.search("تقر", 10)), ["تقرير.docx"]);
        assert_eq!(names(&ix, &ix.search("文档", 10)), ["文档.txt"]);
    }

    #[test]
    fn short_queries_match_word_starts_only() {
        let ix = index(&["alpha", "banana", "big apple", "my_app"]);
        let mut got = names(&ix, &ix.search("a", 10));
        got.sort();
        assert_eq!(got, ["alpha", "big apple", "my_app"]);
    }

    #[test]
    fn short_queries_prefer_name_prefix_then_shorter_names() {
        let ix = index(&["my_app", "alphabet", "alpha", "big apple"]);
        assert_eq!(
            names(&ix, &ix.search("a", 10)),
            ["alpha", "alphabet", "my_app", "big apple"]
        );
    }

    #[test]
    fn multiple_terms_are_anded() {
        let ix = index(&["report_2024.pdf", "report.txt", "invoice.pdf"]);
        assert_eq!(
            names(&ix, &ix.search("report pdf", 10)),
            ["report_2024.pdf"]
        );
    }

    #[test]
    fn fast_path_skips_scattered_matches_but_fuzzy_path_finds_them() {
        let ix = index(&["meeting_notes.txt", "other.txt"]);
        assert!(ix.search("mtgnts", 10).is_empty());
        assert_eq!(
            names(&ix, &ix.search_fuzzy("mtgnts", 10)),
            ["meeting_notes.txt"]
        );
    }

    #[test]
    fn respects_limit_and_is_deterministic() {
        let owned: Vec<String> = (0..500).map(|i| format!("file_{i}.txt")).collect();
        let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
        let ix = index(&refs);
        let a = ix.search("file", 20);
        assert_eq!(a.len(), 20);
        assert_eq!(a, ix.search("file", 20));
    }

    #[test]
    fn empty_and_blank_queries_return_nothing() {
        let ix = index(&["a.txt"]);
        assert!(ix.search("", 10).is_empty());
        assert!(ix.search("   ", 10).is_empty());
    }

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("glint-test-{}-{tag}.idx", std::process::id()))
    }

    #[test]
    fn snapshot_round_trips_through_mmap_and_heap() {
        let ix = index(&["résumé.pdf", "Terminal", "تقرير.docx", "文档.txt", "a"]);
        let path = temp_path("rt");
        ix.save(&path).unwrap();
        for loaded in [Index::open(&path).unwrap(), Index::read(&path).unwrap()] {
            assert_eq!(loaded.len(), 5);
            for q in ["résu", "ter", "تقر", "文档", "a"] {
                assert_eq!(loaded.search(q, 10), ix.search(q, 10), "query {q}");
            }
            assert_eq!(loaded.name(1), "Terminal");
        }
        assert_eq!(Index::open(&path).unwrap().heap_bytes(), 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn empty_index_round_trips() {
        let path = temp_path("empty");
        Index::default().save(&path).unwrap();
        assert!(Index::open(&path).unwrap().is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn corrupt_snapshots_are_rejected_not_trusted() {
        let path = temp_path("bad");
        let ix = index(&["alpha", "beta"]);
        ix.save(&path).unwrap();
        let good = std::fs::read(&path).unwrap();
        for (label, bad) in [
            ("truncated", good[..good.len() - 8].to_vec()),
            ("bad magic", [b"XXXXXXXX".as_slice(), &good[8..]].concat()),
            ("too short", good[..10].to_vec()),
        ] {
            std::fs::write(&path, bad).unwrap();
            assert!(Index::open(&path).is_err(), "{label} accepted by open");
            assert!(Index::read(&path).is_err(), "{label} accepted by read");
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    #[should_panic(expected = "read-only")]
    fn pushing_to_a_mapped_index_panics() {
        let path = temp_path("ro");
        index(&["a"]).save(&path).unwrap();
        let mut ix = Index::open(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        ix.push("b", DirId(0), Kind::File);
    }

    fn tree() -> Index {
        let mut ix = Index::default();
        let root = ix.add_dir(None, "root");
        let docs = ix.add_dir(Some(root), "Documents");
        let reports = ix.add_dir(Some(docs), "تقارير");
        ix.push("Documents", root, Kind::Dir);
        ix.push("تقارير", docs, Kind::Dir);
        ix.push("résumé.pdf", reports, Kind::File);
        ix.push("top.txt", root, Kind::File);
        ix
    }

    fn assert_tree(ix: &Index) {
        let expect = |parts: &[&str]| parts.iter().collect::<PathBuf>();
        assert_eq!(ix.dir_count(), 3);
        assert_eq!(ix.path(0), expect(&["root", "Documents"]));
        assert_eq!(ix.path(1), expect(&["root", "Documents", "تقارير"]));
        assert_eq!(
            ix.path(2),
            expect(&["root", "Documents", "تقارير", "résumé.pdf"])
        );
        assert_eq!(ix.path(3), expect(&["root", "top.txt"]));
        assert_eq!(ix.kind(0), Kind::Dir);
        assert_eq!(ix.kind(2), Kind::File);
    }

    #[test]
    fn rebuilds_paths_and_kinds() {
        assert_tree(&tree());
    }

    #[test]
    fn paths_and_kinds_survive_snapshots() {
        let path = temp_path("tree");
        tree().save(&path).unwrap();
        assert_tree(&Index::open(&path).unwrap());
        assert_tree(&Index::read(&path).unwrap());
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn drive_root_joins_into_a_windows_path() {
        let mut ix = Index::default();
        let root = ix.add_dir(None, "C:\\");
        let users = ix.add_dir(Some(root), "Users");
        ix.push("a.txt", users, Kind::File);
        assert_eq!(ix.path(0), PathBuf::from("C:\\Users\\a.txt"));
    }

    #[test]
    fn folders_are_searchable_only_when_pushed_as_entries() {
        let ix = tree();
        let got = ix.search("documents", 10);
        assert_eq!(got.len(), 1);
        assert_eq!(ix.kind(got[0].id), Kind::Dir);
        assert!(ix.search("root", 10).is_empty());
    }

    #[test]
    fn corrupt_folder_links_are_rejected() {
        let path = temp_path("links");
        let ix = tree();
        ix.save(&path).unwrap();
        let good = std::fs::read(&path).unwrap();
        let lay = parse_header(&good).unwrap();
        for (label, offset) in [
            ("parent pointing forward", lay.dir_parents),
            ("entry folder out of range", lay.entry_dirs),
        ] {
            let mut bad = good.clone();
            bad[offset..offset + 4].copy_from_slice(&7u32.to_le_bytes());
            std::fs::write(&path, bad).unwrap();
            assert!(Index::open(&path).is_err(), "{label} accepted by open");
            assert!(Index::read(&path).is_err(), "{label} accepted by read");
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn matches_do_not_span_neighbouring_names() {
        let ix = index(&["ab", "cd"]);
        assert!(ix.search("bc", 10).is_empty());
    }
}
