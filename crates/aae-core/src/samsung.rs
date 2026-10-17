//! SamMobile's public firmware catalogue. These packages target physical
//! Samsung hardware; they are never offered as bootable emulator images.
use crate::error::{Error, Result};
use serde::{Deserialize, de::DeserializeOwned};
use std::time::Duration;

const API: &str =
    "https://www.sammobile.com/wp-content/plugins/sam-firmwarepage/includes/SMFirmware/ajax";

#[derive(Debug, Clone, Deserialize)]
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
        .map_err(|e| Error::Message(format!("SamMobile could not be reached: {e}")))?
        .body_mut()
        .with_config()
        .limit(8 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| Error::Message(format!("SamMobile's answer could not be read: {e}")))?;
    serde_json::from_str(&body)
        .map_err(|e| Error::Message(format!("SamMobile's catalogue format could not be read: {e}. Try opening SamMobile in your browser.")))
}

pub fn models() -> Result<Vec<Choice>> {
    let mut models: Vec<Choice> = get("ajax.search.models.php?term=all")?;
    models.sort_by_key(|m| m.value.to_lowercase());
    let mut seen = std::collections::HashSet::new();
    models.retain(|m| seen.insert(m.id.clone()));
    Ok(models)
}

pub fn countries(model: &str) -> Result<Vec<Choice>> {
    get(&format!(
        "ajax.countriesbymodel.php?model={}",
        identifier(model)?
    ))
}

/// One page, exactly as the source lists it; callers may request more until
/// an empty page. Android versions and build codes are not One UI versions.
pub fn firmwares(model: &str, country: &str, page: u32) -> Result<Vec<Firmware>> {
    if page == 0 {
        return Err(Error::Message("Firmware pages start at 1.".into()));
    }
    get(&format!(
        "ajax.firmwaresbycountry.php?model={}&country={}&page={page}",
        identifier(model)?,
        identifier(country)?
    ))
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
    fn queries_reject_injection() {
        for bad in ["", "../", "SM-G781B&country=XXX", "BTU#", "a b"] {
            assert!(identifier(bad).is_err());
        }
        assert!(identifier("SM-G781B").is_ok());
        assert!(identifier("BTU").is_ok());
    }
    #[test]
    fn source_links_stay_on_sammobile() {
        assert_eq!(source_url("/samsung/galaxy-tab s/firmware/#SM-T705W").unwrap(), "https://www.sammobile.com/samsung/galaxy-tab%20s/firmware/#SM-T705W");
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
