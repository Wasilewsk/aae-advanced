//! Manual integration check against the public catalogue, not a CI network dependency.
fn main() -> aae_core::Result<()> {
    let models = aae_core::samsung::models()?;
    assert!(models.iter().any(|m| m.id == "SM-G781B"));
    for model in &models { aae_core::samsung::source_url(&model.url)?; }
    let countries = aae_core::samsung::countries("SM-G781B")?;
    assert!(countries.iter().any(|c| c.id == "BTU"));
    for country in &countries { aae_core::samsung::source_url(&country.url)?; }
    let firmware = aae_core::samsung::firmwares("SM-G781B", "BTU", 1)?;
    assert!(!firmware.is_empty());
    for f in &firmware {
        assert_eq!(f.model, "SM-G781B");
        assert_eq!(f.product_code, "BTU");
        aae_core::samsung::source_url(&f.url)?;
    }
    println!(
        "{} Samsung models, {} regions for SM-G781B, {} UK firmware builds",
        models.len(),
        countries.len(),
        firmware.len()
    );
    Ok(())
}
