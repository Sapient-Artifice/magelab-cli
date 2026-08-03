use futures_util::{SinkExt, StreamExt};
use magelab_cli::client::headless::HeadlessClient;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use wiremock::{
    matchers::{body_json, method, path},
    Mock, MockServer, ResponseTemplate,
};

async fn websocket_server<F, Fut>(handler: F) -> (String, tokio::task::JoinHandle<()>)
where
    F: FnOnce(tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let socket = accept_async(stream).await.unwrap();
        handler(socket).await;
    });
    (format!("ws://{address}"), task)
}

async fn read_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    match socket.next().await.unwrap().unwrap() {
        Message::Text(text) => serde_json::from_str(&text).unwrap(),
        other => panic!("expected text message, got {other:?}"),
    }
}

async fn send_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>, value: Value)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket.send(Message::Text(value.to_string())).await.unwrap();
}

#[tokio::test]
async fn acknowledged_setup_precedes_turn_and_waits_for_terminal_completion() {
    let (url, server) = websocket_server(|mut socket| async move {
        let runtime = read_json(&mut socket).await;
        assert_eq!(runtime["type"], "write_runtime_state");
        assert_eq!(runtime["state"]["session_id"], 42);
        send_json(
            &mut socket,
            json!({
                "type": "runtime_state_write_result",
                "request_id": runtime["request_id"],
                "ok": true,
            }),
        )
        .await;

        let new_chat = read_json(&mut socket).await;
        assert_eq!(new_chat["type"], "new_chat");
        send_json(
            &mut socket,
            json!({
                "type": "new_chat_result",
                "request_id": new_chat["request_id"],
                "ok": true,
                "chat_id": 99,
            }),
        )
        .await;

        let prompt = read_json(&mut socket).await;
        assert_eq!(prompt["type"], "text");
        let id = prompt["client_request_id"].clone();
        for event in [
            json!({"type": "assistant_stream", "phase": "delta", "token": "before ", "client_request_id": id}),
            json!({"type": "assistant_stream", "phase": "end", "client_request_id": id}),
            json!({"type": "tool_result", "function_name": "lookup", "result": "ok"}),
            json!({"type": "assistant_stream", "phase": "delta", "token": "after", "client_request_id": id}),
            json!({"type": "assistant_complete", "client_request_id": id, "status": "completed"}),
        ] {
            send_json(&mut socket, event).await;
        }
    })
    .await;

    let mut client = HeadlessClient::connect(&url, None, None).await.unwrap();
    let chat_id = client
        .prepare_conversation(json!({"session_id": 42}), true, None)
        .await
        .unwrap();
    assert_eq!(chat_id, 99);

    let mut events = Vec::new();
    let result = client
        .run_turn("hello", |event| events.push(event.clone()))
        .await
        .unwrap();
    assert_eq!(result.status, "completed");
    assert_eq!(result.text, "before after");
    assert_eq!(events.last().unwrap()["type"], "assistant_complete");
    client.close().await;
    server.await.unwrap();
}

#[tokio::test]
async fn uncorrelated_terminal_event_is_rejected_as_an_old_backend() {
    let (url, server) = websocket_server(|mut socket| async move {
        let prompt = read_json(&mut socket).await;
        assert_eq!(prompt["type"], "text");
        send_json(
            &mut socket,
            json!({"type": "assistant_complete", "status": "completed"}),
        )
        .await;
    })
    .await;

    let mut client = HeadlessClient::connect(&url, None, None).await.unwrap();
    let error = client.run_turn("hello", |_| {}).await.unwrap_err();
    assert!(error.to_string().contains("v0.12.0 or newer"));
    client.close().await;
    server.await.unwrap();
}

#[tokio::test]
async fn session_creation_sends_canonical_mcp_state() {
    let http = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/sessions"))
        .and(body_json(json!({
            "name": "CRM",
            "state": {"mcps": {"enabled_servers": ["pipedrive"]}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "session": {
                "id": 7,
                "name": "CRM",
                "state": {"mcps": {"enabled_servers": ["pipedrive"]}}
            }
        })))
        .expect(1)
        .mount(&http)
        .await;

    let (url, server) =
        websocket_server(|mut socket| async move { while socket.next().await.is_some() {} }).await;
    let mut client = HeadlessClient::connect(&url, None, Some(http.uri()))
        .await
        .unwrap();
    let result = client
        .create_session(
            "CRM",
            Some(json!({"mcps": {"enabled_servers": ["pipedrive"]}})),
        )
        .await
        .unwrap();
    assert_eq!(result["session"]["id"], 7);
    client.close().await;
    server.await.unwrap();
}
