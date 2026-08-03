use assert_cmd::Command;
use futures_util::{SinkExt, StreamExt};
use predicates::prelude::*;
use serde_json::{json, Value};
use std::io::Write;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[test]
fn help_lists_headless_commands() {
    let mut command = Command::cargo_bin("mage").unwrap();
    command.arg("--help").assert().success().stdout(
        predicate::str::contains("sessions")
            .and(predicate::str::contains("chats"))
            .and(predicate::str::contains("ask"))
            .and(predicate::str::contains("storage"))
            .and(predicate::str::contains("protocol")),
    );
}

#[test]
fn protocol_capabilities_json_is_machine_readable() {
    let mut command = Command::cargo_bin("mage").unwrap();
    let output = command
        .args(["protocol", "capabilities", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["client"]["request_correlation"], true);
    assert_eq!(
        value["client"]["concurrency_mode"],
        "serialized_global_runtime"
    );
}

#[test]
fn ask_requires_chat_or_new_chat_before_connecting() {
    let mut command = Command::cargo_bin("mage").unwrap();
    command
        .args(["ask", "hello", "--session", "1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "Provide --chat <id> or --new-chat",
        ));
}

#[tokio::test(flavor = "multi_thread")]
async fn ask_jsonl_keeps_stdout_machine_readable_and_sends_prompt_once() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut health, _) = listener.accept().await.unwrap();
        let mut request = vec![0_u8; 4096];
        let _ = health.read(&mut request).await.unwrap();
        let body = br#"{"status":"ok"}"#;
        health
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        health.write_all(body).await.unwrap();

        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        let mut prompts = 0;
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let message: Value = serde_json::from_str(&text).unwrap();
            match message["type"].as_str().unwrap() {
                "write_runtime_state" => {
                    socket
                        .send(Message::Text(
                            json!({
                                "type": "runtime_state_write_result",
                                "request_id": message["request_id"],
                                "ok": true
                            })
                            .to_string(),
                        ))
                        .await
                        .unwrap();
                }
                "new_chat" => {
                    socket
                        .send(Message::Text(
                            json!({
                                "type": "new_chat_result",
                                "request_id": message["request_id"],
                                "ok": true,
                                "chat_id": 91
                            })
                            .to_string(),
                        ))
                        .await
                        .unwrap();
                }
                "text" => {
                    prompts += 1;
                    let id = message["client_request_id"].clone();
                    for event in [
                        json!({"type": "assistant_stream", "phase": "delta", "token": "hello", "client_request_id": id}),
                        json!({"type": "assistant_stream", "phase": "end", "client_request_id": id}),
                        json!({"type": "assistant_complete", "client_request_id": id, "status": "completed"}),
                    ] {
                        socket.send(Message::Text(event.to_string())).await.unwrap();
                    }
                    break;
                }
                other => panic!("unexpected message: {other}"),
            }
        }
        prompts
    });

    let config_root = tempfile::tempdir().unwrap();
    let config_dir = config_root.path().join("magelab");
    std::fs::create_dir_all(&config_dir).unwrap();
    let mut config = std::fs::File::create(config_dir.join("cli.toml")).unwrap();
    writeln!(config, "local_url = \"http://{address}\"").unwrap();
    writeln!(config, "telemetry = false").unwrap();

    let output = tokio::process::Command::new(assert_cmd::cargo::cargo_bin("mage"))
        .args(["ask", "hello", "--session", "42", "--new-chat", "--jsonl"])
        .env("XDG_CONFIG_HOME", config_root.path())
        .output()
        .await
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events[0]["event"], "assistant_delta");
    assert_eq!(events.last().unwrap()["event"], "assistant_complete");
    assert_eq!(events.last().unwrap()["status"], "completed");
    assert_eq!(events.last().unwrap()["text"], "hello");
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"] == "assistant_complete")
            .count(),
        1
    );
    assert_eq!(server.await.unwrap(), 1);
}
