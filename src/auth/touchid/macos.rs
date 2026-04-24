use anyhow::Result;

/// Check if Touch ID hardware is available via LAContext
pub fn is_hardware_available() -> bool {
    // TODO: implement with LAContext in Task 4
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
