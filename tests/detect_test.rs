use magelab_cli::detect::{find_magelab_home, ConnectionMode};

#[test]
fn test_find_magelab_home_env_var() {
    // MAGELAB_HOME env var should take priority
    std::env::set_var("MAGELAB_HOME", "/tmp/fake-magelab");
    let result = find_magelab_home(None);
    assert_eq!(result, Some(std::path::PathBuf::from("/tmp/fake-magelab")));
    std::env::remove_var("MAGELAB_HOME");
}

#[test]
fn test_connection_mode_from_flags() {
    assert!(matches!(
        ConnectionMode::from_flags(true, false),
        ConnectionMode::Local
    ));
    assert!(matches!(
        ConnectionMode::from_flags(false, true),
        ConnectionMode::Remote
    ));
    assert!(matches!(
        ConnectionMode::from_flags(false, false),
        ConnectionMode::Auto
    ));
}
