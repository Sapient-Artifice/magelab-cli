use anyhow::Result;

pub fn is_hardware_available() -> bool {
    false
}

pub fn prompt_biometric(_reason: &str) -> Result<()> {
    Ok(())
}

pub fn store_biometric_item(_token: &str) -> Result<()> {
    Ok(())
}

pub fn load_biometric_item() -> Result<Option<String>> {
    Ok(None)
}

pub fn delete_biometric_item() -> Result<()> {
    Ok(())
}
