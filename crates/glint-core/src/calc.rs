//! Offline calculator, unit conversion, and currency conversion from a supplied rate table.

/// Evaluate an arithmetic expression, or `None` if it is not one.
pub fn eval(input: &str) -> Option<f64> {
    let mut p = Parser {
        s: input.as_bytes(),
        i: 0,
        pct: false,
    };
    let value = p.sum()?;
    p.skip();
    (p.i == p.s.len() && value.is_finite()).then_some(value)
}

/// Whether text looks like a calculation rather than a search, such as a bare year or `a-b`.
pub fn looks_like_math(input: &str) -> bool {
    let t = input.trim();
    // A date or part number such as 2024-11-06, not a subtraction.
    let dashed = t.bytes().all(|b| b.is_ascii_digit() || b == b'-');
    if dashed && t.matches('-').count() >= 2 {
        return false;
    }
    let has_digit = t.bytes().any(|b| b.is_ascii_digit());
    let has_op = t
        .bytes()
        .enumerate()
        .any(|(i, b)| b"+*/^%".contains(&b) || (b == b'-' && i > 0));
    let has_func = FUNCTIONS.iter().any(|f| t.starts_with(&format!("{f}(")));
    (has_digit && has_op) || has_func
}

const FUNCTIONS: &[&str] = &[
    "sqrt", "sin", "cos", "tan", "ln", "log", "abs", "round", "floor", "ceil",
];

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    /// Whether the term just parsed was a lone percentage such as `6%`, which `+` and `-`
    /// take as a share of the left side: `3 + 6%` is 3.18.
    pct: bool,
}

impl Parser<'_> {
    fn skip(&mut self) {
        while self.s.get(self.i).is_some_and(|b| b.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        self.skip();
        if self.s.get(self.i) == Some(&b) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        loop {
            if self.eat(b'+') {
                let term = self.product()?;
                v += if self.pct { v * term } else { term };
            } else if self.eat(b'-') {
                let term = self.product()?;
                v -= if self.pct { v * term } else { term };
            } else {
                self.pct = false;
                return Some(v);
            }
        }
    }

    fn product(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        let mut lone_percent = self.pct;
        loop {
            if self.eat(b'*') {
                v *= self.unary()?;
            } else if self.eat(b'/') {
                v /= self.unary()?;
            } else if self.eat(b'%') {
                // Reached only when an operand follows, so this is modulo.
                v %= self.unary()?;
            } else {
                self.pct = lone_percent;
                return Some(v);
            }
            lone_percent = false;
        }
    }

    fn unary(&mut self) -> Option<f64> {
        self.pct = false;
        if self.eat(b'-') {
            let v = -self.unary()?;
            self.pct = false;
            return Some(v);
        }
        let base = self.atom()?;
        // `^` binds tighter than a leading minus and is right-associative.
        if self.eat(b'^') {
            let v = base.powf(self.unary()?);
            self.pct = false;
            return Some(v);
        }
        if self.percent_follows() {
            self.eat(b'%');
            let mut v = base / 100.0;
            self.pct = true;
            // `20% of 50` is 10.
            if self.eat_word("of") {
                v *= self.unary()?;
                self.pct = false;
            }
            return Some(v);
        }
        Some(base)
    }

    /// Whether the next `%` is a percent sign, not modulo: nothing that starts an operand
    /// follows it, except the word `of`.
    fn percent_follows(&mut self) -> bool {
        self.skip();
        if self.s.get(self.i) != Some(&b'%') {
            return false;
        }
        let mut j = self.i + 1;
        while self.s.get(j).is_some_and(|b| b.is_ascii_whitespace()) {
            j += 1;
        }
        let word_of = self.s[j..].starts_with(b"of")
            && !self.s.get(j + 2).is_some_and(|b| b.is_ascii_alphanumeric());
        word_of
            || !self
                .s
                .get(j)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'(' || *b == b'.')
    }

    fn eat_word(&mut self, word: &str) -> bool {
        self.skip();
        let end = self.i + word.len();
        let matches = self.s[self.i..].starts_with(word.as_bytes())
            && !self.s.get(end).is_some_and(|b| b.is_ascii_alphanumeric());
        if matches {
            self.i = end;
        }
        matches
    }

    fn atom(&mut self) -> Option<f64> {
        self.skip();
        if self.eat(b'(') {
            let v = self.sum()?;
            return self.eat(b')').then_some(v);
        }
        let start = self.i;
        while self
            .s
            .get(self.i)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'.')
        {
            self.i += 1;
        }
        let word = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        if word.is_empty() {
            return None;
        }
        if word.as_bytes()[0].is_ascii_digit() || word.starts_with('.') {
            return word.parse().ok();
        }
        match word {
            "pi" => return Some(std::f64::consts::PI),
            "e" => return Some(std::f64::consts::E),
            _ => {}
        }
        if !self.eat(b'(') {
            return None;
        }
        let x = self.sum()?;
        if !self.eat(b')') {
            return None;
        }
        Some(match word {
            "sqrt" => x.sqrt(),
            "sin" => x.sin(),
            "cos" => x.cos(),
            "tan" => x.tan(),
            "ln" => x.ln(),
            "log" => x.log10(),
            "abs" => x.abs(),
            "round" => x.round(),
            "floor" => x.floor(),
            "ceil" => x.ceil(),
            _ => return None,
        })
    }
}

/// Exchange rates: how many units of each currency equal one unit of `base`.
#[derive(Debug, Clone, PartialEq)]
pub struct Rates {
    pub base: String,
    /// Publication date, `YYYY-MM-DD`.
    pub date: String,
    pub rates: std::collections::HashMap<String, f64>,
}

impl Rates {
    /// Parse the JSON published by the rates service, rejecting anything implausible.
    pub fn from_json(text: &str) -> Result<Rates, String> {
        #[derive(serde::Deserialize)]
        struct Wire {
            base: String,
            date: String,
            rates: std::collections::HashMap<String, f64>,
        }
        let wire: Wire = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let code_ok = |c: &str| c.len() == 3 && c.bytes().all(|b| b.is_ascii_alphabetic());
        if !code_ok(&wire.base) {
            return Err("the base currency is not a 3-letter code".to_string());
        }
        if civil_days(&wire.date).is_none() {
            return Err("the rates have no valid date".to_string());
        }
        if wire.rates.is_empty() {
            return Err("the rate table is empty".to_string());
        }
        let mut rates = std::collections::HashMap::new();
        for (code, rate) in wire.rates {
            if !code_ok(&code) || !rate.is_finite() || rate <= 0.0 {
                return Err(format!("the rate for {code} is not usable"));
            }
            rates.insert(code.to_uppercase(), rate);
        }
        let base = wire.base.to_uppercase();
        rates.insert(base.clone(), 1.0);
        Ok(Rates {
            base,
            date: wire.date,
            rates,
        })
    }

    /// Whole days from the rates' date to `today` (days since 1970-01-01).
    pub fn age_days(&self, today: i64) -> Option<i64> {
        civil_days(&self.date).map(|d| today - d)
    }

    pub fn convert(&self, amount: f64, from: &str, to: &str) -> Option<f64> {
        let (from, to) = (self.rates.get(from)?, self.rates.get(to)?);
        Some(amount / from * to)
    }
}

/// Days since 1970-01-01 for a `YYYY-MM-DD` date.
fn civil_days(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let (y, m, d): (i64, i64, i64) = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Days since 1970-01-01 right now, in UTC.
pub fn today_days() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as i64)
}

/// Convert text like `100 usd to eur`, `100usd in eur` or `$50 to eur`. Returns the amount and
/// the target currency code.
pub fn convert_currency(input: &str, rates: &Rates) -> Option<(f64, String)> {
    let lower = input.trim().to_lowercase();
    let (symbol, rest) = match lower.chars().next() {
        Some('$') => (Some("USD"), &lower[1..]),
        Some('€') => (Some("EUR"), &lower['€'.len_utf8()..]),
        Some('£') => (Some("GBP"), &lower['£'.len_utf8()..]),
        _ => (None, &lower[..]),
    };
    let mut words: Vec<&str> = rest.split_whitespace().collect();
    // Allow `100usd to eur` as well as `100 usd to eur`.
    let glued;
    if symbol.is_none() && words.len() == 3 {
        let at = words[0]
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != ',')
            .filter(|&i| i > 0)?;
        glued = words[0].split_at(at);
        words = vec![glued.0, glued.1, words[1], words[2]];
    }
    let (amount, from, joiner, to) = match (symbol, &words[..]) {
        (Some(code), [a, j, t]) => (*a, code.to_string(), *j, *t),
        (None, [a, f, j, t]) => (*a, f.to_uppercase(), *j, *t),
        _ => return None,
    };
    if joiner != "to" && joiner != "in" {
        return None;
    }
    let amount: f64 = amount.replace(',', "").parse().ok()?;
    let to = to.to_uppercase();
    let value = rates.convert(amount, &from, &to)?;
    Some((value, to))
}

/// A money amount to two decimals.
pub fn format_money(v: f64) -> String {
    format!("{v:.2}")
}

/// Units as (name, group, factor to the group's base unit).
const UNITS: &[(&str, &str, f64)] = &[
    ("mm", "length", 0.001),
    ("cm", "length", 0.01),
    ("m", "length", 1.0),
    ("km", "length", 1000.0),
    ("in", "length", 0.0254),
    ("ft", "length", 0.3048),
    ("yd", "length", 0.9144),
    ("mi", "length", 1609.344),
    ("mg", "mass", 0.001),
    ("g", "mass", 1.0),
    ("kg", "mass", 1000.0),
    ("oz", "mass", 28.349523125),
    ("lb", "mass", 453.59237),
    ("ms", "time", 0.001),
    ("s", "time", 1.0),
    ("min", "time", 60.0),
    ("h", "time", 3600.0),
    ("day", "time", 86400.0),
    ("week", "time", 604800.0),
    ("ml", "volume", 0.001),
    ("l", "volume", 1.0),
    ("gal", "volume", 3.785411784),
    ("b", "data", 1.0),
    ("kb", "data", 1e3),
    ("mb", "data", 1e6),
    ("gb", "data", 1e9),
    ("tb", "data", 1e12),
    ("kib", "data", 1024.0),
    ("mib", "data", 1048576.0),
    ("gib", "data", 1073741824.0),
];

/// Convert text like `10 km to mi` or `100 f in c`. Returns the value and target unit.
pub fn convert(input: &str) -> Option<(f64, String)> {
    let lower = input.trim().to_lowercase();
    let mut words: Vec<&str> = lower.split_whitespace().collect();
    // Allow `10km to mi` as well as `10 km to mi`.
    let split;
    if words.len() == 3 {
        let digits = words[0]
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
            .filter(|&i| i > 0)?;
        split = words[0].split_at(digits);
        words = vec![split.0, split.1, words[1], words[2]];
    }
    let [number, from, joiner, to] = words[..] else {
        return None;
    };
    if joiner != "to" && joiner != "in" {
        return None;
    }
    let value: f64 = number.parse().ok()?;
    if let Some(result) = convert_temperature(value, from, to) {
        return Some((result, to.to_uppercase()));
    }
    let find = |name: &str| UNITS.iter().find(|u| u.0 == name);
    let (a, b) = (find(from)?, find(to)?);
    (a.1 == b.1).then(|| (value * a.2 / b.2, to.to_string()))
}

fn convert_temperature(value: f64, from: &str, to: &str) -> Option<f64> {
    let celsius = match from {
        "c" => value,
        "f" => (value - 32.0) * 5.0 / 9.0,
        "k" => value - 273.15,
        _ => return None,
    };
    match to {
        "c" => Some(celsius),
        "f" => Some(celsius * 9.0 / 5.0 + 32.0),
        "k" => Some(celsius + 273.15),
        _ => None,
    }
}

/// Show a result without floating-point noise: `0.1 + 0.2` reads `0.3`.
pub fn format_number(v: f64) -> String {
    if v != 0.0 && (v.abs() >= 1e15 || v.abs() < 1e-6) {
        return format!("{v:e}");
    }
    let text = format!("{v:.10}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_with_correct_precedence() {
        assert_eq!(eval("2 + 3 * 4"), Some(14.0));
        assert_eq!(eval("(2 + 3) * 4"), Some(20.0));
        assert_eq!(eval("2 ^ 3 ^ 2"), Some(512.0));
        assert_eq!(eval("-2 ^ 2"), Some(-4.0));
        assert_eq!(eval("7 % 3"), Some(1.0));
        assert_eq!(eval("sqrt(16) + 1"), Some(5.0));
    }

    #[test]
    fn percentages_work_like_a_phone_calculator() {
        let close = |input: &str, want: f64| {
            let got = eval(input).unwrap_or_else(|| panic!("no result for {input}"));
            assert!((got - want).abs() < 1e-9, "{input} = {got}, wanted {want}");
        };
        close("3 + 6%", 3.18);
        close("100 - 10%", 90.0);
        close("200 * 15%", 30.0);
        close("50 / 50%", 100.0);
        close("6%", 0.06);
        close("(3 + 3) + 50%", 9.0);
        close("20% of 50", 10.0);
        close("20 % of 50", 10.0);
        close("3 + 6% * 2", 3.12);
        close("1000 + 10% + 10%", 1210.0);
        close("1+2+3%", 3.09);
    }

    #[test]
    fn modulo_still_works_when_an_operand_follows() {
        assert_eq!(eval("7 % 3"), Some(1.0));
        assert_eq!(eval("7 %3"), Some(1.0));
        assert_eq!(eval("10 % (2 + 1)"), Some(1.0));
        assert_eq!(eval("10 % sqrt(4)"), Some(0.0));
    }

    #[test]
    fn rejects_non_math_and_non_finite_results() {
        for bad in ["hello", "2 +", "1/0", "(1", "foo(2)", "2 2", ""] {
            assert_eq!(eval(bad), None, "{bad}");
        }
    }

    #[test]
    fn does_not_mistake_searches_for_math() {
        for s in ["2024", "report", "report-2024", "a-b", "2024-11-06"] {
            assert!(!looks_like_math(s) || eval(s).is_none(), "{s}");
        }
        for s in ["2+2", "10 / 4", "sqrt(9)", "3 + 6%", "20% of 50"] {
            assert!(looks_like_math(s), "{s}");
        }
    }

    #[test]
    fn converts_units_and_temperature() {
        let (v, u) = convert("10 km to mi").unwrap();
        assert!((v - 6.21371).abs() < 1e-4);
        assert_eq!(u, "mi");
        assert!((convert("100 F in C").unwrap().0 - 37.7777).abs() < 1e-3);
        assert!((convert("1gib to mb").unwrap().0 - 1073.741824).abs() < 1e-6);
    }

    #[test]
    fn refuses_mismatched_or_unknown_units() {
        assert_eq!(convert("10 km to kg"), None);
        assert_eq!(convert("10 parsec to m"), None);
        assert_eq!(convert("report to boss"), None);
    }

    const RATES: &str = r#"{"amount":1.0,"base":"EUR","date":"2026-11-06",
        "rates":{"USD":1.08,"GBP":0.83,"JPY":176.0}}"#;

    #[test]
    fn parses_the_rate_service_response_and_adds_the_base_currency() {
        let rates = Rates::from_json(RATES).unwrap();
        assert_eq!(rates.base, "EUR");
        assert_eq!(rates.date, "2026-11-06");
        assert_eq!(rates.rates["EUR"], 1.0);
        assert_eq!(rates.rates["USD"], 1.08);
    }

    #[test]
    fn rejects_implausible_rate_data() {
        for bad in [
            "{not json",
            r#"{"base":"EUR","date":"2026-11-06","rates":{}}"#,
            r#"{"base":"EUR","date":"soon","rates":{"USD":1.0}}"#,
            r#"{"base":"EUR","date":"2026-13-40","rates":{"USD":1.0}}"#,
            r#"{"base":"EURO","date":"2026-11-06","rates":{"USD":1.0}}"#,
            r#"{"base":"EUR","date":"2026-11-06","rates":{"USD":0}}"#,
            r#"{"base":"EUR","date":"2026-11-06","rates":{"USD":-2.5}}"#,
            r#"{"base":"EUR","date":"2026-11-06","rates":{"US":1.2}}"#,
        ] {
            assert!(Rates::from_json(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn dates_count_days_correctly() {
        assert_eq!(civil_days("1970-01-01"), Some(0));
        assert_eq!(civil_days("2000-03-01"), Some(11_017));
        assert_eq!(civil_days("2024-02-29"), Some(19_782));
        let rates = Rates::from_json(RATES).unwrap();
        assert_eq!(rates.age_days(civil_days("2026-11-10").unwrap()), Some(4));
        assert!(today_days() > civil_days("2026-01-01").unwrap());
    }

    #[test]
    fn converts_through_the_base_currency() {
        let rates = Rates::from_json(RATES).unwrap();
        let close = |got: Option<f64>, want: f64| assert!((got.unwrap() - want).abs() < 1e-9);
        close(rates.convert(100.0, "EUR", "USD"), 108.0);
        close(rates.convert(108.0, "USD", "EUR"), 100.0);
        close(rates.convert(1.08, "USD", "GBP"), 0.83);
        close(rates.convert(5.0, "JPY", "JPY"), 5.0);
        assert_eq!(rates.convert(1.0, "USD", "XXX"), None);
    }

    #[test]
    fn understands_the_ways_people_write_currency_questions() {
        let rates = Rates::from_json(RATES).unwrap();
        for input in [
            "100 usd to eur",
            "100 USD in EUR",
            "100usd to eur",
            "$100 to eur",
            "  100 Usd To Eur ",
        ] {
            let (v, to) = convert_currency(input, &rates).unwrap_or_else(|| panic!("{input}"));
            assert_eq!(to, "EUR", "{input}");
            assert_eq!(format_money(v), "92.59", "{input}");
        }
        assert_eq!(
            format_money(convert_currency("€10 to usd", &rates).unwrap().0),
            "10.80"
        );
        assert_eq!(
            format_money(convert_currency("£1 to eur", &rates).unwrap().0),
            "1.20"
        );
        assert_eq!(
            format_money(convert_currency("1,000 usd to eur", &rates).unwrap().0),
            "925.93"
        );
    }

    #[test]
    fn leaves_other_text_alone() {
        let rates = Rates::from_json(RATES).unwrap();
        for input in [
            "100 usd",
            "100 usd to",
            "usd to eur",
            "100 usd to xyz",
            "100 abc to eur",
            "report to boss",
            "10 km to mi",
            "to eur",
            "",
        ] {
            assert_eq!(convert_currency(input, &rates), None, "{input}");
        }
    }

    #[test]
    fn formats_without_float_noise() {
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(14.0), "14");
        assert_eq!(format_number(2.5), "2.5");
        assert_eq!(format_number(-0.0), "0");
    }
}
