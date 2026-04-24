use magelab_cli::auth::touchid::{self, Tier};

#[test]
fn is_available_returns_false_when_disabled() {
    touchid::set_disabled(true);
    assert!(!touchid::is_available());
    // Reset for other tests
    touchid::set_disabled(false);
}

#[test]
fn set_disabled_can_be_toggled() {
    touchid::set_disabled(true);
    assert!(!touchid::is_available());
    touchid::set_disabled(false);
    // On non-macOS CI, is_available() is still false (no hardware)
    // but the flag itself was toggled successfully
}

#[test]
fn verify_returns_ok_when_not_available() {
    // When Touch ID is not available (no hardware or disabled),
    // verify() should return Ok(()) — graceful fallback
    touchid::set_disabled(true);
    let result = touchid::verify(Tier::Sensitive, "test");
    assert!(result.is_ok());
    let result = touchid::verify(Tier::Cached, "test");
    assert!(result.is_ok());
    touchid::set_disabled(false);
}

#[test]
fn clear_returns_ok_when_not_available() {
    touchid::set_disabled(true);
    let result = touchid::clear();
    assert!(result.is_ok());
    touchid::set_disabled(false);
}
