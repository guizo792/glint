//! Parses what the user typed: filters like `ext:pdf` and mode prefixes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Only {
    App,
    File,
    Dir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Search everything.
    All,
    /// `> ` system commands and settings pages.
    Commands,
    /// `? ` web search.
    Web,
    /// `= ` calculator.
    Calc,
    /// `w ` open windows.
    Windows,
    /// `c ` clipboard history.
    Clipboard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub mode: Mode,
    pub text: String,
    pub only: Option<Only>,
    /// Lower-case extension without the dot.
    pub ext: Option<String>,
    /// Lower-case text the path must contain.
    pub within: Option<String>,
}

pub fn parse(input: &str) -> Query {
    let trimmed = input.trim_start();
    let (mode, rest) = [
        ("> ", Mode::Commands),
        ("? ", Mode::Web),
        ("= ", Mode::Calc),
        ("w ", Mode::Windows),
        ("c ", Mode::Clipboard),
    ]
    .iter()
    .find_map(|(p, m)| trimmed.strip_prefix(p).map(|r| (*m, r)))
    .unwrap_or((Mode::All, trimmed));

    let mut q = Query {
        mode,
        text: String::new(),
        only: None,
        ext: None,
        within: None,
    };
    let mut words = Vec::new();
    for word in rest.split_whitespace() {
        let value = |p: &str| word.strip_prefix(p).filter(|v| !v.is_empty());
        if word.eq_ignore_ascii_case("app:") {
            q.only = Some(Only::App);
        } else if word.eq_ignore_ascii_case("file:") {
            q.only = Some(Only::File);
        } else if word.eq_ignore_ascii_case("dir:") {
            q.only = Some(Only::Dir);
        } else if let Some(v) = value("ext:") {
            q.ext = Some(v.trim_start_matches('.').to_lowercase());
        } else if let Some(v) = value("in:") {
            q.within = Some(v.to_lowercase());
        } else {
            words.push(word);
        }
    }
    q.text = words.join(" ");
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_a_normal_search() {
        let q = parse("  annual report ");
        assert_eq!(q.mode, Mode::All);
        assert_eq!(q.text, "annual report");
    }

    #[test]
    fn extracts_filters_anywhere() {
        let q = parse("report ext:.PDF in:Documents file: 2024");
        assert_eq!(q.text, "report 2024");
        assert_eq!(q.ext.as_deref(), Some("pdf"));
        assert_eq!(q.within.as_deref(), Some("documents"));
        assert_eq!(q.only, Some(Only::File));
    }

    #[test]
    fn recognises_mode_prefixes_only_with_a_space() {
        assert_eq!(parse("= 2+2").mode, Mode::Calc);
        assert_eq!(parse("= 2+2").text, "2+2");
        assert_eq!(parse("> lock").mode, Mode::Commands);
        assert_eq!(parse("? rust").mode, Mode::Web);
        assert_eq!(parse("w chrome").mode, Mode::Windows);
        assert_eq!(parse("c token").mode, Mode::Clipboard);
        assert_eq!(parse("word").mode, Mode::All);
        assert_eq!(parse("w").mode, Mode::All);
    }

    #[test]
    fn an_empty_filter_value_stays_as_text() {
        assert_eq!(parse("ext: foo").text, "ext: foo");
    }
}
