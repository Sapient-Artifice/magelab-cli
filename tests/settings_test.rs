use magelab_cli::settings::RuntimeSettings;
use serde_json::json;
use std::collections::HashMap;

#[test]
fn test_parse_runtime_config() {
    let mut map: HashMap<String, serde_json::Value> = HashMap::new();
    map.insert("llm_model_name".into(), json!("gpt-4o"));
    map.insert("llm_provider_name".into(), json!("openai"));
    map.insert("yolo_mode".into(), json!(true));
    map.insert("stream".into(), json!(true));
    map.insert(
        "enabled_functions".into(),
        json!(["read_file", "write_file", "bash_commands"]),
    );
    map.insert("sys_msg".into(), json!("You are helpful."));
    map.insert("tool_choice".into(), json!("auto"));

    let settings = RuntimeSettings::from_config_map(&map);
    assert_eq!(settings.model, "gpt-4o");
    assert_eq!(settings.provider, "openai");
    assert!(settings.yolo_mode);
    assert!(settings.stream);
    assert_eq!(settings.enabled_functions.len(), 3);
    assert!(settings
        .enabled_functions
        .contains(&"read_file".to_string()));
    assert_eq!(settings.system_prompt, Some("You are helpful.".into()));
}

#[test]
fn test_default_settings() {
    let settings = RuntimeSettings::default();
    assert!(!settings.yolo_mode);
    assert!(settings.stream);
}
