use serde_json::{json, Value};
use std::process::Output;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn run(args: Vec<String>, local_url: String, token: Option<&str>) -> Output {
    let token = token.map(str::to_owned);
    // Windows resolves the config directory through its known-folder API, which
    // ignores APPDATA overrides. Keep subprocess requests on the mock backend.
    #[cfg(windows)]
    let args = {
        let mut args = args;
        if args
            .first()
            .is_some_and(|command| command == "skills" || command == "mcp")
            && !args.iter().any(|argument| argument == "--url")
        {
            args.extend(["--url".to_string(), local_url.clone()]);
        }
        args
    };
    tokio::task::spawn_blocking(move || {
        let temporary = tempfile::tempdir().unwrap();
        for base in [
            temporary.path().to_path_buf(),
            temporary.path().join("Library/Application Support"),
        ] {
            let dir = base.join("magelab");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("cli.toml"),
                format!("local_url = {local_url:?}\ntelemetry = false\n"),
            )
            .unwrap();
        }
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_mage"));
        command
            .args(args)
            .stdin(std::process::Stdio::null())
            .env("XDG_CONFIG_HOME", temporary.path())
            .env("APPDATA", temporary.path())
            .env("HOME", temporary.path())
            .env("USERPROFILE", temporary.path())
            .env_remove("MAGELAB_REMOTE_ACCESS_TOKEN")
            .env_remove("MAGELAB_API_KEY");
        if let Some(token) = token {
            command.env("MAGELAB_REMOTE_ACCESS_TOKEN", token);
        }
        command.output().unwrap()
    })
    .await
    .unwrap()
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn failure(output: &Output, message: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn preview() -> Value {
    json!({
        "preview_id": "preview-id", "source": {"url": "owner/repo", "revision": "123"},
        "scope": "user", "diagnostics": [],
        "choices": [
            {"id": "one", "name": "review", "full_name": "team:review", "selected": true,
             "permissions": {"allowed_tools": ["read_file"]}, "diagnostics": []},
            {"id": "two", "name": "new-skill", "full_name": "team:new-skill", "selected": false,
             "diagnostics": []}
        ]
    })
}

#[tokio::test]
async fn required_command_help_is_available_without_configuration() {
    for command in [
        vec!["skills", "add"],
        vec!["skills", "list"],
        vec!["skills", "update"],
        vec!["skills", "remove"],
        vec!["mcp", "approve"],
        vec!["mcp", "revoke"],
        vec!["mcp", "policy"],
        vec!["sessions"],
        vec!["chats"],
        vec!["ask"],
        vec!["internal", "auth-token"],
        vec!["internal", "vault-secrets"],
    ] {
        let mut arguments = args(&command);
        arguments.push("--help".into());
        let output = run(arguments, "http://127.0.0.1:1".into(), None).await;
        assert!(
            output.status.success(),
            "{command:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
    }
    let output = run(args(&["--version"]), "http://127.0.0.1:1".into(), None).await;
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        concat!("mage ", env!("CARGO_PKG_VERSION"))
    );
}

#[tokio::test]
async fn mcp_commands_set_exact_policies_and_authenticate_owner_requests() {
    let server = MockServer::start().await;
    for (command, policy) in [
        ("approve", "allow"),
        ("revoke", "prompt"),
        ("policy", "deny"),
    ] {
        Mock::given(method("PUT"))
            .and(path("/api/mcp/servers/crm.test/install_policy"))
            .and(query_param("owner_session_id", "42"))
            .and(header("authorization", "Bearer backend-test-token"))
            .and(body_json(json!({"install_policy": policy})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"server_name": "crm.test", "install_policy": policy})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let mut arguments = args(&["mcp", command, "crm.test"]);
        if command == "policy" {
            arguments.push(policy.into());
        }
        arguments.extend(args(&["--session", "42", "--json", "--url", &server.uri()]));
        let output = run(
            arguments,
            "http://127.0.0.1:1".into(),
            Some("backend-test-token"),
        )
        .await;
        assert_eq!(success(&output)["install_policy"], policy);
    }
}

#[tokio::test]
async fn mcp_global_policy_uses_configured_backend_without_session_query() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/api/mcp/servers/crm/install_policy"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"install_policy": "allow"})))
        .expect(1)
        .mount(&server)
        .await;
    let config = magelab_cli::config::Config {
        local_url: server.uri(),
        ..Default::default()
    };
    magelab_cli::managed_commands::mcp(
        &config,
        magelab_cli::managed_commands::McpCommand::Approve {
            server: "crm".into(),
            backend: magelab_cli::managed_commands::BackendOptions {
                url: None,
                session: None,
                json: true,
            },
        },
    )
    .await
    .unwrap();
    let requests = server.received_requests().await.unwrap();
    assert!(requests[0].url.query().is_none());
}

#[tokio::test]
async fn invalid_names_ids_policies_and_sessions_never_contact_backend() {
    let server = MockServer::start().await;
    for arguments in [
        args(&["mcp", "approve", "../other"]),
        args(&["mcp", "policy", "crm", "maybe"]),
        args(&["mcp", "approve", "crm", "--session", "0"]),
        args(&["skills", "update", "../other", "--yes"]),
        args(&["skills", "remove", "../other", "--yes"]),
        args(&["skills", "add", "owner/repo", "--project", "--yes"]),
    ] {
        assert!(!run(arguments, server.uri(), None).await.status.success());
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn management_rejects_redirects_without_forwarding_backend_token() {
    let source = MockServer::start().await;
    let destination = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/skills/sources"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/stolen", destination.uri())),
        )
        .expect(1)
        .mount(&source)
        .await;
    let output = run(
        args(&["skills", "list", "--json"]),
        source.uri(),
        Some("backend-test-token"),
    )
    .await;
    failure(&output, "redirects are refused");
    assert!(destination.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn management_rejects_credential_and_query_bearing_backend_urls() {
    for url in [
        "https://user:password@example.org",
        "https://example.org/?token=secret",
        "https://example.org/#x",
        "file:///tmp/backend",
    ] {
        let output = run(
            args(&["skills", "list", "--url", url]),
            "http://127.0.0.1:1".into(),
            None,
        )
        .await;
        failure(&output, "direct HTTP(S) backend URL");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("password"));
    }
}

#[tokio::test]
async fn backend_errors_remain_actionable() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({"detail": "Forbidden"})))
        .mount(&server)
        .await;
    let output = run(args(&["mcp", "approve", "crm"]), server.uri(), None).await;
    failure(&output, "403");
    failure(&output, "Forbidden");
}

#[tokio::test]
async fn skill_preview_displays_permissions_and_never_installs() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/skills/sources/preview"))
        .and(query_param("owner_session_id", "42"))
        .and(body_json(
            json!({"source": "owner/repo", "scope": "project"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(preview()))
        .expect(1)
        .mount(&server)
        .await;
    let result = success(
        &run(
            args(&[
                "skills",
                "add",
                "owner/repo",
                "--project",
                "--session",
                "42",
                "--preview",
                "--json",
            ]),
            server.uri(),
            None,
        )
        .await,
    );
    assert_eq!(
        result["choices"][0]["permissions"]["allowed_tools"],
        json!(["read_file"])
    );
    assert_eq!(result["selected_ids"], json!(["one"]));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn skill_add_keeps_owner_and_authentication_for_both_phases() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/skills/sources/preview"))
        .and(query_param("owner_session_id", "42"))
        .and(header("authorization", "Bearer backend-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(preview()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/skills/sources/install"))
        .and(query_param("owner_session_id", "42"))
        .and(header("authorization", "Bearer backend-test-token"))
        .and(body_json(
            json!({"preview_id": "preview-id", "selected_ids": ["two"]}),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"installed": [{"id": "entry"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let result = success(
        &run(
            args(&[
                "skills",
                "add",
                "owner/repo",
                "--session",
                "42",
                "--skill",
                "team:new-skill",
                "--yes",
                "--json",
            ]),
            server.uri(),
            Some("backend-test-token"),
        )
        .await,
    );
    assert_eq!(result["installed"][0]["id"], "entry");
}

#[tokio::test]
async fn skill_update_preserves_previous_selection_and_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/skills/sources/entry/update"))
        .and(query_param("owner_session_id", "42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(preview()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/skills/sources/install"))
        .and(query_param("owner_session_id", "42"))
        .and(body_json(
            json!({"preview_id": "preview-id", "selected_ids": ["one"]}),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"installed": [{"id": "entry"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    success(
        &run(
            args(&[
                "skills",
                "update",
                "entry",
                "--session",
                "42",
                "--yes",
                "--json",
            ]),
            server.uri(),
            None,
        )
        .await,
    );
}

#[tokio::test]
async fn skill_list_and_remove_share_owner_scope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/skills/sources"))
        .and(query_param("owner_session_id", "42"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"entries": [{"id": "entry"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/skills/sources/entry"))
        .and(query_param("owner_session_id", "42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"removed": "entry"})))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        success(
            &run(
                args(&["skills", "list", "--session", "42", "--json"]),
                server.uri(),
                None
            )
            .await
        )["entries"][0]["id"],
        "entry"
    );
    success(
        &run(
            args(&[
                "skills",
                "remove",
                "entry",
                "--session",
                "42",
                "--yes",
                "--json",
            ]),
            server.uri(),
            None,
        )
        .await,
    );
}

#[tokio::test]
async fn unattended_changes_require_explicit_confirmation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/skills/sources/preview"))
        .respond_with(ResponseTemplate::new(200).set_body_json(preview()))
        .expect(1)
        .mount(&server)
        .await;
    failure(
        &run(args(&["skills", "add", "owner/repo"]), server.uri(), None).await,
        "require --yes",
    );
    failure(
        &run(args(&["skills", "remove", "entry"]), server.uri(), None).await,
        "require --yes",
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn preview_errors_and_unknown_selection_prevent_installation() {
    let server = MockServer::start().await;
    let mut invalid = preview();
    invalid["choices"][0]["diagnostics"] =
        json!([{"severity": "error", "message": "Invalid component"}]);
    Mock::given(method("POST"))
        .and(path("/api/skills/sources/preview"))
        .respond_with(ResponseTemplate::new(200).set_body_json(invalid))
        .expect(2)
        .mount(&server)
        .await;
    failure(
        &run(
            args(&["skills", "add", "owner/repo", "--yes"]),
            server.uri(),
            None,
        )
        .await,
        "Resolve the preview errors",
    );
    failure(
        &run(
            args(&["skills", "add", "owner/repo", "--skill", "missing", "--yes"]),
            server.uri(),
            None,
        )
        .await,
        "must match exactly one",
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn unselected_choice_errors_do_not_block_healthy_selected_skills() {
    let server = MockServer::start().await;
    let mut reviewed = preview();
    reviewed["choices"][1]["diagnostics"] =
        json!([{"severity": "error", "message": "Unsupported optional component"}]);
    Mock::given(method("POST"))
        .and(path("/api/skills/sources/preview"))
        .respond_with(ResponseTemplate::new(200).set_body_json(reviewed))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/skills/sources/install"))
        .and(body_json(
            json!({"preview_id": "preview-id", "selected_ids": ["one"]}),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"installed": [{"id": "entry"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    success(
        &run(
            args(&["skills", "add", "owner/repo", "--yes", "--json"]),
            server.uri(),
            None,
        )
        .await,
    );
}

#[tokio::test]
async fn empty_default_selection_and_source_errors_prevent_installation() {
    for source_error in [false, true] {
        let server = MockServer::start().await;
        let mut reviewed = preview();
        if source_error {
            reviewed["diagnostics"] = json!([{"severity": "error", "message": "Invalid source"}]);
        } else {
            reviewed["choices"][0]["selected"] = json!(false);
        }
        Mock::given(method("POST"))
            .and(path("/api/skills/sources/preview"))
            .respond_with(ResponseTemplate::new(200).set_body_json(reviewed))
            .expect(1)
            .mount(&server)
            .await;
        let output = run(
            args(&["skills", "add", "owner/repo", "--yes"]),
            server.uri(),
            None,
        )
        .await;
        failure(
            &output,
            if source_error {
                "Resolve the preview errors"
            } else {
                "No skills selected"
            },
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}
