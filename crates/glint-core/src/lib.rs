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
use std::path::Path;
use std::sync::Arc;

#[cfg(not(target_endian = "little"))]
compile_error!("snapshot format is little-endian only");

/// Names scanned per parallel task.
const CHUNK: usize = 32_768;

const MAGIC: &[u8; 8] = b"GLNTIDX1";
const HEADER: usize = 32;

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
}

fn align8(n: usize) -> usize {
    (n + 7) & !7
}

/// Section offsets of a snapshot file; each section starts 8-byte aligned.
struct Layout {
    name_ends: usize,
    fold_ends: usize,
    names: usize,
    folded: usize,
    names_len: usize,
    folded_len: usize,
    total: usize,
}

fn layout(count: usize, names_len: usize, folded_len: usize) -> Layout {
    let name_ends = HEADER;
    let fold_ends = align8(name_ends + 4 * count);
    let names = align8(fold_ends + 4 * count);
    let folded = align8(names + names_len);
    Layout {
        name_ends,
        fold_ends,
        names,
        folded,
        names_len,
        folded_len,
        total: align8(folded + folded_len),
    }
}

fn corrupt(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("bad snapshot: {msg}"))
}

/// Validate the header (and total size) of snapshot bytes; returns `(count, layout)`.
fn parse_header(bytes: &[u8]) -> io::Result<(usize, Layout)> {
    if bytes.len() < HEADER || &bytes[..8] != MAGIC {
        return Err(corrupt("missing magic"));
    }
    let word = |i: usize| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap()) as usize;
    let (count, nl, fl) = (word(8), word(16), word(24));
    // Reject absurd sizes before they can overflow the layout arithmetic.
    if count > u32::MAX as usize || nl > u32::MAX as usize || fl > u32::MAX as usize {
        return Err(corrupt("size out of range"));
    }
    let lay = layout(count, nl, fl);
    if bytes.len() != lay.total {
        return Err(corrupt("length mismatch"));
    }
    Ok((count, lay))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub id: u32,
    pub score: u32,
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
}

impl Default for Index {
    fn default() -> Self {
        Index {
            names: Bytes::Owned(Vec::new()),
            name_ends: Words::Owned(Vec::new()),
            folded: Bytes::Owned(Vec::new()),
            fold_ends: Words::Owned(Vec::new()),
        }
    }
}

impl Index {
    /// Append a name. Panics on an index opened from a snapshot (those are read-only).
    pub fn push(&mut self, name: &str) {
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
        let heap = |owned: bool, n: usize| if owned { n } else { 0 };
        heap(
            matches!(self.names, Bytes::Owned(_)),
            self.names.get().len(),
        ) + heap(
            matches!(self.folded, Bytes::Owned(_)),
            self.folded.get().len(),
        ) + heap(matches!(self.name_ends, Words::Owned(_)), 4 * self.len())
            + heap(matches!(self.fold_ends, Words::Owned(_)), 4 * self.len())
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
        let (names, folded) = (self.names.get(), self.folded.get());
        let (ne, fe) = (self.name_ends.get(), self.fold_ends.get());
        let lay = layout(ne.len(), names.len(), folded.len());
        let tmp = path.with_extension("tmp");
        let mut w = io::BufWriter::new(std::fs::File::create(&tmp)?);
        w.write_all(MAGIC)?;
        for v in [ne.len(), names.len(), folded.len()] {
            w.write_all(&(v as u64).to_le_bytes())?;
        }
        let mut pos = HEADER;
        for (start, bytes) in [
            (lay.name_ends, bytemuck::cast_slice::<u32, u8>(ne)),
            (lay.fold_ends, bytemuck::cast_slice::<u32, u8>(fe)),
            (lay.names, names),
            (lay.folded, folded),
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
        let (count, lay) = parse_header(&map)?;
        let ix = Index {
            name_ends: Words::Mapped(map.clone(), lay.name_ends, count),
            fold_ends: Words::Mapped(map.clone(), lay.fold_ends, count),
            names: Bytes::Mapped(map.clone(), lay.names, lay.names_len),
            folded: Bytes::Mapped(map.clone(), lay.folded, lay.folded_len),
        };
        ix.validate(count, &lay, &map[..])?;
        Ok(ix)
    }

    /// Load a snapshot into heap memory (fallback where mapping is unsuitable).
    pub fn read(path: &Path) -> io::Result<Index> {
        let bytes = std::fs::read(path)?;
        let (count, lay) = parse_header(&bytes)?;
        let words = |off: usize| -> Vec<u32> {
            let (chunks, _) = bytes[off..off + 4 * count].as_chunks::<4>();
            chunks.iter().map(|c| u32::from_le_bytes(*c)).collect()
        };
        let (ne, fe) = (words(lay.name_ends), words(lay.fold_ends));
        let ix = Index {
            name_ends: Words::Owned(ne),
            fold_ends: Words::Owned(fe),
            names: Bytes::Owned(bytes[lay.names..lay.names + lay.names_len].to_vec()),
            folded: Bytes::Owned(bytes[lay.folded..lay.folded + lay.folded_len].to_vec()),
        };
        ix.validate(count, &lay, &bytes)?;
        Ok(ix)
    }

    /// Cheap integrity checks: the offset tables must end exactly at their data, with the
    /// final offsets inside the file, so lookups cannot index out of range.
    fn validate(&self, count: usize, lay: &Layout, file: &[u8]) -> io::Result<()> {
        let (ne, fe) = (self.name_ends.get(), self.fold_ends.get());
        if ne.len() != count || fe.len() != count {
            return Err(corrupt("table size"));
        }
        let last = |t: &[u32]| t.last().copied().unwrap_or(0) as usize;
        if last(ne) != lay.names_len || last(fe) != lay.folded_len || file.len() != lay.total {
            return Err(corrupt("offsets do not match data"));
        }
        Ok(())
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
        for n in names {
            ix.push(n);
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
        ix.push("b");
    }

    #[test]
    fn matches_do_not_span_neighbouring_names() {
        let ix = index(&["ab", "cd"]);
        assert!(ix.search("bc", 10).is_empty());
    }
}
