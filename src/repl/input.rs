#[derive(Debug, PartialEq)]
pub enum SlashCommand {
    Model(String),
    Tools,
    ToolToggle(String),
    Yolo,
    Clear,
    Chats,
    Chat(String),
    Mode,
    Config,
    Help,
    Quit,
}

/// Parse a line of input into a slash command, if it starts with /
pub fn parse_slash_command(input: &str) -> Option<SlashCommand> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
    let cmd = parts[0].to_lowercase();
    let arg = parts.get(1).map(|s| s.trim().to_string());

    match cmd.as_str() {
        "/model" | "/m" => arg.map(SlashCommand::Model),
        "/tools" => Some(SlashCommand::Tools),
        "/tool" => arg.map(SlashCommand::ToolToggle),
        "/yolo" => Some(SlashCommand::Yolo),
        "/clear" => Some(SlashCommand::Clear),
        "/chats" => Some(SlashCommand::Chats),
        "/chat" => arg.map(SlashCommand::Chat),
        "/mode" => Some(SlashCommand::Mode),
        "/config" => Some(SlashCommand::Config),
        "/help" | "/h" | "/?" => Some(SlashCommand::Help),
        "/quit" | "/q" | "/exit" => Some(SlashCommand::Quit),
        _ => None,
    }
}

/// Print the help text for slash commands
pub fn print_help() {
    println!("Commands:");
    println!("  /model <name>   Switch model");
    println!("  /tools          List enabled/disabled tools");
    println!("  /tool <name>    Toggle a tool on/off");
    println!("  /yolo           Toggle auto-approve mode");
    println!("  /clear          Reset conversation");
    println!("  /chats          List chat histories");
    println!("  /chat <name>    Switch to chat history");
    println!("  /mode           Show connection mode (local/remote)");
    println!("  /config         Show current settings");
    println!("  /help           Show this help");
    println!("  /quit           Exit");
}
