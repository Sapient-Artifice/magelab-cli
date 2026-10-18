//! Direct-backend management commands. These never launch a backend or resolve a relay.
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use reqwest::{header, Method};
use serde_json::{json, Value};
use std::io::{IsTerminal, Write};
use std::time::Duration;
use url::Url;

use crate::config::Config;

#[derive(Args, Debug, Clone)]
pub struct BackendOptions {
    /// Direct backend HTTP URL (defaults to config.local_url)
    #[arg(long)]
    pub url: Option<String>,
    /// Durable session owning the project or installation policy
    #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
    pub session: Option<i64>,
    /// Output JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ReviewOptions {
    /// Show the source and permissions without installing
    #[arg(long, conflicts_with = "yes")]
    pub preview: bool,
    /// Apply the reviewed selection without an interactive prompt
    #[arg(long)]
    pub yes: bool,
    /// Select a skill by its name, full name or preview choice ID; repeat as needed
    #[arg(long = "skill")]
    pub skills: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum SkillsCommand {
    /// Preview and install a local folder, Git repository or npm package
    Add {
        source: String,
        /// Install into the selected session's project instead of the user directory
        #[arg(long, requires = "session")]
        project: bool,
        #[command(flatten)]
        backend: BackendOptions,
        #[command(flatten)]
        review: ReviewOptions,
    },
    /// List managed source installations
    List {
        #[command(flatten)]
        backend: BackendOptions,
    },
    /// Preview and apply an update, retaining the previous skill selection
    Update {
        installation_id: String,
        #[command(flatten)]
        backend: BackendOptions,
        #[command(flatten)]
        review: ReviewOptions,
    },
    /// Withdraw a managed installation without deleting the original source
    Remove {
        installation_id: String,
        #[arg(long)]
        yes: bool,
        #[command(flatten)]
        backend: BackendOptions,
    },
}

#[derive(ValueEnum, Debug, Copy, Clone)]
pub enum InstallPolicy {
    Allow,
    Prompt,
    Deny,
}

impl InstallPolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Prompt => "prompt",
            Self::Deny => "deny",
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum McpCommand {
    /// Allow future dependency installation; does not grant tool access
    Approve {
        server: String,
        #[command(flatten)]
        backend: BackendOptions,
    },
    /// Restore prompting for future dependency installation
    Revoke {
        server: String,
        #[command(flatten)]
        backend: BackendOptions,
    },
    /// Set future dependency installation policy without connecting the server
    Policy {
        server: String,
        policy: InstallPolicy,
        #[command(flatten)]
        backend: BackendOptions,
    },
}

struct Backend {
    client: reqwest::Client,
    base: Url,
    session: Option<i64>,
}

impl Backend {
    fn new(config: &Config, options: &BackendOptions) -> Result<Self> {
        if options.session.is_some_and(|id| id <= 0) {
            bail!("--session must identify a positive durable session ID");
        }
        let base = Url::parse(options.url.as_deref().unwrap_or(&config.local_url))
            .context("Invalid direct backend URL")?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            bail!("Use a direct HTTP(S) backend URL without credentials, query or fragment");
        }
        let mut headers = header::HeaderMap::new();
        if let Ok(token) = std::env::var("MAGELAB_REMOTE_ACCESS_TOKEN") {
            if !token.is_empty() {
                let mut value = header::HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| anyhow::anyhow!("Invalid MAGELAB_REMOTE_ACCESS_TOKEN"))?;
                value.set_sensitive(true);
                headers.insert(header::AUTHORIZATION, value);
            }
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(180))
            .default_headers(headers)
            .build()?;
        Ok(Self {
            client,
            base,
            session: options.session,
        })
    }

    async fn request(
        &self,
        method: Method,
        segments: &[&str],
        body: Option<Value>,
    ) -> Result<Value> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Invalid direct backend URL"))?
            .pop_if_empty()
            .extend(segments.iter().copied());
        if let Some(session) = self.session {
            url.query_pairs_mut()
                .append_pair("owner_session_id", &session.to_string());
        }
        let mut request = self.client.request(method, url);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request
            .send()
            .await
            .context("Could not contact the direct backend")?;
        let status = response.status();
        if status.is_redirection() {
            bail!("Backend returned HTTP {status}; redirects are refused for authenticated management requests");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("Could not read backend response")?
        {
            if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
                bail!("Backend response exceeds 4 MiB");
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).with_context(|| {
            format!("Backend returned HTTP {status} with an invalid JSON response")
        })?;
        if !status.is_success() {
            let detail = value.get("detail").or_else(|| value.get("error"));
            bail!(
                "Backend returned HTTP {status}: {}",
                detail.unwrap_or(&value)
            );
        }
        Ok(value)
    }
}

fn validate_installation_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("Invalid installation ID; use an ID returned by mage skills list");
    }
    Ok(())
}

fn validate_server_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
    {
        bail!("Invalid MCP server name: use 1–64 ASCII letters, digits, dots, underscores or hyphens, starting with a letter or digit");
    }
    Ok(())
}

fn print_result(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn confirm(yes: bool, question: &str) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        bail!("Noninteractive changes require --yes; review with --preview first");
    }
    eprint!("{question} [y/N] ");
    std::io::stderr().flush()?;
    let mut reply = String::new();
    std::io::stdin().read_line(&mut reply)?;
    if !matches!(reply.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        bail!("Cancelled; nothing was changed");
    }
    Ok(())
}

fn selection(preview: &Value, requested: &[String]) -> Result<Vec<String>> {
    let choices = preview
        .get("choices")
        .and_then(Value::as_array)
        .context("Backend preview is missing its choices")?;
    let mut selected = Vec::new();
    if requested.is_empty() {
        for choice in choices {
            if choice.get("selected").and_then(Value::as_bool) == Some(true) {
                selected.push(
                    choice
                        .get("id")
                        .and_then(Value::as_str)
                        .context("Backend preview choice is missing its ID")?
                        .to_string(),
                );
            }
        }
    } else {
        for requested_name in requested {
            let matches: Vec<&Value> = choices
                .iter()
                .filter(|choice| {
                    ["id", "name", "full_name"].iter().any(|key| {
                        choice.get(*key).and_then(Value::as_str) == Some(requested_name.as_str())
                    })
                })
                .collect();
            if matches.len() != 1 {
                bail!("Skill selection '{requested_name}' must match exactly one preview choice; use its full name or ID");
            }
            let id = matches[0]
                .get("id")
                .and_then(Value::as_str)
                .context("Backend preview choice is missing its ID")?
                .to_string();
            if !selected.contains(&id) {
                selected.push(id);
            }
        }
    }
    Ok(selected)
}

fn has_errors(preview: &Value, selected: &[String]) -> bool {
    let errors = |value: &Value| {
        value
            .get("diagnostics")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.get("severity").and_then(Value::as_str) == Some("error"))
            })
    };
    errors(preview)
        || preview
            .get("choices")
            .and_then(Value::as_array)
            .is_some_and(|choices| {
                choices.iter().any(|choice| {
                    choice
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| selected.iter().any(|selected| selected == id))
                        && errors(choice)
                })
            })
}

async fn apply_preview(
    backend: &Backend,
    preview: Value,
    review: ReviewOptions,
    json_output: bool,
) -> Result<()> {
    let selected = selection(&preview, &review.skills)?;
    let mut reviewed = preview.clone();
    reviewed["selected_ids"] = json!(selected);
    if review.preview {
        return print_result(&reviewed);
    }
    // All source, permission and diagnostic details remain visible before a change.
    eprintln!("{}", serde_json::to_string_pretty(&reviewed)?);
    if has_errors(&preview, &selected) {
        bail!("Resolve the preview errors before installing");
    }
    if selected.is_empty() {
        bail!("No skills selected; review the preview and select a skill with --skill");
    }
    let preview_id = preview
        .get("preview_id")
        .and_then(Value::as_str)
        .context("Backend preview is missing its ID")?;
    confirm(review.yes, "Install the selected skills and plugins?")?;
    let result = backend
        .request(
            Method::POST,
            &["api", "skills", "sources", "install"],
            Some(json!({"preview_id": preview_id, "selected_ids": selected})),
        )
        .await?;
    if !json_output {
        eprintln!("Installed. Activate the desired skills separately in the owning session.");
    }
    print_result(&result)
}

pub async fn skills(config: &Config, command: SkillsCommand) -> Result<()> {
    match command {
        SkillsCommand::Add {
            source,
            project,
            backend,
            review,
        } => {
            if project && backend.session.is_none() {
                bail!("Project installations require --session");
            }
            let client = Backend::new(config, &backend)?;
            let preview = client.request(Method::POST, &["api", "skills", "sources", "preview"],
                Some(json!({"source": source, "scope": if project { "project" } else { "user" }}))).await?;
            apply_preview(&client, preview, review, backend.json).await
        }
        SkillsCommand::List { backend } => {
            let result = Backend::new(config, &backend)?
                .request(Method::GET, &["api", "skills", "sources"], None)
                .await?;
            print_result(&result)
        }
        SkillsCommand::Update {
            installation_id,
            backend,
            review,
        } => {
            validate_installation_id(&installation_id)?;
            let client = Backend::new(config, &backend)?;
            let preview = client
                .request(
                    Method::POST,
                    &["api", "skills", "sources", &installation_id, "update"],
                    Some(json!({})),
                )
                .await?;
            apply_preview(&client, preview, review, backend.json).await
        }
        SkillsCommand::Remove {
            installation_id,
            yes,
            backend,
        } => {
            validate_installation_id(&installation_id)?;
            let client = Backend::new(config, &backend)?;
            confirm(
                yes,
                &format!("Remove managed installation {installation_id}?"),
            )?;
            let result = client
                .request(
                    Method::DELETE,
                    &["api", "skills", "sources", &installation_id],
                    None,
                )
                .await?;
            print_result(&result)
        }
    }
}

pub async fn mcp(config: &Config, command: McpCommand) -> Result<()> {
    let (server, policy, backend) = match command {
        McpCommand::Approve { server, backend } => (server, InstallPolicy::Allow, backend),
        McpCommand::Revoke { server, backend } => (server, InstallPolicy::Prompt, backend),
        McpCommand::Policy {
            server,
            policy,
            backend,
        } => (server, policy, backend),
    };
    validate_server_name(&server)?;
    let result = Backend::new(config, &backend)?
        .request(
            Method::PUT,
            &["api", "mcp", "servers", &server, "install_policy"],
            Some(json!({"install_policy": policy.as_str()})),
        )
        .await?;
    if !backend.json {
        eprintln!("Updated future dependency installation policy; retry setup to use it.");
    }
    print_result(&result)
}
