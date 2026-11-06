//! Prototype: contiguous-arena name index + parallel fuzzy search,
//! with a char-bitmask prefilter and keystroke-to-keystroke narrowing.
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use rayon::prelude::*;

#[derive(Default)]
pub struct Index {
    arena: String,
    /// (start, len) byte ranges into `arena`.
    spans: Vec<(u32, u32)>,
    /// Per-name bitmask: bits 0..36 = ascii [a-z0-9] present (case-folded), bit 63 = has non-ascii.
    masks: Vec<u64>,
}

fn bit(b: u8) -> u64 {
    match b.to_ascii_lowercase() {
        c @ b'a'..=b'z' => 1 << (c - b'a'),
        c @ b'0'..=b'9' => 1 << (26 + (c - b'0')),
        _ => 0,
    }
}

fn mask_of(s: &str) -> u64 {
    s.bytes().fold(0, |m, b| if b < 128 { m | bit(b) } else { m | 1 << 63 })
}

impl Index {
    pub fn push(&mut self, name: &str) {
        let start = self.arena.len() as u32;
        self.arena.push_str(name);
        self.spans.push((start, name.len() as u32));
        self.masks.push(mask_of(name));
    }
    pub fn len(&self) -> usize {
        self.spans.len()
    }
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }
    pub fn name(&self, id: usize) -> &str {
        let (s, l) = self.spans[id];
        &self.arena[s as usize..(s + l) as usize]
    }

    /// All matches as (score, id). `subset` restricts the scan (e.g. previous keystroke's matches,
    /// valid only when the new query extends the old one). `prefilter` toggles the bitmask check.
    pub fn matches(&self, query: &str, subset: Option<&[u32]>, prefilter: bool) -> Vec<(u32, u32)> {
        let pat = Pattern::new(query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
        // Non-ascii query chars: the mask can't judge them, so the filter is skipped.
        let qmask = if prefilter && query.is_ascii() { mask_of(query) } else { 0 };
        let run = |ids: &[u32], m: &mut Matcher, buf: &mut Vec<char>| {
            let mut out = Vec::new();
            for &id in ids {
                // Names with non-ascii chars (bit 63) always pass; the mask only rejects ascii-only names.
                let mk = self.masks[id as usize];
                if mk >> 63 == 0 && mk & qmask != qmask {
                    continue;
                }
                if let Some(s) = pat.score(Utf32Str::new(self.name(id as usize), buf), m) {
                    out.push((s, id));
                }
            }
            out
        };
        let init = || (Matcher::new(Config::DEFAULT), Vec::<char>::new());
        let hits: Vec<(u32, u32)> = match subset {
            Some(ids) => ids
                .par_chunks(16_384)
                .map_init(init, |(m, b), c| run(c, m, b))
                .flatten()
                .collect(),
            None => (0..self.spans.len() as u32)
                .into_par_iter()
                .chunks(16_384)
                .map_init(init, |(m, b), c| run(&c, m, b))
                .flatten()
                .collect(),
        };
        hits
    }

    /// Best `k` matches without materialising every hit: each chunk keeps a bounded top-k.
    pub fn search_topk(&self, query: &str, k: usize) -> Vec<(u32, u32)> {
        let pat = Pattern::new(query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
        let qmask = if query.is_ascii() { mask_of(query) } else { 0 };
        let prune = |v: &mut Vec<(u32, u32)>| {
            if v.len() > k * 4 {
                v.select_nth_unstable_by(k, |a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                v.truncate(k);
            }
        };
        let init = || (Matcher::new(Config::DEFAULT), Vec::<char>::new());
        let mut hits: Vec<(u32, u32)> = (0..self.spans.len() as u32)
            .into_par_iter()
            .chunks(16_384)
            .map_init(init, |(m, buf), ids| {
                let mut out: Vec<(u32, u32)> = Vec::new();
                for id in ids {
                    let mk = self.masks[id as usize];
                    if mk >> 63 == 0 && mk & qmask != qmask {
                        continue;
                    }
                    if let Some(sc) = pat.score(Utf32Str::new(self.name(id as usize), buf), m) {
                        out.push((sc, id));
                        prune(&mut out);
                    }
                }
                Self::top(&mut out, k)
            })
            .flatten()
            .collect();
        Self::top(&mut hits, k)
    }

    pub fn top(hits: &mut Vec<(u32, u32)>, limit: usize) -> Vec<(u32, u32)> {
        hits.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        hits.iter().take(limit).copied().collect()
    }
}
