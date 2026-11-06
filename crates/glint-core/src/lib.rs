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

/// Names scanned per parallel task.
const CHUNK: usize = 32_768;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub id: u32,
    pub score: u32,
}

#[derive(Default)]
pub struct Index {
    /// Original names, concatenated.
    names: String,
    /// End offset of each name in `names`.
    name_ends: Vec<u32>,
    /// Lower-cased names, each followed by a NUL terminator.
    folded: Vec<u8>,
    /// End offset (past the terminator) of each name in `folded`.
    fold_ends: Vec<u32>,
}

impl Index {
    pub fn push(&mut self, name: &str) {
        self.names.push_str(name);
        self.name_ends.push(self.names.len() as u32);
        let lower = name.to_lowercase();
        self.folded
            .extend_from_slice(lower.replace('\0', " ").as_bytes());
        self.folded.push(0);
        self.fold_ends.push(self.folded.len() as u32);
    }

    pub fn len(&self) -> usize {
        self.name_ends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.name_ends.is_empty()
    }

    /// Approximate heap bytes used by the index.
    pub fn heap_bytes(&self) -> usize {
        self.names.len() + self.folded.len() + 4 * (self.name_ends.len() + self.fold_ends.len())
    }

    pub fn name(&self, id: u32) -> &str {
        let i = id as usize;
        let start = if i == 0 {
            0
        } else {
            self.name_ends[i - 1] as usize
        };
        &self.names[start..self.name_ends[i] as usize]
    }

    fn fold_start(&self, i: usize) -> usize {
        if i == 0 {
            0
        } else {
            self.fold_ends[i - 1] as usize
        }
    }

    fn folded_name(&self, id: u32) -> &[u8] {
        let i = id as usize;
        &self.folded[self.fold_start(i)..self.fold_ends[i] as usize - 1]
    }

    /// Ids whose folded name contains `needle`. With `word_start`, the match must begin a word.
    fn candidates(&self, needle: &str, word_start: bool) -> Vec<u32> {
        let finder = memmem::Finder::new(needle.as_bytes());
        let n = self.len();
        (0..n.div_ceil(CHUNK))
            .into_par_iter()
            .flat_map_iter(|c| {
                let a = c * CHUNK;
                let b = ((c + 1) * CHUNK).min(n);
                let base = self.fold_start(a);
                let hay = &self.folded[base..self.fold_ends[b - 1] as usize];
                let mut out = Vec::new();
                let mut last = u32::MAX;
                for pos in finder.find_iter(hay) {
                    let abs = base + pos;
                    if word_start && abs > 0 {
                        let prev = self.folded[abs - 1];
                        if prev.is_ascii_alphanumeric() || prev >= 0x80 {
                            continue;
                        }
                    }
                    let id = self.fold_ends.partition_point(|&e| (e as usize) <= abs) as u32;
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

    #[test]
    fn matches_do_not_span_neighbouring_names() {
        let ix = index(&["ab", "cd"]);
        assert!(ix.search("bc", 10).is_empty());
    }
}
