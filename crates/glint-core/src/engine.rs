//! Turns typed text into ranked results from every source.
//!
//! Each source is a [`Provider`], the same interface extensions use. The engine merges
//! what they return and applies what it has learned from use.
use crate::history::History;
use crate::live::Live;
use crate::query::{parse, Mode, Only, Query};
use crate::{calc, config, fold, Index, Kind};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

/// One step of a score. Index scores for ordinary matches stay below this.
const STEP: u32 = 1 << 20;
const APP_BONUS: u32 = 3 * STEP;
const COMMAND_BONUS: u32 = 2 * STEP;
const WINDOW_BONUS: u32 = 2 * STEP;
const TOP_SCORE: u32 = 10 * STEP;
const AFFINITY_STEP: u32 = 4 * STEP;
const MAX_AFFINITY_COUNT: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    App,
    File,
    Folder,
    Command,
    Setting,
    Window,
    Clipboard,
    Calc,
    Web,
    Extension,
    Content,
}

/// What happens when a result is opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Target {
    Path(PathBuf),
    App(String),
    Url(String),
    Copy(String),
    System(String),
    Window(isize),
    Extension { extension: String, id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// Stable identity, used by history.
    pub key: String,
    pub title: String,
    pub subtitle: String,
    pub kind: ItemKind,
    pub score: u32,
    #[serde(skip)]
    pub target: Target,
    /// Ask before running.
    pub confirm: bool,
    pub pinned: bool,
    pub actions: Vec<&'static str>,
}

impl Item {
    pub fn new(
        key: String,
        title: String,
        subtitle: String,
        kind: ItemKind,
        target: Target,
    ) -> Item {
        let actions = actions_for(kind);
        Item {
            key,
            title,
            subtitle,
            kind,
            score: 0,
            target,
            confirm: false,
            pinned: false,
            actions,
        }
    }

    pub fn from_path(path: &Path, name: &str, kind: Kind) -> Item {
        let subtitle = path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        Item::new(
            file_key(path),
            name.to_string(),
            subtitle,
            match kind {
                Kind::File => ItemKind::File,
                Kind::Dir => ItemKind::Folder,
            },
            Target::Path(path.to_path_buf()),
        )
    }
}

fn file_key(path: &Path) -> String {
    format!("file:{}", path.display())
}

/// The action menu for a kind of result, in display order.
pub fn actions_for(kind: ItemKind) -> Vec<&'static str> {
    match kind {
        ItemKind::File | ItemKind::Folder => vec![
            "open",
            "admin",
            "reveal",
            "copy_path",
            "copy_name",
            "terminal",
            "properties",
            "pin",
            "hide",
            "forget",
        ],
        ItemKind::App => vec!["open", "admin", "copy_name", "pin", "hide", "forget"],
        ItemKind::Command | ItemKind::Setting => vec!["open", "pin", "hide", "forget"],
        ItemKind::Window
        | ItemKind::Clipboard
        | ItemKind::Calc
        | ItemKind::Web
        | ItemKind::Extension
        | ItemKind::Content => vec!["open"],
    }
}

/// A source of results. Implemented by built-in sources and the extension host alike.
pub trait Provider: Send + Sync {
    /// Results for `q`, at most `limit`. `fuzzy` asks for looser, slower matching; providers
    /// without a slow path can return nothing for it, since their fast results are already in.
    fn query(&self, q: &Query, limit: usize, fuzzy: bool) -> Vec<Item>;

    /// Rebuild a result from its key, for pinned and remembered items.
    fn resolve(&self, _key: &str) -> Option<Item> {
        None
    }
}

/// Files and folders on disk.
pub struct Files(pub Arc<Live>);

impl Provider for Files {
    fn query(&self, q: &Query, limit: usize, fuzzy: bool) -> Vec<Item> {
        if q.mode != Mode::All || q.text.is_empty() || q.only == Some(Only::App) {
            return Vec::new();
        }
        let keep = |name: &str, path: &Path, kind: Kind| {
            let kind_ok = match q.only {
                Some(Only::File) => kind == Kind::File,
                Some(Only::Dir) => kind == Kind::Dir,
                _ => true,
            };
            let ext_ok = q.ext.as_ref().is_none_or(|e| {
                name.rsplit_once('.')
                    .is_some_and(|(_, x)| x.eq_ignore_ascii_case(e))
            });
            let within_ok = q
                .within
                .as_ref()
                .is_none_or(|w| path.to_string_lossy().to_lowercase().contains(w));
            kind_ok && ext_ok && within_ok
        };
        self.0
            .search(&q.text, limit, fuzzy, keep)
            .into_iter()
            .map(|h| {
                let mut item = Item::from_path(&h.path, &h.name, h.kind);
                item.score = h.score;
                item
            })
            .collect()
    }

    fn resolve(&self, key: &str) -> Option<Item> {
        let path = Path::new(key.strip_prefix("file:")?);
        let meta = path.symlink_metadata().ok()?;
        let name = path.file_name()?.to_str()?;
        let kind = if meta.is_dir() { Kind::Dir } else { Kind::File };
        Some(Item::from_path(path, name, kind))
    }
}

/// A fixed list of items searched by title: apps, commands, settings pages.
#[derive(Default)]
pub struct ItemList {
    index: Index,
    items: Vec<Item>,
    by_key: HashMap<String, usize>,
}

impl ItemList {
    pub fn new(items: Vec<Item>) -> ItemList {
        let mut index = Index::default();
        let root = index.add_dir(None, "");
        let mut by_key = HashMap::new();
        for (i, item) in items.iter().enumerate() {
            index.push(&item.title, root, Kind::File);
            by_key.insert(item.key.clone(), i);
        }
        ItemList {
            index,
            items,
            by_key,
        }
    }

    fn search(&self, text: &str, limit: usize, fuzzy: bool, bonus: u32) -> Vec<Item> {
        let hits = if fuzzy {
            self.index.search_fuzzy(text, limit)
        } else {
            self.index.search(text, limit)
        };
        hits.into_iter()
            .map(|h| {
                let mut item = self.items[h.id as usize].clone();
                item.score = h.score + bonus;
                item
            })
            .collect()
    }

    fn get(&self, key: &str) -> Option<Item> {
        self.by_key.get(key).map(|&i| self.items[i].clone())
    }
}

/// Installed apps. The list is replaced when it is refreshed.
#[derive(Default)]
pub struct Apps(pub RwLock<ItemList>);

impl Apps {
    pub fn item(name: &str, target: &str) -> Item {
        Item::new(
            format!("app:{target}"),
            name.to_string(),
            String::new(),
            ItemKind::App,
            Target::App(target.to_string()),
        )
    }
}

impl Provider for Apps {
    fn query(&self, q: &Query, limit: usize, fuzzy: bool) -> Vec<Item> {
        let filtered =
            matches!(q.only, Some(Only::File | Only::Dir)) || q.ext.is_some() || q.within.is_some();
        if q.mode != Mode::All || q.text.is_empty() || filtered {
            return Vec::new();
        }
        self.0
            .read()
            .unwrap()
            .search(&q.text, limit, fuzzy, APP_BONUS)
    }

    fn resolve(&self, key: &str) -> Option<Item> {
        self.0.read().unwrap().get(key)
    }
}

/// System commands and settings pages: listed under `> `, and found by name in normal search.
pub struct Commands(pub ItemList);

impl Provider for Commands {
    fn query(&self, q: &Query, limit: usize, fuzzy: bool) -> Vec<Item> {
        let usable = q.mode == Mode::Commands
            || (q.mode == Mode::All && q.only.is_none() && q.ext.is_none() && q.within.is_none());
        if !usable {
            return Vec::new();
        }
        if q.mode == Mode::Commands && q.text.is_empty() {
            return self.0.items.iter().take(limit).cloned().collect();
        }
        if q.text.is_empty() {
            return Vec::new();
        }
        self.0.search(&q.text, limit, fuzzy, COMMAND_BONUS)
    }

    fn resolve(&self, key: &str) -> Option<Item> {
        self.0.get(key)
    }
}

/// Open windows, from a function supplied by the platform layer.
pub struct Windows(pub Box<dyn Fn() -> Vec<(isize, String)> + Send + Sync>);

impl Provider for Windows {
    fn query(&self, q: &Query, limit: usize, fuzzy: bool) -> Vec<Item> {
        let usable = q.mode == Mode::Windows
            || (q.mode == Mode::All
                && q.only.is_none()
                && q.ext.is_none()
                && q.within.is_none()
                && q.text.chars().count() >= 2);
        if !usable || fuzzy {
            return Vec::new();
        }
        let needle = fold(&q.text);
        (self.0)()
            .into_iter()
            .filter(|(_, title)| needle.is_empty() || fold(title).contains(&needle))
            .take(limit)
            .map(|(handle, title)| {
                let mut item = Item::new(
                    format!("window:{handle}"),
                    title,
                    "Switch to window".to_string(),
                    ItemKind::Window,
                    Target::Window(handle),
                );
                item.score = WINDOW_BONUS;
                item
            })
            .collect()
    }
}

/// Recent copied text, held in memory only.
#[derive(Default)]
pub struct ClipboardHistory(pub Mutex<Vec<String>>);

const MAX_CLIPS: usize = 100;
const MAX_CLIP_BYTES: usize = 64 * 1024;

impl ClipboardHistory {
    pub fn add(&self, text: String) {
        if text.trim().is_empty() || text.len() > MAX_CLIP_BYTES {
            return;
        }
        let mut clips = self.0.lock().unwrap();
        clips.retain(|c| *c != text);
        clips.insert(0, text);
        clips.truncate(MAX_CLIPS);
    }

    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

impl Provider for ClipboardHistory {
    fn query(&self, q: &Query, limit: usize, fuzzy: bool) -> Vec<Item> {
        if q.mode != Mode::Clipboard || fuzzy {
            return Vec::new();
        }
        let needle = fold(&q.text);
        let clips = self.0.lock().unwrap();
        clips
            .iter()
            .filter(|c| needle.is_empty() || fold(c).contains(&needle))
            .take(limit)
            .enumerate()
            .map(|(i, text)| {
                let title: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                let title: String = title.chars().take(100).collect();
                let mut item = Item::new(
                    format!("clip:{i}"),
                    title,
                    "Copy again".to_string(),
                    ItemKind::Clipboard,
                    Target::Copy(text.clone()),
                );
                item.score = TOP_SCORE - i as u32;
                item
            })
            .collect()
    }
}

/// Calculator, unit conversion and, when rates are supplied, currency conversion.
#[derive(Default)]
pub struct Calculator {
    /// Exchange rates, filled in by the app only when the user has turned them on.
    pub rates: Arc<RwLock<Option<calc::Rates>>>,
}

impl Calculator {
    /// `(result, subtitle)` for the text, if it is a calculation or conversion.
    fn answer(&self, text: &str, explicit: bool) -> Option<(String, String)> {
        let copy_hint = |text: &str| format!("{text} (press Enter to copy)");
        if let Some((v, unit)) = calc::convert(text) {
            return Some((
                format!("{} {unit}", calc::format_number(v)),
                copy_hint(text),
            ));
        }
        if let Some(rates) = self.rates.read().unwrap().as_ref() {
            if let Some((v, code)) = calc::convert_currency(text, rates) {
                let age = rates.age_days(calc::today_days()).filter(|d| *d > 5);
                let note = match age {
                    Some(d) => format!("ECB rates from {}, {d} days old", rates.date),
                    None => format!("ECB rates from {}", rates.date),
                };
                return Some((format!("{} {code}", calc::format_money(v)), note));
            }
        }
        (explicit || calc::looks_like_math(text))
            .then(|| calc::eval(text).map(calc::format_number))
            .flatten()
            .map(|result| (result, copy_hint(text)))
    }
}

impl Provider for Calculator {
    fn query(&self, q: &Query, _limit: usize, fuzzy: bool) -> Vec<Item> {
        let explicit = q.mode == Mode::Calc;
        if fuzzy || !(explicit || q.mode == Mode::All) {
            return Vec::new();
        }
        let Some((result, subtitle)) = self.answer(q.text.trim(), explicit) else {
            return Vec::new();
        };
        let mut item = Item::new(
            "calc".to_string(),
            result.clone(),
            subtitle,
            ItemKind::Calc,
            Target::Copy(result),
        );
        item.score = TOP_SCORE;
        vec![item]
    }
}

/// A last row that opens Windows' own content search for the text. Glint indexes names only,
/// so this hands the question to the Windows Search index, which also reads file contents.
pub fn content_search_item(text: &str) -> Item {
    let phrase = if text.contains(char::is_whitespace) {
        format!("content:\"{text}\"")
    } else {
        format!("content:{text}")
    };
    Item::new(
        format!("content:{text}"),
        format!("Search file contents for {text}"),
        "Opens Windows search in its indexed locations".to_string(),
        ItemKind::Content,
        Target::Url(format!("search-ms:query={}", url_encode(&phrase))),
    )
}

/// Encode text for use in a URL.
pub fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Web search shortcuts and quicklinks.
pub struct Web {
    pub bangs: Vec<config::Bang>,
    pub quicklinks: Vec<config::Quicklink>,
}

impl Web {
    fn search_item(&self, bang: &config::Bang, text: &str) -> Item {
        let url = bang.url.replace("{q}", &url_encode(text));
        Item::new(
            format!("web:{}:{text}", bang.key),
            format!("Search {} for {text}", bang.name),
            url.clone(),
            ItemKind::Web,
            Target::Url(url),
        )
    }

    /// The default search, offered when nothing on the PC matches.
    pub fn fallback(&self, text: &str) -> Option<Item> {
        self.bangs.first().map(|b| self.search_item(b, text))
    }
}

impl Provider for Web {
    fn query(&self, q: &Query, _limit: usize, fuzzy: bool) -> Vec<Item> {
        if fuzzy || q.text.is_empty() {
            return Vec::new();
        }
        if q.mode == Mode::Web {
            return self
                .fallback(&q.text)
                .into_iter()
                .map(|mut i| {
                    i.score = TOP_SCORE;
                    i
                })
                .collect();
        }
        if q.mode != Mode::All {
            return Vec::new();
        }
        let mut out = Vec::new();
        if let Some((first, rest)) = q.text.split_once(' ') {
            if let Some(bang) = self
                .bangs
                .iter()
                .find(|b| b.key.eq_ignore_ascii_case(first))
            {
                let mut item = self.search_item(bang, rest.trim());
                item.score = TOP_SCORE;
                out.push(item);
            }
        }
        let needle = fold(&q.text);
        for link in &self.quicklinks {
            if fold(&link.name).contains(&needle) {
                let mut item = Item::new(
                    format!("link:{}", link.url),
                    link.name.clone(),
                    link.url.clone(),
                    ItemKind::Web,
                    Target::Url(link.url.clone()),
                );
                item.score = COMMAND_BONUS;
                out.push(item);
            }
        }
        out
    }
}

pub struct Engine {
    providers: Vec<Arc<dyn Provider>>,
    web: Arc<Web>,
    pub history: Mutex<History>,
    history_path: PathBuf,
    /// The last two result sets, so the UI refers to results by key and cannot name arbitrary
    /// targets. Two, because a slow search finishing late must not invalidate what is on screen.
    shown: Mutex<[HashMap<String, Item>; 2]>,
}

impl Engine {
    pub fn new(providers: Vec<Arc<dyn Provider>>, web: Arc<Web>, history_path: PathBuf) -> Engine {
        let history = History::load(&history_path);
        Engine {
            providers,
            web,
            history: Mutex::new(history),
            history_path,
            shown: Mutex::default(),
        }
    }

    fn resolve(&self, key: &str) -> Option<Item> {
        self.providers.iter().find_map(|p| p.resolve(key))
    }

    /// Ranked results for the typed text. With `fuzzy`, adds slower scattered matches to the
    /// normal results.
    pub fn search(&self, input: &str, limit: usize, fuzzy: bool, now: u64) -> Vec<Item> {
        let mut q = parse(input);
        // `w` and `c` are also ordinary words: if their mode finds nothing, search normally.
        if matches!(q.mode, Mode::Windows | Mode::Clipboard) && !q.text.is_empty() {
            let empty = self
                .providers
                .iter()
                .all(|p| p.query(&q, 1, false).is_empty());
            if empty {
                q = Query {
                    mode: Mode::All,
                    text: input.trim().to_string(),
                    ..q
                };
            }
        }
        let history = self.history.lock().unwrap();
        let results = if input.trim().is_empty() {
            self.home_items(&history, limit, now)
        } else {
            let mut merged: HashMap<String, Item> = HashMap::new();
            let mut add = |item: Item| match merged.get(&item.key) {
                Some(old) if old.score >= item.score => {}
                _ => {
                    merged.insert(item.key.clone(), item);
                }
            };
            for p in &self.providers {
                p.query(&q, limit, false).into_iter().for_each(&mut add);
                // The looser pass adds to the normal results. Providers without a slow path
                // return nothing for it, so replacing would lose their answers.
                if fuzzy {
                    p.query(&q, limit, true).into_iter().for_each(&mut add);
                }
            }
            // Bring back remembered results that fell outside the providers' limits.
            let plain =
                q.mode == Mode::All && q.only.is_none() && q.ext.is_none() && q.within.is_none();
            if plain {
                let needle = fold(&q.text);
                for (key, _) in history.affine(&q.text) {
                    if let Some(item) = self.resolve(&key) {
                        add(item);
                    }
                }
                for (key, stat) in &history.items {
                    if !needle.is_empty() && fold(&stat.title).contains(&needle) {
                        if let Some(item) = self.resolve(key) {
                            add(item);
                        }
                    }
                }
            }
            let affine: HashMap<String, u32> = history.affine(&q.text).into_iter().collect();
            let mut items: Vec<Item> = merged
                .into_values()
                .filter(|i| !history.hidden.contains(&i.key))
                .map(|mut i| {
                    i.pinned = history.pins.contains(&i.key);
                    let frecency = history.frecency(&i.key, now);
                    let learned =
                        (frecency.ln_1p() * 0.5 * STEP as f64).min(2.0 * STEP as f64) as u32;
                    let chosen = affine
                        .get(&i.key)
                        .copied()
                        .unwrap_or(0)
                        .min(MAX_AFFINITY_COUNT);
                    i.score += learned + chosen * AFFINITY_STEP;
                    i
                })
                .collect();
            items.sort_by(|a, b| b.score.cmp(&a.score).then(a.key.cmp(&b.key)));
            items.truncate(limit);
            if items.is_empty() && plain && !q.text.is_empty() {
                items.extend(self.web.fallback(&q.text));
            }
            // Always last, and not for answers such as a calculation.
            let text = q.text.trim();
            if plain && text.chars().count() >= 3 && items.iter().all(|i| i.kind != ItemKind::Calc)
            {
                items.push(content_search_item(text));
            }
            items
        };
        drop(history);
        let mut shown = self.shown.lock().unwrap();
        shown.swap(0, 1);
        shown[0] = results.iter().map(|i| (i.key.clone(), i.clone())).collect();
        results
    }

    /// What the empty search bar shows: pinned, then most used.
    fn home_items(&self, history: &History, limit: usize, now: u64) -> Vec<Item> {
        let mut keys: Vec<String> = history.pins.clone();
        for key in history.top(limit, now) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys.iter()
            .filter(|k| !history.hidden.contains(*k))
            .filter_map(|k| {
                let mut item = self.resolve(k)?;
                item.pinned = history.pins.contains(k);
                Some(item)
            })
            .take(limit)
            .collect()
    }

    /// A result from a recent search.
    pub fn shown(&self, key: &str) -> Option<Item> {
        let shown = self.shown.lock().unwrap();
        shown.iter().find_map(|set| set.get(key).cloned())
    }

    /// Remember that a result was opened.
    pub fn record_open(&self, query: &str, item: &Item, now: u64) {
        // These are transient: the same key would mean something else next time.
        if matches!(
            item.kind,
            ItemKind::Window
                | ItemKind::Clipboard
                | ItemKind::Calc
                | ItemKind::Web
                | ItemKind::Extension
                | ItemKind::Content
        ) {
            return;
        }
        self.update_history(|h| h.record(query, &item.key, &item.title, now));
    }

    /// Change the history and save it.
    pub fn update_history(&self, change: impl FnOnce(&mut History)) {
        let mut history = self.history.lock().unwrap();
        change(&mut history);
        let _ = history.save(&self.history_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::walk::WalkConfig;
    use std::fs;

    struct Fixture {
        engine: Engine,
        tree: PathBuf,
        apps: Arc<Apps>,
        clips: Arc<ClipboardHistory>,
        rates: Arc<RwLock<Option<calc::Rates>>>,
    }

    fn fixture(tag: &str) -> Fixture {
        let base = std::env::temp_dir().join(format!("glint-engine-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let tree = base.join("tree");
        fs::create_dir_all(tree.join("Docs")).unwrap();
        for f in [
            "Docs/terminal-notes.txt",
            "Docs/report.pdf",
            "Docs/report.txt",
            "Docs/résumé.pdf",
        ] {
            fs::write(tree.join(f), b"x").unwrap();
        }
        let store = Arc::new(Store::open(&base.join("data")).unwrap());
        let live = Arc::new(Live::new(store, Vec::new()));
        live.rebuild(&WalkConfig {
            roots: vec![tree.clone()],
            exclude_dirs: Vec::new(),
            threads: 1,
        })
        .unwrap();
        let apps = Arc::new(Apps(RwLock::new(ItemList::new(vec![
            Apps::item("Terminal", "term.app"),
            Apps::item("Notepad", "note.app"),
        ]))));
        let commands = Arc::new(Commands(ItemList::new(vec![Item::new(
            "cmd:lock".to_string(),
            "Lock".to_string(),
            String::new(),
            ItemKind::Command,
            Target::System("lock".to_string()),
        )])));
        let clips = Arc::new(ClipboardHistory::default());
        let web = Arc::new(Web {
            bangs: config::Config::default().bangs,
            quicklinks: vec![config::Quicklink {
                name: "Webmail".to_string(),
                url: "https://mail.example.com".to_string(),
            }],
        });
        let windows = Arc::new(Windows(Box::new(|| vec![(7, "Inbox - Mail".to_string())])));
        let calculator = Arc::new(Calculator::default());
        let rates = calculator.rates.clone();
        let engine = Engine::new(
            vec![
                Arc::new(Files(live)),
                apps.clone(),
                commands,
                windows,
                clips.clone(),
                calculator,
                web.clone(),
            ],
            web,
            base.join("history.json"),
        );
        Fixture {
            engine,
            tree,
            apps,
            clips,
            rates,
        }
    }

    /// Result titles, without the always-present content search row.
    fn titles(items: &[Item]) -> Vec<&str> {
        items
            .iter()
            .filter(|i| i.kind != ItemKind::Content)
            .map(|i| i.title.as_str())
            .collect()
    }

    // A fixed old date, so the staleness note does not depend on today.
    const RATES: &str = r#"{"base":"EUR","date":"2020-01-01","rates":{"USD":1.08,"GBP":0.83}}"#;

    #[test]
    fn currency_conversion_needs_rates_and_says_where_they_come_from() {
        let f = fixture("currency");
        // Off by default: no rates, so it is just a search.
        let items = f.engine.search("100 usd to eur", 10, false, 0);
        assert!(items.iter().all(|i| i.kind != ItemKind::Calc));
        *f.rates.write().unwrap() = Some(calc::Rates::from_json(RATES).unwrap());
        let items = f.engine.search("100 usd to eur", 10, false, 0);
        assert_eq!(items[0].title, "92.59 EUR");
        assert_eq!(items[0].target, Target::Copy("92.59 EUR".to_string()));
        assert!(items[0].subtitle.contains("ECB rates from 2020-01-01"));
        assert!(
            items[0].subtitle.contains("days old"),
            "{}",
            items[0].subtitle
        );
        assert_eq!(
            f.engine.search("$5 to gbp", 10, true, 0)[0].title,
            "3.84 GBP"
        );
        // Units still win for unit questions, and plain searches are unaffected.
        assert_eq!(
            f.engine.search("1 km to m", 10, false, 0)[0].title,
            "1000 m"
        );
        assert!(f
            .engine
            .search("usd", 10, false, 0)
            .iter()
            .all(|i| i.kind != ItemKind::Calc));
    }

    #[test]
    fn fresh_rates_carry_no_age_warning() {
        let f = fixture("fresh");
        let today = calc::today_days();
        let (y, m, d) = {
            // Rebuild a YYYY-MM-DD string for today from the day count.
            let z = today + 719_468;
            let era = z.div_euclid(146_097);
            let doe = z - era * 146_097;
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            (yoe + era * 400 + i64::from(m <= 2), m, d)
        };
        let json =
            format!(r#"{{"base":"EUR","date":"{y:04}-{m:02}-{d:02}","rates":{{"USD":1.08}}}}"#);
        *f.rates.write().unwrap() = Some(calc::Rates::from_json(&json).unwrap());
        let item = f.engine.search("10 eur to usd", 10, false, 0).remove(0);
        assert_eq!(item.title, "10.80 USD");
        assert!(!item.subtitle.contains("days old"), "{}", item.subtitle);
    }

    #[test]
    fn a_last_row_hands_content_search_to_windows() {
        let f = fixture("content");
        let items = f.engine.search("terminal", 10, false, 0);
        let last = items.last().unwrap();
        assert_eq!(last.kind, ItemKind::Content);
        assert_eq!(last.title, "Search file contents for terminal");
        assert_eq!(
            last.target,
            Target::Url("search-ms:query=content%3Aterminal".to_string())
        );
        assert_eq!(
            items.iter().filter(|i| i.kind == ItemKind::Content).count(),
            1
        );
        // More than one word is quoted so it is searched as a phrase.
        let phrase = f
            .engine
            .search("annual report", 10, false, 0)
            .pop()
            .unwrap();
        assert_eq!(
            phrase.target,
            Target::Url("search-ms:query=content%3A%22annual%20report%22".to_string())
        );
    }

    #[test]
    fn the_content_row_stays_out_of_the_way() {
        let f = fixture("content-quiet");
        let has = |q: &str| {
            f.engine
                .search(q, 10, false, 0)
                .iter()
                .any(|i| i.kind == ItemKind::Content)
        };
        assert!(!has("te"), "too short to be worth it");
        assert!(!has(""));
        assert!(!has("2+2*3"), "a calculation is already answered");
        assert!(!has("report ext:pdf"), "filters mean the user wants names");
        assert!(!has("> lock"));
        assert!(has("qqqqzzzz"), "still offered when nothing matches");
        let nothing = f.engine.search("qqqqzzzz", 10, false, 0);
        assert_eq!(nothing[0].kind, ItemKind::Web);
        assert_eq!(nothing[1].kind, ItemKind::Content);
    }

    #[test]
    fn the_content_row_is_not_remembered() {
        let f = fixture("content-history");
        let row = f.engine.search("terminal", 10, false, 0).pop().unwrap();
        f.engine.record_open("terminal", &row, 0);
        assert!(f.engine.history.lock().unwrap().items.is_empty());
    }

    #[test]
    fn apps_outrank_files_of_the_same_quality() {
        let f = fixture("apps");
        let items = f.engine.search("ter", 10, false, 0);
        assert_eq!(items[0].title, "Terminal");
        assert!(titles(&items).contains(&"terminal-notes.txt"));
    }

    #[test]
    fn choosing_a_result_makes_it_first_for_that_query() {
        let f = fixture("learn");
        let note = f
            .engine
            .search("ter", 10, false, 0)
            .into_iter()
            .find(|i| i.title == "terminal-notes.txt")
            .unwrap();
        f.engine.record_open("ter", &note, 0);
        assert_eq!(
            f.engine.search("ter", 10, false, 0)[0].title,
            "terminal-notes.txt"
        );
    }

    #[test]
    fn remembered_results_return_even_when_beyond_the_limit() {
        let f = fixture("limit");
        let note = f
            .engine
            .search("ter", 10, false, 0)
            .into_iter()
            .find(|i| i.title == "terminal-notes.txt")
            .unwrap();
        f.engine.record_open("ter", &note, 0);
        assert_eq!(
            titles(&f.engine.search("ter", 1, false, 0)),
            ["terminal-notes.txt"]
        );
    }

    #[test]
    fn empty_query_shows_pinned_then_frequent_items_only() {
        let f = fixture("home");
        assert!(f.engine.search("", 10, false, 0).is_empty());
        let items = f.engine.search("note", 10, false, 0);
        let app = items.iter().find(|i| i.title == "Notepad").unwrap().clone();
        let file = items
            .iter()
            .find(|i| i.title == "terminal-notes.txt")
            .unwrap()
            .clone();
        f.engine.record_open("note", &app, 0);
        f.engine.update_history(|h| h.toggle_pin(&file.key));
        assert_eq!(
            titles(&f.engine.search("", 10, false, 0)),
            ["terminal-notes.txt", "Notepad"]
        );
    }

    #[test]
    fn hidden_results_disappear_and_deleted_files_are_not_resolved() {
        let f = fixture("hidden");
        let report = f.engine.search("report.txt", 10, false, 0).remove(0);
        f.engine.record_open("report.txt", &report, 0);
        fs::remove_file(f.tree.join("Docs/report.txt")).unwrap();
        assert!(!titles(&f.engine.search("report", 10, false, 0)).contains(&"report.txt"));
        let notepad = f.engine.search("notepad", 10, false, 0).remove(0);
        f.engine.update_history(|h| h.hide(&notepad.key));
        assert!(!titles(&f.engine.search("notepad", 10, false, 0)).contains(&"Notepad"));
    }

    #[test]
    fn filters_narrow_results_and_exclude_apps() {
        let f = fixture("filters");
        assert_eq!(
            titles(&f.engine.search("report ext:pdf", 10, false, 0)),
            ["report.pdf"]
        );
        assert_eq!(
            titles(&f.engine.search("note app:", 10, false, 0)),
            ["Notepad"]
        );
        assert!(!titles(&f.engine.search("note file:", 10, false, 0)).contains(&"Notepad"));
        assert_eq!(titles(&f.engine.search("doc dir:", 10, false, 0)), ["Docs"]);
        assert_eq!(
            titles(&f.engine.search("report in:nowhere", 10, false, 0)).len(),
            0
        );
    }

    #[test]
    fn accents_are_found_either_way() {
        let f = fixture("accents");
        assert_eq!(
            titles(&f.engine.search("resume", 10, false, 0)),
            ["résumé.pdf"]
        );
    }

    #[test]
    fn calculator_answers_math_and_conversions_but_not_plain_numbers() {
        let f = fixture("calc");
        let items = f.engine.search("2+2*3", 10, false, 0);
        assert_eq!(items[0].title, "8");
        assert_eq!(items[0].target, Target::Copy("8".to_string()));
        assert_eq!(f.engine.search("= 2024", 10, false, 0)[0].title, "2024");
        assert_eq!(
            f.engine.search("1 km to m", 10, false, 0)[0].title,
            "1000 m"
        );
        assert_eq!(f.engine.search("3 + 6%", 10, false, 0)[0].title, "3.18");
        assert_eq!(f.engine.search("20% of 50", 10, true, 0)[0].title, "10");
        assert!(!titles(&f.engine.search("2024", 10, false, 0)).contains(&"2024"));
    }

    #[test]
    fn the_loose_pass_keeps_answers_from_sources_without_a_slow_path() {
        let f = fixture("loose");
        assert_eq!(f.engine.search("12*12", 10, true, 0)[0].title, "144");
        assert_eq!(
            titles(&f.engine.search("w mail", 10, true, 0)),
            ["Inbox - Mail"]
        );
        f.clips.add("loose text".to_string());
        assert_eq!(
            titles(&f.engine.search("c loose", 10, true, 0)),
            ["loose text"]
        );
        // And it still finds scattered matches the fast pass skips.
        assert!(!titles(&f.engine.search("trmnlnts", 10, false, 0)).contains(&"terminal-notes.txt"));
        assert!(titles(&f.engine.search("trmnlnts", 10, true, 0)).contains(&"terminal-notes.txt"));
    }

    #[test]
    fn web_bangs_encode_the_query_and_fallback_offers_search_when_nothing_matches() {
        let f = fixture("web");
        let items = f.engine.search("g rust & go", 10, false, 0);
        assert_eq!(
            items[0].target,
            Target::Url("https://www.google.com/search?q=rust%20%26%20go".to_string())
        );
        assert_eq!(
            f.engine.search("qqqqzzzz", 10, false, 0)[0].kind,
            ItemKind::Web
        );
        assert_eq!(
            f.engine.search("webmail", 10, false, 0)[0].kind,
            ItemKind::Web
        );
    }

    #[test]
    fn modes_show_only_their_source() {
        let f = fixture("modes");
        assert_eq!(titles(&f.engine.search("> ", 10, false, 0)), ["Lock"]);
        assert_eq!(titles(&f.engine.search("> lo", 10, false, 0)), ["Lock"]);
        assert_eq!(
            titles(&f.engine.search("w mail", 10, false, 0)),
            ["Inbox - Mail"]
        );
        assert_eq!(
            f.engine.search("w mail", 10, false, 0)[0].target,
            Target::Window(7)
        );
    }

    #[test]
    fn letter_prefixes_fall_back_to_normal_search_when_their_mode_finds_nothing() {
        let f = fixture("prefix");
        *f.apps.0.write().unwrap() = ItemList::new(vec![
            Apps::item("C Drive Tool", "c.app"),
            Apps::item("W Report Viewer", "w.app"),
        ]);
        assert_eq!(
            titles(&f.engine.search("c drive", 10, false, 0)),
            ["C Drive Tool"]
        );
        assert_eq!(
            titles(&f.engine.search("w report", 10, false, 0))[0],
            "W Report Viewer"
        );
        assert_eq!(
            f.engine.search("w qqqqzzzz", 10, false, 0)[0].kind,
            ItemKind::Web
        );
        // The mode still wins when it has something.
        f.clips.add("drive secret".to_string());
        assert_eq!(
            titles(&f.engine.search("c drive", 10, false, 0)),
            ["drive secret"]
        );
    }

    #[test]
    fn clipboard_history_is_searchable_newest_first_and_clearable() {
        let f = fixture("clips");
        f.clips.add("first text".to_string());
        f.clips.add("second text".to_string());
        f.clips.add("first text".to_string());
        f.clips.add("   ".to_string());
        assert_eq!(
            titles(&f.engine.search("c ", 10, false, 0)),
            ["first text", "second text"]
        );
        assert_eq!(
            titles(&f.engine.search("c second", 10, false, 0)),
            ["second text"]
        );
        assert!(f
            .engine
            .search("second", 10, false, 0)
            .iter()
            .all(|i| i.kind != ItemKind::Clipboard));
        f.clips.clear();
        assert!(f.engine.search("c ", 10, false, 0).is_empty());
    }

    #[test]
    fn only_recent_results_can_be_looked_up() {
        let f = fixture("shown");
        let item = f.engine.search("notepad", 10, false, 0).remove(0);
        assert_eq!(f.engine.shown(&item.key), Some(item.clone()));
        // A slow search finishing after a newer one must not invalidate what is on screen.
        f.engine.search("terminal", 10, false, 0);
        assert_eq!(f.engine.shown(&item.key), Some(item.clone()));
        f.engine.search("report", 10, false, 0);
        assert_eq!(f.engine.shown(&item.key), None);
        assert_eq!(f.engine.shown("file:C:\\Windows\\System32\\cmd.exe"), None);
    }

    #[test]
    fn app_list_can_be_replaced_while_running() {
        let f = fixture("refresh");
        *f.apps.0.write().unwrap() = ItemList::new(vec![Apps::item("Zed", "zed.app")]);
        assert_eq!(f.engine.search("zed", 10, false, 0)[0].title, "Zed");
        assert!(!titles(&f.engine.search("notepad", 10, false, 0)).contains(&"Notepad"));
    }

    #[test]
    fn transient_results_are_not_remembered() {
        let f = fixture("transient");
        let calc = f.engine.search("1+1", 10, false, 0).remove(0);
        f.engine.record_open("1+1", &calc, 0);
        assert!(f.engine.history.lock().unwrap().items.is_empty());
    }

    #[test]
    fn results_are_deterministic() {
        let f = fixture("order");
        let a = f.engine.search("report", 10, false, 0);
        assert_eq!(a, f.engine.search("report", 10, false, 0));
    }
}
