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

/// Send an analytics event. Fails silently — never blocks the CLI.
pub async fn track(event_name: &str, user_id: &str, extra_properties: Value, config: &Config) {
    if !config.telemetry() {
        return;
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

    // Fire with timeout — swallow all errors
    let _ = tokio::time::timeout(CAPTURE_TIMEOUT, posthog_rs::capture(event)).await;
}

/// Check if activation should fire for this user (pure logic, testable without PostHog).
pub fn should_track_activation(user_id: &str, config: &Config) -> bool {
    config.telemetry() && config.activated_user_id.as_deref() != Some(user_id)
}

/// Track activation if this user hasn't been activated yet.
pub async fn track_activation(user_id: &str, command: &str, config: &mut Config) {
    if !should_track_activation(user_id, config) {
        return;
    }

    track(
        "cli_activated",
        user_id,
        json!({ "command": command }),
        config,
    )
    .await;

    // Persist activation
    config.activated_user_id = Some(user_id.to_string());
    let _ = config.save();
}
