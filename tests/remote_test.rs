use magelab_cli::client::remote::RemoteClient;

#[test]
fn test_remote_client_builds_auth_header() {
    let client = RemoteClient::new("https://api.magelab.ai", "mage_test123");
    assert_eq!(client.gateway_url(), "https://api.magelab.ai");
}

#[test]
fn test_remote_client_new_with_jwt() {
    let client = RemoteClient::new(
        "https://api.magelab.ai",
        "eyJhbGciOiJSUzI1NiJ9.test",
    );
    assert_eq!(client.gateway_url(), "https://api.magelab.ai");
}

#[test]
fn test_remote_client_strips_trailing_slash() {
    let client = RemoteClient::new(
        "https://api.magelab.ai/",
        "mage_test",
    );
    assert_eq!(client.gateway_url(), "https://api.magelab.ai");
}
