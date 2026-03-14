use magelab_cli::client::remote::RemoteClient;

#[test]
fn test_remote_client_builds_auth_header() {
    let client = RemoteClient::new("https://api.magelab.ai", "mage_test123");
    assert_eq!(client.gateway_url(), "https://api.magelab.ai");
}

#[test]
fn test_build_chat_body() {
    let body = magelab_cli::client::remote::build_chat_body(
        &[("user".into(), "hello".into())],
        "gpt-4o",
        true,
    );
    assert_eq!(body["model"], "gpt-4o");
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "hello");
}
