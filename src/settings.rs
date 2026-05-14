use serde_json::Value;
use std::collections::HashMap;

/// Parsed runtime settings from the local backend's get_runtime_config response.
/// These mirror the desktop app's settings so the CLI stays in sync.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RuntimeSettings {
    pub model: String,
    pub provider: String,
    pub endpoint: String,
    pub yolo_mode: bool,
    pub stream: bool,
    pub tool_choice: String,
    pub context_window: String,
    pub enabled_functions: Vec<String>,
    pub all_functions: Vec<String>,
    pub system_prompt: Option<String>,
    pub mute: bool,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            model: "qwen-3-235b-a22b-instruct-2507".into(),
            provider: "mage lab gateway".into(),
            endpoint: "https://api.magelab.ai/v1".into(),
            yolo_mode: false,
            stream: true,
            tool_choice: "auto".into(),
            context_window: "all".into(),
            enabled_functions: Vec::new(),
            all_functions: Vec::new(),
            system_prompt: None,
            mute: true,
        }
    }
}

#[allow(dead_code)]
impl RuntimeSettings {
    /// Parse a runtime_config WebSocket response into structured settings
    pub fn from_config_map(map: &HashMap<String, Value>) -> Self {
        let mut settings = Self::default();

        if let Some(v) = map.get("llm_model_name").and_then(|v| v.as_str()) {
            settings.model = v.to_string();
        }
        if let Some(v) = map.get("llm_provider_name").and_then(|v| v.as_str()) {
            settings.provider = v.to_string();
        }
        if let Some(v) = map.get("llm_endpoint").and_then(|v| v.as_str()) {
            settings.endpoint = v.to_string();
        }
        if let Some(v) = map.get("yolo_mode").and_then(|v| v.as_bool()) {
            settings.yolo_mode = v;
        }
        if let Some(v) = map.get("stream").and_then(|v| v.as_bool()) {
            settings.stream = v;
        }
        if let Some(v) = map.get("tool_choice").and_then(|v| v.as_str()) {
            settings.tool_choice = v.to_string();
        }
        if let Some(v) = map.get("context_window").and_then(|v| v.as_str()) {
            settings.context_window = v.to_string();
        }
        if let Some(arr) = map.get("enabled_functions").and_then(|v| v.as_array()) {
            settings.enabled_functions = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
        if let Some(arr) = map.get("all_functions").and_then(|v| v.as_array()) {
            settings.all_functions = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
        if let Some(v) = map.get("sys_msg").and_then(|v| v.as_str()) {
            if !v.is_empty() {
                settings.system_prompt = Some(v.to_string());
            }
        }
        if let Some(v) = map.get("mute").and_then(|v| v.as_bool()) {
            settings.mute = v;
        }

        settings
    }

    /// Build a config update payload for pushing changes back to the backend
    pub fn build_update(&self, field: &str, value: Value) -> HashMap<String, Value> {
        let mut update = HashMap::new();
        update.insert(field.to_string(), value);
        update
    }
}
