use crossterm::execute;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use std::io::{self, Write};

/// Generate a smart preview string for a tool result based on the tool name
pub fn smart_preview(function_name: &str, result: &serde_json::Value) -> String {
    let text = match result {
        serde_json::Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };

    match function_name {
        "read_file" | "search_files" => {
            let line_count = text.lines().count();
            if line_count > 1 {
                format!("{} lines", line_count)
            } else {
                truncate_line(&text, 50)
            }
        }
        "bash_commands" => {
            let lines: Vec<&str> = text.lines().collect();
            if lines.is_empty() {
                "(no output)".to_string()
            } else if lines.len() == 1 {
                truncate_line(lines[0], 60)
            } else {
                format!("{} lines", lines.len())
            }
        }
        "write_file" => {
            if text.contains("written") || text.contains("created") || text.contains("saved") {
                truncate_line(&text, 50)
            } else {
                "written".to_string()
            }
        }
        "BraveSearch" | "search_web" | "search_images" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(arr) = v.as_array() {
                    return format!("{} results", arr.len());
                }
            }
            let result_count = text.matches("http").count();
            if result_count > 0 {
                format!("{} results", result_count)
            } else {
                truncate_line(&text, 50)
            }
        }
        _ => truncate_line(&text, 50),
    }
}

/// Display a full tool result below the tree (for important/long results)
pub fn display_full_result(function_name: &str, result: &serde_json::Value) {
    let text = match result {
        serde_json::Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };

    // Only display full results for read-heavy tools with substantial output
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= 3 {
        return; // Preview is enough
    }

    let mut stdout = io::stdout();

    // Header
    execute!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        Print(format!(
            "  ─── {} ({} lines) ───\n",
            function_name,
            lines.len()
        )),
    )
    .ok();

    // Show first 20 lines, dimmed
    let display_lines = if lines.len() > 20 { 20 } else { lines.len() };
    for line in &lines[..display_lines] {
        execute!(stdout, Print(format!("  {}\n", line)),).ok();
    }

    if lines.len() > 20 {
        execute!(
            stdout,
            Print(format!("  ... ({} more lines)\n", lines.len() - 20)),
        )
        .ok();
    }

    execute!(stdout, Print("  ──────────────────\n"), ResetColor,).ok();
    stdout.flush().ok();
}

fn truncate_line(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() > max {
        format!("{}...", &first_line[..max.saturating_sub(3)])
    } else {
        first_line.to_string()
    }
}
