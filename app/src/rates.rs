//! Exchange rates. This is the only network use in Glint, and it runs only when the user has
//! turned on "currency_rates" in Settings.
use glint_core::calc::Rates;
use std::path::Path;
use std::time::Duration;

/// European Central Bank reference rates, republished by Frankfurter. No account or key.
const URL: &str = "https://api.frankfurter.dev/v1/latest?base=EUR";
const TIMEOUT: Duration = Duration::from_secs(10);

/// Rates saved by an earlier download, if any.
pub fn load(path: &Path) -> Option<Rates> {
    Rates::from_json(&std::fs::read_to_string(path).ok()?).ok()
}

/// Download the latest rates and save them. The request is a plain GET of the fixed address
/// above: it carries nothing the user typed, and nothing about their files or account.
pub fn fetch(path: &Path) -> Result<Rates, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();
    let mut response = agent
        .get(URL)
        .header("User-Agent", "Glint")
        .call()
        .map_err(|e| e.to_string())?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    // Check it before it replaces anything that works.
    let rates = Rates::from_json(&body)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &body).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, path).map_err(|e| e.to_string())?;
    Ok(rates)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run on purpose with `cargo test -p glint -- --ignored`: it uses the real network.
    #[test]
    #[ignore = "uses the network"]
    fn downloads_and_saves_real_rates() {
        let dir = std::env::temp_dir().join(format!("glint-rates-live-{}", std::process::id()));
        let path = dir.join("rates.json");
        let rates = fetch(&path).unwrap();
        assert_eq!(rates.base, "EUR");
        assert!(rates.rates.len() > 20, "{} currencies", rates.rates.len());
        assert!(rates.rates["USD"] > 0.5 && rates.rates["USD"] < 2.0);
        assert_eq!(load(&path).unwrap(), rates);
        let (value, code) = glint_core::calc::convert_currency("100 usd to eur", &rates).unwrap();
        println!("100 USD = {value:.2} {code} (ECB rates of {})", rates.date);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_or_corrupt_cache_means_no_rates() {
        let dir = std::env::temp_dir().join(format!("glint-rates-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rates.json");
        assert!(load(&path).is_none());
        std::fs::write(&path, "{broken").unwrap();
        assert!(load(&path).is_none());
        std::fs::write(
            &path,
            r#"{"base":"EUR","date":"2026-11-06","rates":{"USD":1.08}}"#,
        )
        .unwrap();
        assert_eq!(load(&path).unwrap().rates["USD"], 1.08);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
