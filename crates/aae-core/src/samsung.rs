//! SamMobile's public firmware catalogue. These packages target physical
//! Samsung hardware; they are never offered as bootable emulator images.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Duration;

const API: &str =
    "https://www.sammobile.com/wp-content/plugins/sam-firmwarepage/includes/SMFirmware/ajax";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub id: String,
    pub value: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Firmware {
    pub model: String,
    pub country: String,
    pub product_code: String,
    pub pda: String,
    // SamMobile uses both numbers and strings across old and new releases.
    pub version: serde_json::Value,
    #[serde(default)]
    pub os: Option<String>,
    pub url: String,
}

/// Only identifiers go into endpoint queries, never arbitrary URLs or shell text.
fn identifier(value: &str) -> Result<&str> {
    if value.is_empty()
        || value.len() > 32
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::Message(
            "Choose a valid Samsung model or country code.".into(),
        ));
    }
    Ok(value)
}

/// Download a bounded response with a timeout. A blocked or changed endpoint
/// produces an ordinary error that the host app can display and retry.
fn get<T: DeserializeOwned>(endpoint: &str) -> Result<Vec<T>> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .new_agent();
    let body = agent
        .get(format!("{API}/{endpoint}"))
        .header("User-Agent", "AAE-Advanced/0.7 (firmware catalogue)")
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(429) => Error::Message("SamMobile is limiting requests. Wait a few minutes and search again.".into()),
            other => Error::Message(format!("SamMobile could not be reached: {other}")),
        })?
        .body_mut()
        .with_config()
        .limit(8 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| Error::Message(format!("SamMobile's answer could not be read: {e}")))?;
    serde_json::from_str(&body)
        .map_err(|e| Error::Message(format!("SamMobile's catalogue format could not be read: {e}. Try opening SamMobile in your browser.")))
}

/// Match UK regions, including carrier-specific firmware.
fn is_uk(label: &str) -> bool {
    let label = label.trim();
    label
        .get(..14)
        .is_some_and(|s| s.eq_ignore_ascii_case("United Kingdom"))
        && label
            .get(14..)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(" ("))
}

fn search_term(query: &str) -> Result<String> {
    let query = query.trim();
    if query.len() < 3
        || query.len() > 80
        || query.eq_ignore_ascii_case("all")
        || !query
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b' ')
    {
        return Err(Error::Message("Enter a model number or phone name with at least three letters or numbers, such as SM-G781B or Galaxy S20 FE.".into()));
    }
    Ok(query.replace(' ', "%20"))
}

/// Search first, then verify UK availability for each matching model. This
/// avoids scanning thousands of international models on every refresh.
pub fn models(query: &str) -> Result<Vec<Choice>> {
    let term = search_term(query)?;
    let mut models: Vec<Choice> = get(&format!("ajax.search.models.php?term={term}"))?;
    if models.len() > 30 {
        return Err(Error::Message("That search matches too many Samsung models. Enter a more specific name or the exact model number to check UK availability.".into()));
    }
    let mut uk_models = Vec::new();
    models.sort_by_key(|m| m.value.to_lowercase());
    let mut seen = std::collections::HashSet::new();
    for model in models {
        if seen.insert(model.id.clone()) && !countries(&model.id)?.is_empty() {
            uk_models.push(model);
        }
    }
    Ok(uk_models)
}

/// Only verified UK country/carrier choices leave the shared API. Country
/// records are cached for one day so repeated searches don't hammer the site.
pub fn countries(model: &str) -> Result<Vec<Choice>> {
    identifier(model)?;
    let cache = crate::paths::data_dir()
        .join("cache/samsung-regions")
        .join(format!("{model}.json"));
    let fresh = std::fs::metadata(&cache)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < Duration::from_secs(24 * 60 * 60));
    if fresh {
        if let Ok(text) = std::fs::read_to_string(&cache) {
            if let Ok(countries) = serde_json::from_str::<Vec<Choice>>(&text) {
                return Ok(countries.into_iter().filter(|c| is_uk(&c.value)).collect());
            }
        }
    }
    let countries: Vec<Choice> = get(&format!("ajax.countriesbymodel.php?model={model}"))?;
    if let Some(dir) = cache.parent() {
        if std::fs::create_dir_all(dir).is_ok() {
            if let Ok(text) = serde_json::to_vec(&countries) {
                let _ = std::fs::write(&cache, text);
            }
        }
    }
    Ok(countries.into_iter().filter(|c| is_uk(&c.value)).collect())
}

/// One page, exactly as the source lists it; callers may request more until
/// an empty page. Android versions and build codes are not One UI versions.
pub fn firmwares(model: &str, country: &str, page: u32) -> Result<Vec<Firmware>> {
    if page == 0 {
        return Err(Error::Message("Firmware pages start at 1.".into()));
    }
    identifier(model)?;
    identifier(country)?;
    if !countries(model)?.iter().any(|c| c.id == country) {
        return Err(Error::Message(
            "Only United Kingdom firmware and UK carrier codes are available.".into(),
        ));
    }
    let firmware: Vec<Firmware> = get(&format!(
        "ajax.firmwaresbycountry.php?model={model}&country={country}&page={page}"
    ))?;
    // Defend against ignored query parameters or a changed upstream endpoint.
    Ok(firmware
        .into_iter()
        .filter(|f| f.model == model && f.product_code == country && is_uk(&f.country))
        .collect())
}

/// Validate links from the third party before offering them to a host browser.
pub fn source_url(url: &str) -> Result<String> {
    // Two legacy model slugs in the source contain literal spaces. Encode
    // them rather than rejecting the entire otherwise valid model list.
    let url = url.replace(' ', "%20");
    let url = if url.starts_with("/samsung/") {
        format!("https://www.sammobile.com{url}")
    } else {
        url
    };
    if !url.starts_with("https://www.sammobile.com/samsung/")
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(Error::Message(
            "SamMobile returned an unexpected firmware link.".into(),
        ));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_uk_and_uk_carrier_labels_are_accepted() {
        for uk in [
            "United Kingdom",
            "United Kingdom (EE)",
            "United Kingdom (BTU)",
        ] {
            assert!(is_uk(uk));
        }
        for other in [
            "South Africa",
            "United States",
            "Not United Kingdom",
            "United Kingdomish",
        ] {
            assert!(!is_uk(other));
        }
    }

    #[test]
    fn searches_are_bounded_and_query_safe() {
        assert_eq!(search_term("Galaxy S20 FE").unwrap(), "Galaxy%20S20%20FE");
        assert_eq!(search_term("SM-G781B").unwrap(), "SM-G781B");
        for query in ["", "S2", "all", "SM-G781B&country=XFA", "../", "%"] {
            assert!(search_term(query).is_err());
        }
    }

    #[test]
    fn queries_reject_injection() {
        for bad in ["", "../", "SM-G781B&country=XXX", "BTU#", "a b"] {
            assert!(identifier(bad).is_err());
        }
        assert!(identifier("SM-G781B").is_ok());
        assert!(identifier("BTU").is_ok());
    }
    #[test]
    fn source_links_stay_on_sammobile() {
        assert_eq!(
            source_url("/samsung/galaxy-tab s/firmware/#SM-T705W").unwrap(),
            "https://www.sammobile.com/samsung/galaxy-tab%20s/firmware/#SM-T705W"
        );
        assert_eq!(
            source_url("/samsung/galaxy-s20-fe/firmware/#SM-G781B").unwrap(),
            "https://www.sammobile.com/samsung/galaxy-s20-fe/firmware/#SM-G781B"
        );
        for bad in [
            "https://evil.test/samsung/",
            "//evil.test",
            "https://www.sammobile.com.evil.test/samsung/",
            "javascript:alert(1)",
        ] {
            assert!(source_url(bad).is_err());
        }
    }
    #[test]
    fn versions_keep_the_sources_values() {
        for version in ["13", "\"4.4.2\""] {
            let json = format!(
                r#"{{"model":"SM-G781B","country":"United Kingdom","product_code":"BTU","pda":"G781BXXSIHYJ1","version":{version},"os":"Android","url":"https://www.sammobile.com/samsung/galaxy-s20-fe/firmware/"}}"#
            );
            let f: Firmware = serde_json::from_str(&json).unwrap();
            assert_eq!(f.product_code, "BTU");
            assert!(f.version.is_number() || f.version.is_string());
        }
    }
}
