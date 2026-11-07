//! User settings, stored as a hand-editable TOML file.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bang {
    pub key: String,
    pub name: String,
    /// `{q}` is replaced by the URL-encoded query.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quicklink {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub hotkey: String,
    pub autostart: bool,
    /// `system`, `light` or `dark`.
    pub theme: String,
    /// `en` or `fr`.
    pub language: String,
    pub max_results: usize,
    /// Keep the typed query if reopened within this many seconds. 0 clears it every time.
    pub remember_query_secs: u64,
    /// Folders to index beyond the fixed drives, such as a removable or network drive.
    pub extra_roots: Vec<PathBuf>,
    /// Folder names to skip, added to the built-in list.
    pub extra_excludes: Vec<String>,
    /// Hours between full rebuilds.
    pub rebuild_hours: u64,
    /// Keep recent copied text in memory. Off unless the user turns it on.
    pub clipboard_history: bool,
    /// Extension folder names the user has allowed to run.
    pub enabled_extensions: Vec<String>,
    pub bangs: Vec<Bang>,
    pub quicklinks: Vec<Quicklink>,
}

impl Default for Config {
    fn default() -> Self {
        let bang = |key: &str, name: &str, url: &str| Bang {
            key: key.to_string(),
            name: name.to_string(),
            url: url.to_string(),
        };
        Config {
            hotkey: "Alt+Space".to_string(),
            autostart: false,
            theme: "system".to_string(),
            language: "en".to_string(),
            max_results: 20,
            remember_query_secs: 3,
            extra_roots: Vec::new(),
            extra_excludes: Vec::new(),
            rebuild_hours: 6,
            clipboard_history: false,
            enabled_extensions: Vec::new(),
            bangs: vec![
                bang("g", "Google", "https://www.google.com/search?q={q}"),
                bang("ddg", "DuckDuckGo", "https://duckduckgo.com/?q={q}"),
                bang(
                    "yt",
                    "YouTube",
                    "https://www.youtube.com/results?search_query={q}",
                ),
                bang("gh", "GitHub", "https://github.com/search?q={q}"),
            ],
            quicklinks: Vec::new(),
        }
    }
}

impl Config {
    /// Read the config. A missing file gives defaults; a broken one is an error saying what
    /// is wrong, so the caller can show it rather than silently resetting the user's settings.
    pub fn load(path: &Path) -> Result<Config, String> {
        match fs::read_to_string(path) {
            Ok(text) => Config::parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            return Err(format!(
                "theme must be system, light or dark, not {}",
                self.theme
            ));
        }
        if self.max_results == 0 || self.max_results > 200 {
            return Err("max_results must be between 1 and 200".to_string());
        }
        for link in self
            .bangs
            .iter()
            .map(|b| &b.url)
            .chain(self.quicklinks.iter().map(|q| &q.url))
        {
            if !link.starts_with("https://") && !link.starts_with("http://") {
                return Err(format!(
                    "links must start with http:// or https://, not {link}"
                ));
            }
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text)?;
        fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults_and_partial_file_keeps_the_rest() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
        let c = Config::parse("hotkey = \"Ctrl+Alt+K\"").unwrap();
        assert_eq!(c.hotkey, "Ctrl+Alt+K");
        assert_eq!(c.max_results, 20);
    }

    #[test]
    fn a_closed_search_is_remembered_for_three_seconds_by_default() {
        assert_eq!(Config::default().remember_query_secs, 3);
        assert_eq!(Config::parse("").unwrap().remember_query_secs, 3);
    }

    #[test]
    fn clipboard_history_is_off_by_default() {
        assert!(!Config::default().clipboard_history);
    }

    #[test]
    fn errors_name_the_problem() {
        assert!(Config::parse("hotkey = 3").unwrap_err().contains("hotkey"));
        assert!(Config::parse("typo_key = 1")
            .unwrap_err()
            .contains("typo_key"));
        assert!(Config::parse("theme = \"pink\"")
            .unwrap_err()
            .contains("theme"));
        let bad = "[[quicklinks]]\nname = \"x\"\nurl = \"file:///c:/evil\"";
        assert!(Config::parse(bad).unwrap_err().contains("http"));
    }

    #[test]
    fn round_trips_through_a_file() {
        let path = std::env::temp_dir().join(format!("glint-cfg-{}.toml", std::process::id()));
        let mut c = Config {
            hotkey: "Ctrl+Shift+Space".to_string(),
            ..Config::default()
        };
        c.quicklinks.push(Quicklink {
            name: "Mail".to_string(),
            url: "https://mail.example.com".to_string(),
        });
        c.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), c);
        fs::remove_file(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), Config::default());
    }
}
