use termimad::MadSkin;

/// Create a configured markdown skin for terminal rendering
pub fn make_skin() -> MadSkin {
    MadSkin::default()
}

/// Render a complete markdown string to terminal
pub fn render(text: &str) {
    let skin = make_skin();
    skin.print_text(text);
}
