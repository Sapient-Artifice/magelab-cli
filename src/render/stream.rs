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

/// Print a success message in green
pub fn print_success(msg: &str) {
    let mut stdout = io::stdout();
    execute!(
        stdout,
        SetForegroundColor(Color::Green),
        Print(format!("{}\n", msg)),
        ResetColor,
    )
    .ok();
}

/// Print a warning message in dark yellow (legible on light and dark backgrounds)
pub fn print_warn(msg: &str) {
    let mut stdout = io::stdout();
    execute!(
        stdout,
        SetForegroundColor(Color::DarkYellow),
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

/// Animated input prompt with 24-bit color pulsing cursor.
/// Shows "label ▌" with the block cursor shifting through the mage gradient.
/// On macOS Terminal.app (no truecolor) this creates a glowing cursor effect.
/// Returns the trimmed user input.
pub fn animated_prompt(label: &str) -> String {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // Hide terminal cursor so only our animated one shows
    print!("\x1b[?25l{} ", label);
    io::stdout().flush().ok();

    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    let handle = std::thread::spawn(move || {
        let frames = [
            "\x1b[38;5;141m▌\x1b[0m",
            "\x1b[38;5;135m▌\x1b[0m",
            "\x1b[38;5;128m▌\x1b[0m",
            "\x1b[38;5;93m▌\x1b[0m",
            "\x1b[38;5;57m▌\x1b[0m",
            "\x1b[38;5;55m▌\x1b[0m",
            "\x1b[38;5;57m▌\x1b[0m",
            "\x1b[38;5;91m▌\x1b[0m",
            "\x1b[38;5;93m▌\x1b[0m",
            "\x1b[38;5;128m▌\x1b[0m",
            "\x1b[38;5;135m▌\x1b[0m",
            "\x1b[38;5;177m▌\x1b[0m",
        ];
        let mut i = 0;
        while !stop_clone.load(Ordering::Relaxed) {
            print!("\x1b[1D{}", frames[i % frames.len()]);
            io::stdout().flush().ok();
            std::thread::sleep(std::time::Duration::from_millis(80));
            i += 1;
        }
        print!("\x1b[1D \x1b[1D");
        io::stdout().flush().ok();
    });

    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    stop.store(true, Ordering::Relaxed);
    handle.join().ok();

    // Restore terminal cursor
    print!("\x1b[?25h");
    io::stdout().flush().ok();

    input.trim().to_string()
}
