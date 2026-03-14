use crossterm::execute;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use std::io::{self, BufRead, Write};

pub struct ApprovalPolicy {
    auto_approve_list: Vec<String>,
    yolo: bool,
}

impl ApprovalPolicy {
    pub fn new(auto_approve_list: Vec<String>, yolo: bool) -> Self {
        Self {
            auto_approve_list,
            yolo,
        }
    }

    /// Check if a tool is auto-approved (no user prompt needed)
    pub fn is_auto_approved(&self, function_name: &str) -> bool {
        self.yolo || self.auto_approve_list.iter().any(|t| t == function_name)
    }

    /// Prompt user for tool approval. Returns true if approved.
    /// Prefers `script` (human-readable command) over raw JSON arguments.
    pub fn prompt_approval(
        &self,
        function_name: &str,
        script: Option<&str>,
        arguments: &str,
    ) -> bool {
        if self.is_auto_approved(function_name) {
            return true;
        }

        let display = script.unwrap_or(arguments);
        let preview = if display.len() > 200 {
            format!("{}...", &display[..197])
        } else {
            display.to_string()
        };

        let mut stdout = io::stdout();
        execute!(
            stdout,
            SetForegroundColor(Color::Yellow),
            Print(format!("\n⚙ {} ─────────────────\n", function_name)),
            ResetColor,
            Print(format!("{}\n", preview)),
            SetForegroundColor(Color::Yellow),
            Print("[Y/n] ".to_string()),
            ResetColor,
        )
        .ok();
        stdout.flush().ok();

        let mut input = String::new();
        io::stdin().lock().read_line(&mut input).ok();
        let trimmed = input.trim().to_lowercase();

        trimmed.is_empty() || trimmed == "y" || trimmed == "yes"
    }
}
