use crossterm::execute;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use std::io;

#[allow(dead_code)]
/// Format a tool call for display.
/// Returns the formatted string (for testing) and also prints it.
pub fn format_tool_call(function_name: &str, arguments: &str, auto_approved: bool) -> String {
    let args_preview = if arguments.len() > 80 {
        format!("{}...", &arguments[..77])
    } else {
        arguments.to_string()
    };

    let suffix = if auto_approved { " auto-approved" } else { "" };

    let output = format!("⚙ {}({}){}", function_name, args_preview, suffix);

    let mut stdout = io::stdout();
    let color = if auto_approved {
        Color::DarkGrey
    } else {
        Color::Yellow
    };
    execute!(
        stdout,
        SetForegroundColor(color),
        Print(format!("{}\n", &output)),
        ResetColor,
    )
    .ok();

    output
}

#[allow(dead_code)]
/// Display a tool result (file contents, command output, etc.)
pub fn display_tool_result(function_name: &str, result: &str) {
    let mut stdout = io::stdout();

    // Header
    execute!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        Print(format!("━━━ {} ━━━\n", function_name)),
    )
    .ok();

    // Content (dimmed)
    let preview = if result.len() > 2000 {
        format!(
            "{}...\n[truncated, {} bytes total]",
            &result[..2000],
            result.len()
        )
    } else {
        result.to_string()
    };
    execute!(
        stdout,
        Print(format!("{}\n", preview)),
        Print("━━━━━━━━━━━━━━━━━━\n"),
        ResetColor,
    )
    .ok();
}
