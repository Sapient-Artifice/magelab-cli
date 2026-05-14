use magelab_cli::detect::find_magelab_home;

#[test]
fn test_find_magelab_home_env_var() {
    // MAGELAB_HOME env var should take priority
    std::env::set_var("MAGELAB_HOME", "/tmp/fake-magelab");
    let result = find_magelab_home(None);
    assert_eq!(result, Some(std::path::PathBuf::from("/tmp/fake-magelab")));
    std::env::remove_var("MAGELAB_HOME");
}
