use crossterm::execute;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use std::io::{self, Write};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

/// Processes streamed tokens, detecting code fences and applying syntax highlighting.
pub struct StreamProcessor {
    buffer: String,
    in_code_block: bool,
    code_lang: String,
    code_buffer: String,
    ss: SyntaxSet,
    ts: ThemeSet,
}

impl Default for StreamProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamProcessor {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            in_code_block: false,
            code_lang: String::new(),
            code_buffer: String::new(),
            ss: SyntaxSet::load_defaults_newlines(),
            ts: ThemeSet::load_defaults(),
        }
    }

    /// Feed a token into the processor. Handles buffering for code blocks.
    pub fn push_token(&mut self, token: &str) {
        self.buffer.push_str(token);

        // Process complete lines from the buffer
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..=newline_pos].to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();
            self.process_line(&line);
        }

        // If not in a code block, flush remaining partial line directly
        if !self.in_code_block && !self.buffer.is_empty() {
            print!("{}", self.buffer);
            io::stdout().flush().ok();
            self.buffer.clear();
        }
    }

    fn process_line(&mut self, line: &str) {
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');

        if !self.in_code_block && trimmed.starts_with("```") {
            // Opening fence
            self.in_code_block = true;
            self.code_lang = trimmed.trim_start_matches('`').trim().to_string();
            self.code_buffer.clear();

            // Print language header
            let mut stdout = io::stdout();
            let lang_display = if self.code_lang.is_empty() {
                "code"
            } else {
                &self.code_lang
            };
            execute!(
                stdout,
                SetForegroundColor(Color::DarkGrey),
                Print(format!("─── {} ───\n", lang_display)),
                ResetColor,
            )
            .ok();
        } else if self.in_code_block && trimmed == "```" {
            // Closing fence — highlight and flush the buffered code
            self.flush_code_block();
            self.in_code_block = false;

            let mut stdout = io::stdout();
            execute!(
                stdout,
                SetForegroundColor(Color::DarkGrey),
                Print("────────────\n"),
                ResetColor,
            )
            .ok();
        } else if self.in_code_block {
            // Inside code block — buffer
            self.code_buffer.push_str(line);
        } else {
            // Normal text — print directly
            print!("{}", line);
            io::stdout().flush().ok();
        }
    }

    fn flush_code_block(&mut self) {
        if self.code_buffer.is_empty() {
            return;
        }

        let syntax = self
            .ss
            .find_syntax_by_token(&self.code_lang)
            .unwrap_or_else(|| self.ss.find_syntax_plain_text());

        let theme = &self.ts.themes["base16-ocean.dark"];
        let mut h = HighlightLines::new(syntax, theme);

        for line in LinesWithEndings::from(&self.code_buffer) {
            match h.highlight_line(line, &self.ss) {
                Ok(ranges) => {
                    let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
                    print!("{}", escaped);
                }
                Err(_) => {
                    // Fallback: print without highlighting
                    print!("{}", line);
                }
            }
        }
        // Reset terminal colors after highlighted output
        print!("\x1b[0m");
        io::stdout().flush().ok();

        self.code_buffer.clear();
    }

    /// Flush any remaining buffered content (call at end of stream)
    pub fn flush(&mut self) {
        if self.in_code_block && !self.code_buffer.is_empty() {
            // Unclosed code block — flush what we have
            self.flush_code_block();
            self.in_code_block = false;
        }
        if !self.buffer.is_empty() {
            print!("{}", self.buffer);
            io::stdout().flush().ok();
            self.buffer.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text_passes_through() {
        let mut sp = StreamProcessor::new();
        // Just verify it doesn't panic
        sp.push_token("hello ");
        sp.push_token("world\n");
        sp.flush();
    }

    #[test]
    fn test_code_block_detection() {
        let mut sp = StreamProcessor::new();
        sp.push_token("Here's some code:\n");
        sp.push_token("```rust\n");
        assert!(sp.in_code_block);
        sp.push_token("fn main() {}\n");
        sp.push_token("```\n");
        assert!(!sp.in_code_block);
        sp.flush();
    }

    #[test]
    fn test_code_buffer_accumulates() {
        let mut sp = StreamProcessor::new();
        sp.push_token("```python\n");
        sp.push_token("print(");
        sp.push_token("\"hello\")\n");
        assert_eq!(sp.code_buffer, "print(\"hello\")\n");
        sp.push_token("```\n");
        assert!(sp.code_buffer.is_empty());
    }
}
