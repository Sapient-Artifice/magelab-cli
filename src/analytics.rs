use crate::config::Config;
use serde_json::{json, Value};
use std::time::Duration;

const POSTHOG_API_KEY: &str = "phc_B9UYq1it1AATkn1uQ6NubnQMblUx6TBeobAcWxupebu";
const POSTHOG_HOST: &str = "https://eu.i.posthog.com";
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(2);

/// Common properties attached to every analytics event.
pub fn common_properties() -> Value {
    json!({
        "cli_version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "platform": "cli",
    })
}

/// Initialize the PostHog global client. Call once from main().
pub async fn init() {
    let _ = posthog_rs::init_global((POSTHOG_API_KEY, POSTHOG_HOST)).await;
}

/// Send an analytics event. Returns true if capture succeeded.
/// Never panics or blocks the CLI beyond the timeout.
pub async fn track(
    event_name: &str,
    user_id: &str,
    extra_properties: Value,
    config: &Config,
) -> bool {
    if !config.telemetry() {
        return false;
    }

    let mut event = posthog_rs::Event::new(event_name, user_id);

    // Merge common properties
    if let Value::Object(map) = common_properties() {
        for (k, v) in map {
            let _ = event.insert_prop(k, v);
        }
    }

    // Merge extra properties
    if let Value::Object(map) = extra_properties {
        for (k, v) in map {
            let _ = event.insert_prop(k, v);
        }
    }

    match tokio::time::timeout(CAPTURE_TIMEOUT, posthog_rs::capture(event)).await {
        Ok(Ok(_)) => true,
        Ok(Err(e)) => {
            eprintln!("[analytics] capture failed: {e}");
            false
        }
        Err(_) => {
            eprintln!("[analytics] capture timed out");
            false
        }
    }
}

/// Check if activation should fire for this user (pure logic, testable without PostHog).
pub fn should_track_activation(user_id: &str, config: &Config) -> bool {
    config.telemetry() && config.activated_user_id.as_deref() != Some(user_id)
}

/// Track activation if this user hasn't been activated yet.
/// Only persists activation state if PostHog capture succeeds.
pub async fn track_activation(user_id: &str, command: &str, config: &mut Config) {
    if !should_track_activation(user_id, config) {
        return;
    }

    if !track(
        "cli_activated",
        user_id,
        json!({ "command": command }),
        config,
    )
    .await
    {
        return; // don't persist — we'll retry next run
    }

    config.activated_user_id = Some(user_id.to_string());
    if let Err(e) = config.save() {
        eprintln!("[config] failed to save activation state: {e}");
    }
}

/// Report a CLI error to PostHog. Best-effort, never blocks.
/// No-op when telemetry is disabled or PostHog is uninitialized.
#[allow(dead_code)]
pub async fn report_error(error_type: &str, message: &str, config: &Config) {
    track(
        "cli_error",
        "anonymous",
        json!({ "error_type": error_type, "message": message }),
        config,
    )
    .await;
}
