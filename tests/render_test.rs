use magelab_cli::render::tools::format_tool_call;

#[test]
fn test_format_tool_call_safe() {
    let output = format_tool_call("read_file", r#"{"path":"src/main.rs"}"#, true);
    assert!(output.contains("read_file"));
    assert!(output.contains("auto-approved"));
}

#[test]
fn test_format_tool_call_dangerous() {
    let output = format_tool_call("bash_commands", r#"{"command":"rm -rf /"}"#, false);
    assert!(output.contains("bash_commands"));
    assert!(!output.contains("auto-approved"));
}
