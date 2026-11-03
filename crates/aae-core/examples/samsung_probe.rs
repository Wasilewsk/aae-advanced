//! Manual integration check against the public catalogue, not a CI network dependency.
fn main() -> aae_core::Result<()> {
    let models = aae_core::samsung::models("SM-G781B")?;
    assert!(models.iter().any(|m| m.id == "SM-G781B"));
    for model in &models {
        aae_core::samsung::source_url(&model.url)?;
    }
    let non_uk = aae_core::samsung::models("SM-G781U")?;
    assert!(non_uk.is_empty(), "US-only S20 FE variants must not be shown");
    let countries = aae_core::samsung::countries("SM-G781B")?;
    assert!(countries.iter().any(|c| c.id == "BTU"));
    assert!(
        countries
            .iter()
            .all(|c| c.value.starts_with("United Kingdom"))
    );
    assert!(!countries.iter().any(|c| c.id == "XFA"));
    assert!(aae_core::samsung::firmwares("SM-G781B", "XFA", 1).is_err());
    for country in &countries {
        aae_core::samsung::source_url(&country.url)?;
    }
    let firmware = aae_core::samsung::firmwares("SM-G781B", "BTU", 1)?;
    assert!(!firmware.is_empty());
    for f in &firmware {
        assert_eq!(f.model, "SM-G781B");
        assert_eq!(f.product_code, "BTU");
        aae_core::samsung::source_url(&f.url)?;
    }
    println!(
        "{} UK Samsung model matches, {} UK regions for SM-G781B, {} UK firmware builds",
        models.len(),
        countries.len(),
        firmware.len()
    );
    Ok(())
}
