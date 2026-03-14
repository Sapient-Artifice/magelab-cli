use magelab_cli::repl::input::{parse_slash_command, SlashCommand};

#[test]
fn test_parse_model_command() {
    let cmd = parse_slash_command("/model gpt-4o");
    assert!(matches!(cmd, Some(SlashCommand::Model(m)) if m == "gpt-4o"));
}

#[test]
fn test_parse_clear_command() {
    assert!(matches!(
        parse_slash_command("/clear"),
        Some(SlashCommand::Clear)
    ));
}

#[test]
fn test_parse_quit_command() {
    assert!(matches!(
        parse_slash_command("/quit"),
        Some(SlashCommand::Quit)
    ));
    assert!(matches!(
        parse_slash_command("/q"),
        Some(SlashCommand::Quit)
    ));
}

#[test]
fn test_parse_tools_command() {
    assert!(matches!(
        parse_slash_command("/tools"),
        Some(SlashCommand::Tools)
    ));
}

#[test]
fn test_parse_tool_toggle() {
    let cmd = parse_slash_command("/tool bash_commands");
    assert!(matches!(cmd, Some(SlashCommand::ToolToggle(t)) if t == "bash_commands"));
}

#[test]
fn test_parse_yolo_command() {
    assert!(matches!(
        parse_slash_command("/yolo"),
        Some(SlashCommand::Yolo)
    ));
}

#[test]
fn test_parse_help_command() {
    assert!(matches!(
        parse_slash_command("/help"),
        Some(SlashCommand::Help)
    ));
}

#[test]
fn test_parse_chats_command() {
    assert!(matches!(
        parse_slash_command("/chats"),
        Some(SlashCommand::Chats)
    ));
}

#[test]
fn test_parse_chat_command() {
    let cmd = parse_slash_command("/chat my-session");
    assert!(matches!(cmd, Some(SlashCommand::Chat(c)) if c == "my-session"));
}

#[test]
fn test_not_a_command() {
    assert!(parse_slash_command("hello world").is_none());
    assert!(parse_slash_command("").is_none());
}
