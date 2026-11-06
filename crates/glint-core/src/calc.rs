//! Offline calculator and unit conversion.

/// Evaluate an arithmetic expression, or `None` if it is not one.
pub fn eval(input: &str) -> Option<f64> {
    let mut p = Parser {
        s: input.as_bytes(),
        i: 0,
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
                v += self.product()?;
            } else if self.eat(b'-') {
                v -= self.product()?;
            } else {
                return Some(v);
            }
        }
    }

    fn product(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        loop {
            if self.eat(b'*') {
                v *= self.unary()?;
            } else if self.eat(b'/') {
                v /= self.unary()?;
            } else if self.eat(b'%') {
                v %= self.unary()?;
            } else {
                return Some(v);
            }
        }
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat(b'-') {
            return Some(-self.unary()?);
        }
        let base = self.atom()?;
        // `^` binds tighter than a leading minus and is right-associative.
        if self.eat(b'^') {
            return Some(base.powf(self.unary()?));
        }
        Some(base)
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
        for s in ["2+2", "10 / 4", "sqrt(9)"] {
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

    #[test]
    fn formats_without_float_noise() {
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(14.0), "14");
        assert_eq!(format_number(2.5), "2.5");
        assert_eq!(format_number(-0.0), "0");
    }
}
