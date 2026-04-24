use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Create the mage-themed alchemy spinner for waiting states.
/// Cycles through alchemical element symbols (🜁🜂🜃🜄) with a purple color pulse.
pub fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&[
                "\x1b[38;5;141m🜁\x1b[0m",
                "\x1b[38;5;135m🜁\x1b[0m",
                "\x1b[38;5;128m🜂\x1b[0m",
                "\x1b[38;5;93m🜂\x1b[0m",
                "\x1b[38;5;57m🜃\x1b[0m",
                "\x1b[38;5;55m🜃\x1b[0m",
                "\x1b[38;5;57m🜄\x1b[0m",
                "\x1b[38;5;91m🜄\x1b[0m",
                "\x1b[38;5;93m🜁\x1b[0m",
                "\x1b[38;5;128m🜁\x1b[0m",
                "\x1b[38;5;135m🜂\x1b[0m",
                "\x1b[38;5;141m🜂\x1b[0m",
                "\x1b[38;5;177m🜃\x1b[0m",
                "\x1b[38;5;141m🜃\x1b[0m",
                "\x1b[38;5;135m🜄\x1b[0m",
                "\x1b[38;5;128m🜄\x1b[0m",
                "\x1b[38;5;141m🜁\x1b[0m",
            ])
            .template("{spinner} {msg}")
            .unwrap(),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(60));
    pb
}

/// Animated input prompt with pulsing cursor.
/// Shows "label ▌" pulsing purple→white→purple→black until the user starts typing,
/// then hides the animated cursor and shows the real one for typing.
/// Returns the trimmed user input.
pub fn animated_prompt(label: &str) -> String {
    const CURSOR_FRAMES: &[&str] = &[
        // purple → white
        "\x1b[38;5;93m▌\x1b[0m",
        "\x1b[38;5;135m▌\x1b[0m",
        "\x1b[38;5;141m▌\x1b[0m",
        "\x1b[38;5;183m▌\x1b[0m",
        "\x1b[38;5;189m▌\x1b[0m",
        "\x1b[38;5;231m▌\x1b[0m",
        // white → purple
        "\x1b[38;5;189m▌\x1b[0m",
        "\x1b[38;5;183m▌\x1b[0m",
        "\x1b[38;5;141m▌\x1b[0m",
        "\x1b[38;5;135m▌\x1b[0m",
        "\x1b[38;5;93m▌\x1b[0m",
        // purple → black
        "\x1b[38;5;57m▌\x1b[0m",
        "\x1b[38;5;55m▌\x1b[0m",
        "\x1b[38;5;54m▌\x1b[0m",
        "\x1b[38;5;17m▌\x1b[0m",
        "\x1b[38;5;16m▌\x1b[0m",
        // black → purple
        "\x1b[38;5;17m▌\x1b[0m",
        "\x1b[38;5;54m▌\x1b[0m",
        "\x1b[38;5;55m▌\x1b[0m",
        "\x1b[38;5;57m▌\x1b[0m",
    ];

    // Print label, hide terminal cursor, enter raw mode
    print!("\x1b[?25l{} ", label);
    io::stdout().flush().ok();
    terminal::enable_raw_mode().ok();

    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    // Animate cursor in background
    let handle = std::thread::spawn(move || {
        let mut i = 0;
        while !stop_clone.load(Ordering::Relaxed) {
            print!("\x1b[1D{}", CURSOR_FRAMES[i % CURSOR_FRAMES.len()]);
            io::stdout().flush().ok();
            std::thread::sleep(Duration::from_millis(80));
            i += 1;
        }
        // Erase animated cursor
        print!("\x1b[1D \x1b[1D");
        io::stdout().flush().ok();
    });

    // Read keypresses in raw mode, building the input string
    let mut input = String::new();
    let mut handle = Some(handle);
    loop {
        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                // On first keypress, kill animation and show real cursor
                if !stop.load(Ordering::Relaxed) && input.is_empty() {
                    stop.store(true, Ordering::Relaxed);
                    if let Some(h) = handle.take() {
                        h.join().ok();
                    }
                    print!("\x1b[?25h"); // show terminal cursor
                    io::stdout().flush().ok();
                }

                match key.code {
                    KeyCode::Enter => {
                        println!();
                        break;
                    }
                    KeyCode::Backspace if !input.is_empty() => {
                        input.pop();
                        print!("\x1b[1D \x1b[1D");
                        io::stdout().flush().ok();
                    }
                    KeyCode::Char(c) => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                            stop.store(true, Ordering::Relaxed);
                            println!();
                            break;
                        }
                        input.push(c);
                        print!("{}", c);
                        io::stdout().flush().ok();
                    }
                    KeyCode::Esc => {
                        stop.store(true, Ordering::Relaxed);
                        println!();
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    // Ensure cleanup
    if !stop.load(Ordering::Relaxed) {
        stop.store(true, Ordering::Relaxed);
    }

    terminal::disable_raw_mode().ok();
    print!("\x1b[?25h"); // ensure cursor restored
    io::stdout().flush().ok();

    input.trim().to_string()
}

/// Print a dim label: value line to stderr.
pub fn label(name: &str, value: &str) {
    eprintln!("\x1b[38;5;245m{name}\x1b[0m {value}");
}

/// Print a success line to stderr.
pub fn success(msg: &str) {
    eprintln!("\x1b[38;5;141m🜁\x1b[0m {msg}");
}
