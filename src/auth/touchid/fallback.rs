use anyhow::Result;

pub fn is_hardware_available() -> bool {
    false
}

pub fn prompt_biometric(_reason: &str) -> Result<()> {
    Ok(())
}

pub fn delete_biometric_item() -> Result<()> {
    Ok(())
}
