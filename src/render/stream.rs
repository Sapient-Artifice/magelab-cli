use crossterm::execute;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use std::io::{self, Write};

#[allow(dead_code)]
/// Print a single streamed token (no newline, flush immediately)
pub fn print_token(token: &str) {
    print!("{}", token);
    io::stdout().flush().ok();
}

/// Print a status message in dim color
pub fn print_status(msg: &str) {
    let mut stdout = io::stdout();
    execute!(
        stdout,
        SetForegroundColor(Color::DarkGrey),
        Print(format!("{}\n", msg)),
        ResetColor,
    )
    .ok();
}

/// Print an error message in red
pub fn print_error(msg: &str) {
    let mut stdout = io::stdout();
    execute!(
        stdout,
        SetForegroundColor(Color::Red),
        Print(format!("Error: {}\n", msg)),
        ResetColor,
    )
    .ok();
}

/// Signal end of streaming (newline if needed)
pub fn end_stream() {
    println!();
}
