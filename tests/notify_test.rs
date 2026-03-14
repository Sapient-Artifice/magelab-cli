use magelab_cli::client::messages::IncomingMessage;

#[test]
fn test_deserialize_notify() {
    let json = r#"{"type":"notify","title":"Error","body":"Out of credits"}"#;
    let msg: IncomingMessage = serde_json::from_str(json).unwrap();
    match msg {
        IncomingMessage::Notify { title, body } => {
            assert_eq!(title, "Error");
            assert_eq!(body, "Out of credits");
        }
        _ => panic!("Expected Notify"),
    }
}

#[test]
fn test_deserialize_open_url() {
    let json = r#"{"type":"open_url","url":"https://example.com"}"#;
    let msg: IncomingMessage = serde_json::from_str(json).unwrap();
    match msg {
        IncomingMessage::OpenUrl { url } => {
            assert_eq!(url, "https://example.com");
        }
        _ => panic!("Expected OpenUrl"),
    }
}

#[test]
fn test_deserialize_token_count() {
    let json = r#"{"type":"token_count","sys_count":150,"win_count":45,"total_count":195}"#;
    let msg: IncomingMessage = serde_json::from_str(json).unwrap();
    match msg {
        IncomingMessage::TokenCount { total_count, .. } => {
            assert_eq!(total_count, 195);
        }
        _ => panic!("Expected TokenCount"),
    }
}
