use magelab_cli::render::results::smart_preview;
use serde_json::json;

#[test]
fn test_read_file_line_count() {
    let result = json!("line1\nline2\nline3\nline4\nline5");
    let preview = smart_preview("read_file", &result);
    assert_eq!(preview, "5 lines");
}

#[test]
fn test_bash_single_line() {
    let result = json!("hello world");
    let preview = smart_preview("bash_commands", &result);
    assert_eq!(preview, "hello world");
}

#[test]
fn test_bash_multi_line() {
    let result = json!("file1.txt\nfile2.txt\nfile3.txt");
    let preview = smart_preview("bash_commands", &result);
    assert_eq!(preview, "3 lines");
}

#[test]
fn test_bash_empty() {
    let result = json!("");
    let preview = smart_preview("bash_commands", &result);
    assert_eq!(preview, "(no output)");
}

#[test]
fn test_write_file_preview() {
    let result = json!("File written successfully");
    let preview = smart_preview("write_file", &result);
    assert!(preview.contains("written"));
}

#[test]
fn test_unknown_tool_truncates() {
    let long = "a".repeat(100);
    let result = json!(long);
    let preview = smart_preview("some_tool", &result);
    assert!(preview.len() <= 53); // 50 + "..."
    assert!(preview.ends_with("..."));
}
