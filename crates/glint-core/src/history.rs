//! What the user opened: frecency, per-query memory, pins and hidden results.
//! Stored as one local JSON file, replaced atomically on save.
use crate::fold;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::{fs, io};

const HALF_LIFE_DAYS: f64 = 14.0;
const MAX_ITEMS: usize = 5000;
const MAX_PREFIX: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stat {
    pub title: String,
    pub count: u32,
    /// Seconds since the Unix epoch.
    pub last: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct History {
    pub items: HashMap<String, Stat>,
    /// Folded query prefix to the keys chosen for it, with counts.
    pub affinity: HashMap<String, HashMap<String, u32>>,
    pub pins: Vec<String>,
    pub hidden: HashSet<String>,
}

impl History {
    /// Load from `path`. A missing or unreadable file gives an empty history.
    pub fn load(path: &Path) -> History {
        fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_vec(self)?)?;
        fs::rename(tmp, path)
    }

    /// Record that `key` was opened after typing `query`.
    pub fn record(&mut self, query: &str, key: &str, title: &str, now: u64) {
        let stat = self.items.entry(key.to_string()).or_insert(Stat {
            title: title.to_string(),
            count: 0,
            last: now,
        });
        stat.count += 1;
        stat.last = now;
        stat.title = title.to_string();
        let folded = fold(query.trim());
        let ends = folded
            .char_indices()
            .map(|(i, _)| i)
            .skip(1)
            .chain([folded.len()]);
        for end in ends {
            if end == 0 || end > MAX_PREFIX {
                break;
            }
            *self
                .affinity
                .entry(folded[..end].to_string())
                .or_default()
                .entry(key.to_string())
                .or_default() += 1;
        }
        self.trim();
    }

    /// How strongly recent and frequent use favours `key`, decaying with age.
    pub fn frecency(&self, key: &str, now: u64) -> f64 {
        self.items.get(key).map_or(0.0, |s| {
            let days = now.saturating_sub(s.last) as f64 / 86_400.0;
            s.count as f64 * 0.5f64.powf(days / HALF_LIFE_DAYS)
        })
    }

    /// Keys previously chosen for exactly this query, with counts.
    pub fn affine(&self, query: &str) -> Vec<(String, u32)> {
        self.affinity
            .get(&fold(query.trim()))
            .map(|m| m.iter().map(|(k, c)| (k.clone(), *c)).collect())
            .unwrap_or_default()
    }

    /// Most frecent keys, best first, ties broken by key so the order is deterministic.
    pub fn top(&self, n: usize, now: u64) -> Vec<String> {
        let mut keys: Vec<(&String, f64)> = self
            .items
            .keys()
            .map(|k| (k, self.frecency(k, now)))
            .collect();
        keys.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
        keys.into_iter().take(n).map(|(k, _)| k.clone()).collect()
    }

    pub fn toggle_pin(&mut self, key: &str) {
        if let Some(i) = self.pins.iter().position(|p| p == key) {
            self.pins.remove(i);
        } else {
            self.pins.push(key.to_string());
        }
    }

    pub fn hide(&mut self, key: &str) {
        self.hidden.insert(key.to_string());
    }

    /// Forget all use of `key`. Pins and hidden state are kept.
    pub fn forget(&mut self, key: &str) {
        self.items.remove(key);
        for keys in self.affinity.values_mut() {
            keys.remove(key);
        }
        self.affinity.retain(|_, keys| !keys.is_empty());
    }

    fn trim(&mut self) {
        if self.items.len() <= MAX_ITEMS {
            return;
        }
        let mut by_age: Vec<(String, u64)> = self
            .items
            .iter()
            .map(|(k, s)| (k.clone(), s.last))
            .collect();
        by_age.sort_by_key(|e| e.1);
        for (key, _) in by_age.iter().take(self.items.len() - MAX_ITEMS) {
            self.forget(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const DAY: u64 = 86_400;

    #[test]
    fn recent_use_beats_older_use_of_the_same_count() {
        let mut h = History::default();
        h.record("", "old", "Old", 0);
        h.record("", "new", "New", 28 * DAY);
        let now = 28 * DAY;
        assert!(h.frecency("new", now) > h.frecency("old", now));
        assert!((h.frecency("old", now) - 0.25).abs() < 1e-9);
        assert_eq!(h.top(1, now), ["new"]);
    }

    #[test]
    fn remembers_choices_for_every_prefix_ignoring_case_and_accents() {
        let mut h = History::default();
        h.record("Tér", "app:terminal", "Terminal", 1);
        for q in ["t", "te", "ter", "TER", "tér"] {
            assert_eq!(h.affine(q), [("app:terminal".to_string(), 1)], "query {q}");
        }
        assert!(h.affine("term").is_empty());
    }

    #[test]
    fn forget_removes_use_but_keeps_pins() {
        let mut h = History::default();
        h.record("a", "k", "K", 1);
        h.toggle_pin("k");
        h.forget("k");
        assert_eq!(h.frecency("k", 1), 0.0);
        assert!(h.affine("a").is_empty());
        assert_eq!(h.pins, ["k"]);
        h.toggle_pin("k");
        assert!(h.pins.is_empty());
    }

    #[test]
    fn caps_stored_items_dropping_the_oldest() {
        let mut h = History::default();
        for i in 0..MAX_ITEMS as u64 + 3 {
            h.record("", &format!("k{i}"), "t", i);
        }
        assert_eq!(h.items.len(), MAX_ITEMS);
        assert!(!h.items.contains_key("k0"));
    }

    #[test]
    fn round_trips_and_tolerates_a_corrupt_file() {
        let path = std::env::temp_dir().join(format!("glint-h-{}.json", std::process::id()));
        let mut h = History::default();
        h.record("q", "k", "K", 5);
        h.save(&path).unwrap();
        assert_eq!(History::load(&path).items["k"].count, 1);
        fs::write(&path, b"{not json").unwrap();
        assert!(History::load(&path).items.is_empty());
        fs::remove_file(path).unwrap();
    }
}
