# Terminal Animation, Color, and Visual Effects -- State of the Art (2024-2026)

A comprehensive reference covering escape sequences, color systems, spinner designs,
Unicode art, image protocols, Rust crates, and advanced animation techniques for CLI tools.

---

## Table of Contents

1. [24-bit True Color ANSI](#1-24-bit-true-color-ansi)
2. [Gradient Text Effects](#2-gradient-text-effects)
3. [Animated Spinners](#3-animated-spinners)
4. [Block/Pixel Art](#4-blockpixel-art)
5. [Smooth Animation Techniques](#5-smooth-animation-techniques)
6. [Advanced Unicode Art Symbols](#6-advanced-unicode-art-symbols)
7. [Rust Crates for Terminal Graphics](#7-rust-crates-for-terminal-graphics)
8. [Sixel Graphics](#8-sixel-graphics)
9. [Kitty Graphics Protocol](#9-kitty-graphics-protocol)
10. [Cool Tricks](#10-cool-tricks)

---

## 1. 24-bit True Color ANSI

### Escape Sequences

```
Foreground:  \x1b[38;2;R;G;Bm    (R, G, B = 0..255)
Background:  \x1b[48;2;R;G;Bm
Reset:       \x1b[0m
```

Two syntax variants exist:

| Syntax       | Example                      | Notes                                    |
|--------------|------------------------------|------------------------------------------|
| Semicolon    | `\x1b[38;2;255;100;0m`      | Universally supported, legacy XTerm      |
| Colon        | `\x1b[38:2::255:100:0m`     | ISO 8613-6 compliant, newer terminals    |

**Use semicolon syntax for maximum compatibility.** The colon syntax is technically more correct
per the ITU spec but some terminals (Windows Conhost, older Konsole) only support semicolons.

### Quick Reference -- Other SGR Codes

```
\x1b[0m        Reset all
\x1b[1m        Bold
\x1b[2m        Dim
\x1b[3m        Italic
\x1b[4m        Underline
\x1b[7m        Reverse (swap fg/bg)
\x1b[8m        Hidden
\x1b[9m        Strikethrough
\x1b[22m       Normal intensity (un-bold, un-dim)
\x1b[23m       Not italic
\x1b[24m       Not underlined
\x1b[27m       Not reversed
\x1b[29m       Not strikethrough
```

### Terminal Support (2024-2026)

| Terminal           | True Color | Notes                                   |
|--------------------|:----------:|-----------------------------------------|
| iTerm2             | Yes        | COLORTERM=truecolor set automatically   |
| Kitty              | Yes        | Native, plus graphics protocol          |
| Alacritty          | Yes        | GPU-accelerated                         |
| WezTerm            | Yes        | Plus Sixel, Kitty protocol              |
| Ghostty            | Yes        | Launched Dec 2024, full support         |
| GNOME Terminal     | Yes        | >= 3.28, VTE-based                      |
| Konsole            | Yes        | KDE                                     |
| Windows Terminal   | Yes        | Since v1.0; Sixel added in v1.22        |
| VS Code Terminal   | Yes        | Sixel support added recently            |
| macOS Terminal.app | **No**     | Stuck at 256 colors                     |
| tmux               | Yes        | Requires `set -g default-terminal "tmux-256color"` + `set -as terminal-features ',*:RGB'` |

### Detection Methods

```bash
# Method 1: Check COLORTERM env var (most reliable simple check)
if [ "$COLORTERM" = "truecolor" ] || [ "$COLORTERM" = "24bit" ]; then
    echo "24-bit color supported"
fi

# Method 2: Check TERM variable (less reliable)
case "$TERM" in
    *-256color|*-direct) echo "likely truecolor" ;;
esac

# Method 3: Query terminal with DECRQM (programmatic)
# Send: \x1b[?2031$p
# Response tells you if RGB mode is available
```

**Rust detection pattern:**

```rust
fn supports_truecolor() -> bool {
    if let Ok(ct) = std::env::var("COLORTERM") {
        return ct == "truecolor" || ct == "24bit";
    }
    // Fallback: check TERM for known truecolor terminals
    if let Ok(term) = std::env::var("TERM") {
        return term.contains("256color") || term.contains("direct");
    }
    false
}
```

**Limitations:** COLORTERM is not forwarded through `sudo`, `ssh`, or `docker exec` by default.
Applications should degrade gracefully: truecolor -> 256-color -> 16-color -> no color.

---

## 2. Gradient Text Effects

### Core Technique

Assign each character a different RGB value sampled along a gradient curve. For each
character at position `i` in a string of length `n`:

```
t = i / (n - 1)                     # normalized position [0..1]
r = lerp(start_r, end_r, t)         # linear interpolation
g = lerp(start_g, end_g, t)
b = lerp(start_b, end_b, t)
print("\x1b[38;2;{r};{g};{b}m{char}")
```

### HSL vs RGB Interpolation

**RGB interpolation** produces muddy midpoints (e.g., red-to-blue goes through dark purple).

**HSL/HSV interpolation** rotates through the hue wheel, producing vibrant rainbows:

```
For each character at position i:
    hue = start_hue + (end_hue - start_hue) * (i / n)
    (r, g, b) = hsl_to_rgb(hue, saturation, lightness)
    emit \x1b[38;2;R;G;Bm
```

**OKLCH interpolation** (perceptually uniform) is the gold standard for smooth gradients.
Colors maintain consistent perceived brightness across the spectrum.

### lolcat Technique

lolcat uses a sinusoidal rainbow. Each character gets a hue from:

```python
def rainbow(freq, i):
    r = math.sin(freq * i + 0) * 127 + 128
    g = math.sin(freq * i + 2 * math.pi / 3) * 127 + 128
    b = math.sin(freq * i + 4 * math.pi / 3) * 127 + 128
    return (int(r), int(g), int(b))
```

The three sine waves are offset by 120 degrees (2pi/3), producing a smooth RGB rainbow.
The phase shifts over time for animated scrolling effects. Implementations exist in
Ruby (original), C (c-lolcat, faster), Python, and Rust.

### gradient-string (Node.js)

Supports multi-stop gradients and built-in presets:

```javascript
import gradient from 'gradient-string';

// Built-in presets
console.log(gradient.rainbow('Hello world'));
console.log(gradient.pastel('Hello world'));
console.log(gradient.cristal('Hello world'));

// Custom gradient with color stops
const cool = gradient(['#FF0000', '#00FF00', '#0000FF']);
console.log(cool('Hello world'));

// Multiline: same gradient applied vertically-aligned across lines
console.log(gradient.rainbow.multiline(asciiArt));
```

The `multiline()` method is critical for ASCII art -- it ensures colors align vertically
across lines rather than restarting the gradient per line.

### Rust Implementation Pattern

```rust
fn gradient_text(text: &str, start: (u8, u8, u8), end: (u8, u8, u8)) -> String {
    let len = text.chars().count().max(1) as f32;
    text.chars()
        .enumerate()
        .map(|(i, c)| {
            let t = i as f32 / (len - 1.0).max(1.0);
            let r = (start.0 as f32 + (end.0 as f32 - start.0 as f32) * t) as u8;
            let g = (start.1 as f32 + (end.1 as f32 - start.1 as f32) * t) as u8;
            let b = (start.2 as f32 + (end.2 as f32 - start.2 as f32) * t) as u8;
            format!("\x1b[38;2;{r};{g};{b}m{c}")
        })
        .collect::<String>()
        + "\x1b[0m"
}
```

---

## 3. Animated Spinners

### Spinner Architecture

A spinner is fundamentally:
1. An array of **frames** (character sequences)
2. An **interval** (ms between frames)
3. A render loop: print frame, wait, erase, print next frame

```
loop {
    print!("\r{} {}", frames[i % frames.len()], message);
    flush();
    sleep(interval);
    i += 1;
}
```

### The cli-spinners Catalog

The definitive collection: [sindresorhus/cli-spinners](https://github.com/sindresorhus/cli-spinners).
Over 80 spinner designs. Key ones:

**Braille dots (the modern standard):**
```
dots:   ⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏   (80ms, the default for ora)
dots2:  ⣾ ⣽ ⣻ ⢿ ⡿ ⣟ ⣯ ⣷         (80ms)
dots3:  ⠋ ⠙ ⠚ ⠞ ⠖ ⠦ ⠴ ⠲ ⠳ ⠓     (80ms)
dots8Bit: all 256 braille patterns    (80ms, 256 frames)
dots12: complex 56-frame sequence     (80ms)
```

**Classic:**
```
line:   - \ | /                       (130ms, the Windows fallback)
pipe:   ┤ ┘ ┴ └ ├ ┌ ┬ ┐              (100ms)
arc:    ◜ ◠ ◝ ◞ ◡ ◟                  (100ms)
```

**Progress-style:**
```
bouncingBar:  [    =   ] -> [   =    ] -> etc  (80ms, 16 frames)
aesthetic:    ▰▱▱▱▱▱▱ -> ▰▰▱▱▱▱▱ -> etc        (80ms)
material:    ━━━━━━━━━━ (99 frames, 17ms!)     (the fastest)
```

**Emoji:**
```
moon:    🌑 🌒 🌓 🌔 🌕 🌖 🌗 🌘    (80ms)
earth:   🌍 🌎 🌏                     (180ms)
clock:   🕛 🕐 🕑 🕒 ... 🕚           (100ms, 12 frames)
hearts:  💛 💙 💜 💚 ❤️               (100ms)
weather: 🌤 🌥 🌦 🌧 ... (23 frames)  (100ms)
```

**Creative/narrative:**
```
shark:        (120ms, 26 frames -- swimming animation)
pong:         (80ms, 30 frames -- pong game)
dwarfFortress (80ms, 128 frames -- narrative text!)
```

### Survey of Notable CLI Spinners

#### Bun

Bun's install output is distinctively minimal -- it shows a progress indicator with package
counts and speeds rather than a traditional spinner. Written in Zig, it uses direct terminal
writes for maximum performance. The output style emphasizes speed metrics and brevity over
decorative animation.

#### Turbo (Turborepo)

Turbo 2.0+ uses a full TUI built with **ratatui** (Rust). Instead of a simple spinner, it
renders a split-screen interface: task list on the left with status icons, scrollable logs on
the right. Status uses colored symbols rather than animated spinners:
- Green checkmark for completed
- Yellow dot for running
- Red X for failed

This represents the trend toward full TUI experiences instead of traditional spinner+text output.

#### Biome

Biome (Rust-based linter/formatter) focuses on speed-first output. Its CLI shows contextual
diagnostics with color-coded severity rather than traditional spinners. Operations complete
so fast that spinners are rarely needed -- a philosophy of "be fast enough that you don't
need a loading indicator."

#### Vercel CLI

The Vercel CLI uses a minimalist approach informed by their Geist design system:
- Simple dot spinner or line spinner
- Show-delay of ~150-300ms (don't show spinner if operation completes fast)
- Minimum visible time of ~300-500ms (avoid flash of spinner)
- Clean, monochrome aesthetic with strategic color accents
- Avoids "streamed logs can't be interactive" problem by separating concerns

#### Claude Code

Claude Code uses a multi-character "flower" animation with these frames:

```
·  ✢  ✳  ∗  ✻  ✽
```

Key design details (reverse-engineered by Kyle Martinez and Alex Beals):
- Characters cycle through asterisk/star variants of increasing complexity
- Clever **easing**: first and last characters hold slightly longer than middle frames
- 50ms animation loop (isolated to reduce CPU overhead and UI frame drops)
- Terminal title also animates with braille spinners: ⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏
- "Shimmer" gradient effect on the thinking text (introduced ~v1.0.73-v1.0.81)
- Customizable "spinner verbs" displayed alongside the animation
- Accessibility: shimmer can be disabled for users who find animation distracting

#### npm/yarn/pnpm

- **npm**: Uses ora-style braille spinner during install, progress bar for downloads
- **yarn**: Custom progress bar with package count, uses cli-spinners dots variant
- **pnpm**: Minimal output philosophy; progress bar was a frequently requested feature

### What Makes a Good Spinner

1. **Smooth perceived motion** -- Braille dots win because the "movement" is continuous
2. **Consistent character width** -- Avoid emoji (variable width) in fixed-layout contexts
3. **Appropriate speed** -- 60-100ms feels active; 200ms+ feels sluggish
4. **Show-delay** -- Don't show the spinner for operations under ~150ms
5. **Minimum display** -- Once shown, keep visible for at least ~300ms to avoid flicker
6. **Easing** -- Hold first/last frames slightly longer for organic feel
7. **Monospace-safe** -- Test in multiple fonts; braille patterns are safest
8. **Message updates** -- Update the text alongside the spinner on meaningful events
9. **Completion states** -- Green checkmark on success, red X on failure, clear the spinner line

---

## 4. Block/Pixel Art

### Unicode Half-Block Image Rendering

The core technique used by TerminalImageViewer, timg, viuer, and similar tools:

Each terminal cell renders **two vertical pixels** using the lower half block character:

```
▄ (U+2584 Lower Half Block)

Foreground color = bottom pixel
Background color = top pixel
```

This doubles the vertical resolution. A 80x24 terminal becomes effectively 80x48 pixels.

```
// For each pair of vertically adjacent pixels:
\x1b[38;2;Rb;Gb;Bbm      // foreground = bottom pixel color
\x1b[48;2;Rt;Gt;Btm      // background = top pixel color
▄                          // render half-block
```

If both pixels are the same color, use a space with just the background color set.

### Advanced Block Graphics (TerminalImageViewer approach)

For higher quality, map 4x8 pixel cells to Unicode block characters:

1. For each 4x8 pixel cell, find the color channel (R, G, or B) with the biggest range
2. Split the range at the midpoint to create a 1-bit bitmap
3. Compare bitmap against known Unicode block character shapes
4. Pick the closest matching character
5. Set foreground = average color of "1" pixels, background = average of "0" pixels

This uses characters from Box Drawing (U+2500-U+257F) and Block Elements (U+2580-U+259F)
to achieve much higher fidelity than simple half-blocks.

### Braille Pseudo-Pixels

Each braille character (U+2800-U+28FF) encodes a 2x4 dot grid, giving **8 sub-pixels**
per character cell. The [drawille](https://github.com/asciimoo/drawille) library pioneered
this technique.

```
Braille dot positions:     Binary encoding:
  1  4                     Dot 1 = bit 0  (0x01)
  2  5                     Dot 2 = bit 1  (0x02)
  3  6                     Dot 3 = bit 2  (0x04)
  7  8                     Dot 4 = bit 3  (0x08)
                           Dot 5 = bit 4  (0x10)
                           Dot 6 = bit 5  (0x20)
                           Dot 7 = bit 6  (0x40)
                           Dot 8 = bit 7  (0x80)

Character = U+2800 + bitmask

Example: dots 1,2,4 set = 0x01 | 0x02 | 0x08 = 0x0B
         Character = U+280B = ⠋  (the first frame of the "dots" spinner!)
```

An 80x24 terminal becomes **160x96 effective pixels** with braille rendering.
The trade-off: braille is monochrome per cell (one fg color, one bg color),
whereas half-blocks give full color per pixel-pair.

### Shade Characters

```
░  U+2591  Light shade    (25% fill)
▒  U+2592  Medium shade   (50% fill)
▓  U+2593  Dark shade     (75% fill)
█  U+2588  Full block     (100% fill)
```

Combined with 24-bit color, these give 4 levels of opacity per character cell.

### Quadrant Blocks

```
▖  U+2596  Lower left
▗  U+2597  Lower right
▘  U+2598  Upper left
▙  U+2599  Upper left + lower left + lower right
▚  U+259A  Upper left + lower right (diagonal)
▛  U+259B  Upper left + upper right + lower left
▜  U+259C  Upper left + upper right + lower right
▝  U+259D  Upper right
▞  U+259E  Upper right + lower left (diagonal)
▟  U+259F  Upper right + lower left + lower right
```

These allow 2x2 sub-pixel grids per character cell, giving 4 pixels each with
independent on/off state (the fg/bg colors still shared per cell).

---

## 5. Smooth Animation Techniques

### Cursor Movement Escapes

```
\x1b[nA       Move cursor up n lines
\x1b[nB       Move cursor down n lines
\x1b[nC       Move cursor right n columns
\x1b[nD       Move cursor left n columns
\x1b[nE       Move to beginning of line n lines down
\x1b[nF       Move to beginning of line n lines up
\x1b[nG       Move to column n
\x1b[n;mH     Move to row n, column m (1-indexed)
\x1b[s        Save cursor position (SCO)
\x1b[u        Restore cursor position (SCO)
\x1b7         Save cursor position (DEC, more portable)
\x1b8         Restore cursor position (DEC, more portable)
```

### Line/Screen Clearing

```
\x1b[0K       Clear from cursor to end of line
\x1b[1K       Clear from start of line to cursor
\x1b[2K       Clear entire line
\x1b[0J       Clear from cursor to end of screen
\x1b[1J       Clear from start of screen to cursor
\x1b[2J       Clear entire screen
\x1b[3J       Clear screen and scrollback buffer
```

### Alternate Screen Buffer

```
\x1b[?1049h   Enter alternate screen (saves main buffer)
\x1b[?1049l   Leave alternate screen (restores main buffer)
```

Used by full-screen TUIs (vim, less, htop). The alternate screen is a separate buffer --
cursor position, content, and scroll state are independent. When you leave, the original
terminal content is restored cleanly.

### Cursor Visibility

```
\x1b[?25l     Hide cursor
\x1b[?25h     Show cursor
```

Always hide cursor during animation to prevent flickering cursor artifacts.

### The Double-Buffering Technique

For flicker-free multi-line animation without the alternate screen:

```
Frame 1:                    Frame 2:
1. Hide cursor              1. Move cursor back to frame start
2. Print all lines          2. Print all lines (overwrites old)
3. Flush stdout             3. Flush stdout
4. Record cursor position   4. (repeat)
```

**Practical pattern:**

```rust
// Save position, render frame, restore position
print!("\x1b[s");                    // save cursor
print!("\x1b[?25l");                 // hide cursor
for line in frame_lines {
    print!("\x1b[2K{}\n", line);     // clear line, print, newline
}
stdout().flush();
print!("\x1b[u");                    // restore cursor to start
```

Or more simply for single-line spinners:

```rust
print!("\r\x1b[2K{}", frame);       // carriage return, clear line, print
stdout().flush();
```

### Synchronized Output (DEC Mode 2026)

The modern solution for flicker-free rendering. Tells the terminal to batch all output
and render atomically:

```
\x1b[?2026h     Begin synchronized update (start batching)
... write all your frame data ...
\x1b[?2026l     End synchronized update (render atomically)
```

**How it works:** When enabled, the terminal keeps processing incoming text and escape
sequences into an internal buffer but does not repaint the screen. When disabled again,
it repaints with the final state -- effectively an atomic frame update with no tearing.

**Terminal support (2024-2026):**

| Terminal         | Supported | Since     |
|------------------|:---------:|-----------|
| Kitty            | Yes       | Early     |
| WezTerm          | Yes       | Early     |
| Ghostty          | Yes       | 1.0 (Dec 2024) |
| Alacritty        | Yes       | Recent    |
| Windows Terminal | Yes       | PR #18826 |
| foot             | Yes       |           |
| iTerm2           | Yes       | 3.5+      |
| Contour          | Yes       |           |
| macOS Terminal   | No        |           |
| tmux             | Partial   | 3.4+      |

**Detection:** Query with `\x1b[?2026$p` (DECRQM). Possible responses:
- `\x1b[?2026;1$y` -- set (enabled)
- `\x1b[?2026;2$y` -- reset (disabled but supported)
- `\x1b[?2026;0$y` -- not recognized
- No response -- not supported (set a timeout)

**Best practice:** Always use synchronized output when available, fall back to
cursor-save/restore double-buffering when not.

```rust
fn render_frame(content: &str, sync_supported: bool) {
    let mut out = std::io::stdout();
    if sync_supported {
        write!(out, "\x1b[?2026h").unwrap();  // begin sync
    }

    write!(out, "\x1b[s\x1b[?25l").unwrap();  // save pos, hide cursor
    write!(out, "{}", content).unwrap();
    write!(out, "\x1b[u\x1b[?25h").unwrap();  // restore pos, show cursor

    if sync_supported {
        write!(out, "\x1b[?2026l").unwrap();  // end sync (atomic render)
    }
    out.flush().unwrap();
}
```

---

## 6. Advanced Unicode Art Symbols

### Braille Patterns (U+2800-U+28FF) -- Pseudo-Pixel Grids

256 characters encoding all combinations of an 8-dot (2x4) grid.

```
⠀ ⠁ ⠂ ⠃ ⠄ ⠅ ⠆ ⠇ ⠈ ⠉ ⠊ ⠋ ⠌ ⠍ ⠎ ⠏
⠐ ⠑ ⠒ ⠓ ⠔ ⠕ ⠖ ⠗ ⠘ ⠙ ⠚ ⠛ ⠜ ⠝ ⠞ ⠟
⠠ ⠡ ⠢ ⠣ ⠤ ⠥ ⠦ ⠧ ⠨ ⠩ ⠪ ⠫ ⠬ ⠭ ⠮ ⠯
⠰ ⠱ ⠲ ⠳ ⠴ ⠵ ⠶ ⠷ ⠸ ⠹ ⠺ ⠻ ⠼ ⠽ ⠾ ⠿
⡀ ⡁ ... ⣾ ⣿
```

**Use cases:** Graphs, charts, scatter plots, high-res terminal art, spinners.
The "dots" spinner family is built from these.

### Box Drawing (U+2500-U+257F) -- Lines and Corners

```
Single lines:     ─ │ ┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼
Double lines:     ═ ║ ╔ ╗ ╚ ╝ ╠ ╣ ╦ ╩ ╬
Rounded corners:  ╭ ╮ ╰ ╯
Light/heavy mix:  ┍ ┎ ┑ ┒ ┕ ┖ ┙ ┚
Dashed:           ┄ ┅ ┆ ┇ ┈ ┉ ┊ ┋
```

**Use cases:** Borders, tables, tree views, layout frames.
Rounded corners (╭╮╰╯) give a modern, softer look compared to sharp ┌┐└┘.

### Block Elements (U+2580-U+259F) -- Fills and Halves

```
Upper half:       ▀  (U+2580)
Lower half:       ▄  (U+2584)
Full block:       █  (U+2588)
Left half:        ▌  (U+258C)
Right half:       ▐  (U+2590)
Shades:           ░ ▒ ▓  (light, medium, dark)

Quadrant blocks:  ▖ ▗ ▘ ▙ ▚ ▛ ▜ ▝ ▞ ▟
                  (2x2 sub-cell combinations)

Vertical fills:   ▁ ▂ ▃ ▄ ▅ ▆ ▇ █  (1/8 to 8/8)
Horizontal fills: ▏ ▎ ▍ ▌ ▋ ▊ ▉ █  (1/8 to 8/8)
```

**Use cases:** Progress bars, image rendering, bar charts, density plots, sparklines.

### Geometric Shapes (U+25A0-U+25FF)

```
Squares:          ■ □ ▢ ▣ ▤ ▥ ▦ ▧ ▨ ▩
Circles:          ● ○ ◉ ◎ ◌ ◍ ◯
Triangles:        ▲ △ ▴ ▵ ▶ ▷ ▸ ▹ ► ▻
                  ▼ ▽ ▾ ▿ ◀ ◁ ◂ ◃ ◄ ◅
Diamonds:         ◆ ◇ ◈
Stars:            ★ ☆
Arc quadrants:    ◜ ◝ ◞ ◟ (used in the "arc" spinner)
Half circles:     ◐ ◑ ◒ ◓
```

**Use cases:** Bullet points, status indicators, directional cues, spinner frames.

### Dingbats and Miscellaneous Symbols

```
Stars/asterisks:  ✢ ✣ ✤ ✥ ✦ ✧ ✨ ✩ ✪ ✫ ✬ ✭ ✮ ✯ ✰
                  ✱ ✲ ✳ ✴ ✵ ✶ ✷ ✸ ✹ ✺ ✻ ✼ ✽ ✾ ✿
Crosses:          ✕ ✖ ✗ ✘ ✙ ✚ ✛ ✜
Checks:           ✓ ✔
Arrows:           ➔ ➜ ➝ ➞ ➟ ➠ ➡ ➢ ➣ ➤ ➥ ➦ ➧ ➨
Dots:             · • ∙ ⋅ ⏺
Flowers:          ❀ ❁ ❂ ❃ ✿
Hearts:           ❤ ❥ ❦ ❧
```

Claude Code uses `· ✢ ✳ ∗ ✻ ✽` from this range for its spinner.

### Alchemical Symbols (U+1F700-U+1F77F)

124 characters from historical alchemy texts. Introduced in Unicode 6.0 (2010).

```
Elements:
🜀  Quintessence      🜁  Air       🜂  Fire      🜃  Earth     🜄  Water

Substances:
🜅  Aqua Fortis       🜆  Aqua Regia       🜈  Aqua Vitae
🜍  Sulfur            🜔  Salt              🜕  Nitre
🜖  Vitriol           🜘  Rock Salt

Metals:
🜚  Gold              🜛  Silver            🜜  Iron Ore
🜠  Copper Ore        🜤  Tin Ore           🜥  Lead Ore
🜦  Antimony Ore      🜧  Bismuth Ore

Apparatus:
🜯  Alembic           🜰  Bath              🜱  Crucible (1)
🜲  Crucible (2)

Processes:
🜭  Caput Mortuum     🜪  Powder            🜫  Calx
```

**Use cases:** Distinctive branding, mystical/arcane aesthetic, unique status indicators.
These are uncommon enough that most users won't have seen them, making them memorable.
**Warning:** Font support varies significantly. Test across target terminals. Many system
fonts include these, but some monospace fonts do not.

### Musical Symbols, Zodiac, and Miscellaneous

```
Musical:  ♩ ♪ ♫ ♬ ♭ ♮ ♯
Zodiac:   ♈ ♉ ♊ ♋ ♌ ♍ ♎ ♏ ♐ ♑ ♒ ♓
Chess:    ♔ ♕ ♖ ♗ ♘ ♙ ♚ ♛ ♜ ♝ ♞ ♟
Cards:    ♠ ♡ ♢ ♣ ♤ ♥ ♦ ♧
Misc:     ⚡ ⚙ ⚗ ⚛ ⚜ ⚝ ☀ ☁ ☂ ☃ ☄ ☮ ☯ ☸ ☕
Weather:  ☀ ☁ ☂ ⛅ ⛈ 🌤 🌥 🌦 🌧 🌨 🌩 🌪
```

---

## 7. Rust Crates for Terminal Graphics

### ratatui -- TUI Framework

**The** TUI framework for Rust (fork of tui-rs, which is no longer maintained as of Aug 2023).

```toml
[dependencies]
ratatui = "0.30"           # modular workspace since 0.30
ratatui-crossterm = "0.30" # crossterm backend (default)
```

**Features:**
- Immediate-mode rendering with double-buffered `Terminal`
- Layout engine (constraints, flex, grid)
- Widgets: Block, Paragraph, List, Table, Tabs, Gauge, Chart, Canvas, Sparkline, BarChart
- Style system with full 24-bit color support
- Modular workspace since 0.30 (better compilation times)

**When to use:** Full-screen TUI applications. Turborepo's TUI is built on ratatui.
Not suitable for inline CLI output (spinners, progress bars) -- use indicatif for that.

### ratatui-image -- Image Widget

```toml
[dependencies]
ratatui-image = "4"
```

Renders images inside ratatui using the best available protocol:
1. Kitty graphics protocol
2. iTerm2 inline images
3. Sixel
4. Unicode half-blocks (fallback)

Handles protocol detection, font-size querying, and rendering automatically.

### crossterm -- Low-Level Terminal Control

```toml
[dependencies]
crossterm = "0.28"
```

Cross-platform terminal manipulation (Linux, macOS, Windows 7+). Pure Rust, no C dependencies.

**Key APIs:**

```rust
use crossterm::{
    cursor::{Hide, Show, MoveTo, SavePosition, RestorePosition},
    terminal::{
        Clear, ClearType,
        EnterAlternateScreen, LeaveAlternateScreen,
        enable_raw_mode, disable_raw_mode,
        size,  // (cols, rows)
    },
    style::{
        SetForegroundColor, SetBackgroundColor,
        Color, Attribute, Print,
        Stylize,  // extension trait for .bold(), .red(), etc.
    },
    event::{self, Event, KeyCode, KeyEvent},
    execute, queue,
};

// Immediate execution (flushes after each command)
execute!(stdout(), Hide, MoveTo(0, 0), Print("Hello"))?;

// Queued execution (batched, single flush -- better for frames)
queue!(stdout(), Hide, MoveTo(0, 0), Print("Frame content"))?;
stdout().flush()?;

// True color
queue!(stdout(),
    SetForegroundColor(Color::Rgb { r: 139, g: 92, b: 246 }),
    Print("Purple text"),
    SetForegroundColor(Color::Reset),
)?;
```

**When to use:** Direct terminal control, custom rendering, building your own TUI framework,
or when you need more control than indicatif provides.

### indicatif -- Progress Bars and Spinners

```toml
[dependencies]
indicatif = "0.17"
```

**Features:**
- Progress bars with customizable templates
- Spinners with custom frame sequences
- Multi-progress (multiple bars/spinners simultaneously)
- Elapsed time, ETA, throughput display
- Terminal width awareness
- Steady tick (background animation thread)

```rust
use indicatif::{ProgressBar, ProgressStyle, MultiProgress};

// Spinner
let spinner = ProgressBar::new_spinner();
spinner.set_style(
    ProgressStyle::default_spinner()
        .tick_strings(&["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏","✔"])
        .template("{spinner:.cyan} {msg}")
        .unwrap()
);
spinner.set_message("Loading...");
spinner.enable_steady_tick(std::time::Duration::from_millis(80));
// ... work ...
spinner.finish_with_message("Done!");

// Progress bar
let bar = ProgressBar::new(100);
bar.set_style(
    ProgressStyle::default_bar()
        .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
        .unwrap()
        .progress_chars("━╸─")  // filled, current, empty
);

// Multi-progress (nested spinners/bars)
let mp = MultiProgress::new();
let bar1 = mp.add(ProgressBar::new(100));
let bar2 = mp.add(ProgressBar::new(200));
```

**Note:** The last entry in `tick_strings` is the "finished" state shown when
`spinner.finish()` is called.

### console -- Terminal Utilities

```toml
[dependencies]
console = "0.15"
```

Higher-level utilities by the same team as indicatif:

```rust
use console::{style, Emoji, Term, measure_text_width};

let term = Term::stdout();
let width = term.size().1;  // terminal width

// Styled text
println!("{}", style("Success").green().bold());
println!("{}", style("Error").red().bold());
println!("{}", style("Dimmed").dim());

// Emoji with ASCII fallback
static CHECKMARK: Emoji<'_, '_> = Emoji("✔️ ", "OK ");
println!("{} Done", CHECKMARK);

// Color detection
if console::colors_enabled() { /* use colors */ }

// Truncation and padding
let s = console::truncate_str("long text", 10, "...");
let s = console::pad_str("short", 20, console::Alignment::Left, None);
```

### owo-colors -- Zero-Cost Text Coloring

```toml
[dependencies]
owo-colors = "5"  # v5 released Sep 2024
```

**Key differentiator:** Zero allocation, `no_std` compatible, zero-cost abstractions.
Drop-in replacement for the `colored` crate but works in embedded/no_std contexts.

```rust
use owo_colors::OwoColorize;

println!("{}", "Hello".red());
println!("{}", "World".on_blue().white().bold());
println!("{}", "RGB".truecolor(255, 128, 0));

// Conditional coloring based on stream
use owo_colors::Stream;
println!("{}", "colored if tty".if_supports_color(Stream::Stdout, |text| text.red()));
```

Automatically respects `NO_COLOR` and `FORCE_COLOR` environment variables. Detects TTY
and CI environments. The recommended choice per Rain's Rust CLI recommendations.

### Additional / Newer Crates

| Crate              | Purpose                                    | Notes                          |
|--------------------|--------------------------------------------|--------------------------------|
| `colorgrad`        | Color gradient library                     | Multi-stop gradients, many color spaces |
| `termbg`           | Detect terminal background color           | Useful for adaptive theming    |
| `termcolor`        | Cross-platform terminal colors             | Used by ripgrep                |
| `yansi`            | Paint text with ANSI colors                | Minimal, fast                  |
| `nu-ansi-term`     | ANSI terminal colors (Nushell fork)        | Active maintenance             |
| `dialoguer`        | User prompts and input                     | By the indicatif/console team  |
| `comfy-table`      | Beautiful terminal tables                  | Adaptive column widths         |
| `tabled`           | Table formatting                           | Derive macro support           |
| `unicode-width`    | Calculate display width of Unicode         | Essential for correct layout   |
| `textwrap`         | Word wrapping with terminal width          | Hyphenation support            |
| `syntect`          | Syntax highlighting                        | Used by bat, delta             |
| `ansi-to-tui`      | Parse ANSI text into ratatui widgets       | Bridge library                 |

---

## 8. Sixel Graphics

### What is Sixel?

Sixel ("six pixels") is a bitmap graphics format originally from DEC terminals (VT240, 1984).
It encodes images as terminal escape sequences, allowing **actual raster images** inline in
terminal output.

A sixel is a column of 6 vertical pixels, encoded as a single ASCII character (value 63-126).
Each character represents which of the 6 pixels are "on" for a given color.

### Escape Sequence Format

```
\x1bPq               Start sixel data (DCS = Device Control String)
#0;2;R;G;B           Define color 0 as RGB (each value 0-100, not 0-255!)
#0                   Select color 0
<sixel data>         Characters 0x3F-0x7E (each = 6 vertical pixels)
!<n><char>           Repeat <char> n times (run-length encoding)
$                    Carriage return (start of line, same 6-pixel row)
-                    Next row of 6 pixels
\x1b\               End sixel data (ST = String Terminator)
```

**Pixel encoding:** Each character minus 63 gives a 6-bit mask. Bit 0 = top pixel,
bit 5 = bottom pixel.

```
Character '?' = 0x3F = 63 - 63 = 0b000000 (all off)
Character '~' = 0x7E = 126 - 63 = 0b111111 (all on)
Character '@' = 0x40 = 64 - 63 = 0b000001 (only top pixel)
```

### Terminal Support (2024-2026)

| Terminal         | Sixel | Notes                                    |
|------------------|:-----:|------------------------------------------|
| XTerm            | Yes   | Original modern implementation           |
| mlterm           | Yes   | Long-standing support                    |
| foot             | Yes   | Wayland-native                           |
| WezTerm          | Yes   | Plus Kitty protocol                      |
| Konsole          | Yes   | KDE                                      |
| Windows Terminal | Yes   | Added in v1.22 (2024)                    |
| VS Code Terminal | Yes   | Recent addition                          |
| iTerm2           | Yes   | Also supports own inline image protocol  |
| Kitty            | **No**| Uses own graphics protocol instead       |
| Alacritty        | **No**| No plans (GPU-rendered text focus)       |
| macOS Terminal   | **No**|                                          |

### Key Tools

- **libsixel** -- Reference C implementation for encoding/decoding sixel data
- **lsix** -- `ls` for images, shows thumbnails as sixels
- **timg** -- Terminal image/video viewer, multi-protocol
- **img2sixel** -- Command-line image to sixel converter (part of libsixel)
- **rasterm** -- Go library supporting iTerm/Kitty/Sixel protocols
- **ratatui-image** -- Rust widget with sixel support

### Sixel vs. Other Protocols

| Feature           | Sixel           | Kitty Graphics  | iTerm2 Inline   |
|-------------------|-----------------|-----------------|-----------------|
| Color depth       | Palette (256)   | Full RGBA       | Full RGBA       |
| Bandwidth         | High            | Low (compressed)| Medium (base64) |
| Animation         | Manual          | Supported       | No              |
| Terminal support  | Wide (2024+)    | Kitty + few     | iTerm2 + few    |
| Scrolls with text | Yes             | Yes             | Yes             |
| Max resolution    | Terminal-limited| Arbitrary       | Arbitrary       |

**Practical note:** Most sixel-capable terminals also support the iTerm2 inline image
protocol, which uses fewer bytes, supports full color (not palette), and is simpler to
implement. Consider iTerm2 protocol as your second-choice after Kitty.

---

## 9. Kitty Graphics Protocol

### Overview

The Kitty graphics protocol allows rendering arbitrary pixel (raster) graphics at precise
positions within the terminal. More capable than Sixel, with animation support, z-layering,
and compression, but narrower terminal support.

### Design Goals

1. Terminal emulators don't need to understand image formats (clients decode)
2. Graphics can be drawn at individual pixel positions within cells
3. Graphics integrate with text via alpha blending
4. Graphics scroll with text automatically
5. Optimizations when client and terminal are on the same machine (shared memory)

### Escape Sequence Format

```
\x1b_G<key>=<value>,<key>=<value>;<base64 payload>\x1b\\
```

**Transmission keys:**
```
a=T          Action: T=transmit, t=transmit+display, q=query, d=delete
f=32         Format: 24=RGB, 32=RGBA, 100=PNG
t=d          Transmission: d=direct, f=file, t=temp file, s=shared memory
s=<width>    Image width in pixels
v=<height>   Image height in pixels
o=z          Compression: z=zlib deflate (RFC 1950)
i=<id>       Image ID (for referencing later)
m=0|1        More data coming (1) or last chunk (0)
```

**Display keys:**
```
x,y          Pixel offset within cell for display origin
w,h          Source rectangle width/height (crop)
c,r          Display columns/rows in terminal cells
z=<n>        Z-index (negative = behind text)
X,Y          Cell offset from cursor for placement
```

### Examples

```
# Display a small RGBA image directly (base64 encoded pixel data)
\x1b_Ga=T,f=32,s=100,v=100;<base64 RGBA data>\x1b\\

# Display a PNG file (path is base64 encoded)
\x1b_Ga=T,f=100,t=f;<base64("/path/to/image.png")>\x1b\\

# Chunked transfer for large images
\x1b_Ga=T,f=100,m=1;<first chunk base64>\x1b\\    # m=1 = more chunks coming
\x1b_Gm=1;<middle chunk base64>\x1b\\
\x1b_Gm=0;<final chunk base64>\x1b\\               # m=0 = last chunk

# Image behind text (negative z-index)
\x1b_Ga=T,f=32,z=-1,s=100,v=100;<data>\x1b\\

# Delete image by ID
\x1b_Ga=d,d=i,i=42\x1b\\
```

### Animation Support

```
# Create image with ID
\x1b_Ga=T,f=32,i=1,s=100,v=100;<frame1 data>\x1b\\

# Add animation frames
\x1b_Ga=f,i=1,z=77;<frame2 data>\x1b\\

# Control animation playback
\x1b_Ga=a,i=1,s=3,v=1\x1b\\    # start, 3 loops
```

### Terminal Support

| Terminal    | Support | Notes                          |
|-------------|:-------:|--------------------------------|
| Kitty       | Full    | Reference implementation       |
| WezTerm     | Yes     | Broad support                  |
| Ghostty     | Yes     | Since 1.0 (Dec 2024)          |
| Konsole     | Partial | Basic display                  |
| Others      | No      | Use Sixel or half-blocks       |

### Storage Quotas

To prevent DoS, terminals limit image storage. Kitty's default is **320MB** per buffer.
Applications should manage image lifecycles:
- Assign IDs to images for later deletion
- Delete images when they scroll off-screen
- Monitor response codes for storage limit errors

---

## 10. Cool Tricks

### Detecting Terminal Color Capability

```rust
enum ColorSupport {
    None,        // NO_COLOR set, or dumb terminal
    Basic16,     // Standard ANSI 16 colors
    Color256,    // xterm-256color
    TrueColor,   // 24-bit RGB
}

fn detect_color_support() -> ColorSupport {
    // 1. Respect NO_COLOR (https://no-color.org/)
    if std::env::var("NO_COLOR").is_ok() {
        return ColorSupport::None;
    }

    // 2. Check FORCE_COLOR
    match std::env::var("FORCE_COLOR").as_deref() {
        Ok("3") => return ColorSupport::TrueColor,
        Ok("2") => return ColorSupport::Color256,
        Ok("1") => return ColorSupport::Basic16,
        Ok("0") => return ColorSupport::None,
        _ => {}
    }

    // 3. Check COLORTERM for truecolor
    if let Ok(ct) = std::env::var("COLORTERM") {
        if ct == "truecolor" || ct == "24bit" {
            return ColorSupport::TrueColor;
        }
    }

    // 4. Check TERM
    if let Ok(term) = std::env::var("TERM") {
        if term == "dumb" {
            return ColorSupport::None;
        }
        if term.contains("256color") || term.contains("direct") {
            return ColorSupport::Color256;  // likely truecolor too
        }
    }

    // 5. Check known CI environments (most support truecolor)
    if std::env::var("CI").is_ok()
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("GITLAB_CI").is_ok()
    {
        return ColorSupport::TrueColor;
    }

    // 6. Check if stdout is a TTY
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return ColorSupport::None;
    }

    ColorSupport::Basic16  // safe default for unknown TTYs
}
```

### Flicker-Free Animation Without Synchronized Output

When DEC mode 2026 isn't available, write the **complete frame** to a string buffer first,
then output it in a single `write()` call:

```rust
use std::io::Write;

fn render_frame_buffered(lines: &[String]) {
    let mut buf = String::new();
    buf.push_str("\x1b[s");          // save cursor
    buf.push_str("\x1b[?25l");       // hide cursor

    for line in lines {
        buf.push_str("\x1b[2K");     // clear line
        buf.push_str(line);
        buf.push('\n');
    }

    buf.push_str("\x1b[u");          // restore cursor to start
    buf.push_str("\x1b[?25h");       // show cursor

    // Single write() call = minimal flicker
    std::io::stdout().write_all(buf.as_bytes()).unwrap();
    std::io::stdout().flush().unwrap();
}
```

**Key insight:** The kernel's write buffer will typically deliver the entire string to the
terminal in a single read(), which means the terminal renders it as one atomic update.
Not as reliable as synchronized output, but works everywhere.

### HSL Hue Rotation for Smooth Rainbows

RGB interpolation produces dull midpoints. HSL hue rotation keeps saturation and lightness
constant while sweeping through the spectrum:

```rust
/// Convert HSL to RGB. h in [0,360), s and l in [0,1].
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r1, g1, b1) = match h as u32 {
        0..=59    => (c, x, 0.0),
        60..=119  => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _         => (c, 0.0, x),
    };

    (
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

/// Rainbow text: sweep hue from 0 to 360 across the string
fn rainbow(text: &str, phase: f32) -> String {
    let len = text.chars().count() as f32;
    text.chars()
        .enumerate()
        .map(|(i, c)| {
            let hue = (phase + (i as f32 / len) * 360.0) % 360.0;
            let (r, g, b) = hsl_to_rgb(hue, 0.8, 0.6);
            format!("\x1b[38;2;{r};{g};{b}m{c}")
        })
        .collect::<String>()
        + "\x1b[0m"
}

// Animated rainbow: increment phase each frame
// for frame in 0.. {
//     print!("\r{}", rainbow("Hello, rainbow world!", frame as f32 * 5.0));
//     flush(); sleep(50ms);
// }
```

### The lolcat Sine Wave Trick

Three sine waves offset by 120 degrees produce a smooth full-spectrum rainbow without
needing HSL conversion:

```rust
fn lolcat_color(freq: f32, i: f32) -> (u8, u8, u8) {
    let r = ((freq * i + 0.0).sin() * 127.0 + 128.0) as u8;
    let g = ((freq * i + 2.094).sin() * 127.0 + 128.0) as u8;  // 2*PI/3
    let b = ((freq * i + 4.189).sin() * 127.0 + 128.0) as u8;  // 4*PI/3
    (r, g, b)
}
```

This is computationally cheaper than HSL conversion and produces a pleasant rainbow.
Vary `freq` to control how quickly colors change (0.1 = gradual, 0.3 = rapid).

### Animated Gradient Spinner (Combining Techniques)

```rust
use std::io::Write;
use std::time::Instant;

const SPINNER_CHARS: &[char] = &['·', '✢', '✳', '∗', '✻', '✽'];

fn animated_gradient_spinner(message: &str) {
    let start = Instant::now();

    loop {
        let elapsed = start.elapsed().as_millis() as f32;
        let frame = ((elapsed / 100.0) as usize) % SPINNER_CHARS.len();
        let spinner = SPINNER_CHARS[frame];

        // Color the spinner with hue rotation
        let hue = (elapsed / 10.0) % 360.0;
        let (r, g, b) = hsl_to_rgb(hue, 0.9, 0.65);

        // Gradient the message text
        let colored_msg = rainbow(message, elapsed / 20.0);

        print!("\r\x1b[2K\x1b[38;2;{r};{g};{b}m{spinner}\x1b[0m {colored_msg}");
        std::io::stdout().flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
```

### Useful Environment Variables

| Variable         | Values               | Purpose                              |
|------------------|----------------------|--------------------------------------|
| `NO_COLOR`       | (any value)          | Disable all color output             |
| `FORCE_COLOR`    | `0`,`1`,`2`,`3`     | Force color level                    |
| `COLORTERM`      | `truecolor`/`24bit`  | Advertise truecolor support          |
| `TERM`           | `xterm-256color`     | Terminal capability identifier       |
| `TERM_PROGRAM`   | `iTerm.app` etc      | Which terminal emulator              |
| `COLUMNS`/`LINES`| numbers              | Terminal dimensions                  |
| `CLICOLOR`       | `0` or `1`           | BSD convention for color support     |
| `CLICOLOR_FORCE` | `1`                  | Force colors even if not TTY         |

### The GitHub Copilot CLI Banner -- Lessons Learned

The engineering behind GitHub Copilot CLI's animated ASCII banner (June 2025) is instructive.
Key takeaways from their blog post:

- Over **6,000 lines of TypeScript** for a 3-second animation
- Most code handles **terminal inconsistencies**, not visuals
- Built a custom ANSI preview tool because existing tools don't match real terminal behavior
- Terminals remap colors differently and handle cursor updates inconsistently
- Accessibility: `--screen-reader` mode skips decorative animation entirely
- Open-sourced the tooling at **ascii-motion.app**
- Key quote: "the CLI world is still fragmented -- terminals behave differently, have few
  shared standards, and offer almost no consistent accessibility guidelines"

### Terminal Width and Wrapping

Always check terminal width before rendering wide content:

```rust
fn terminal_width() -> u16 {
    crossterm::terminal::size()
        .map(|(w, _)| w)
        .unwrap_or(80)
}

// Truncate lines to avoid wrapping artifacts
fn safe_line(text: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if text.width() <= max_width {
        text.to_string()
    } else {
        // Truncate and add ellipsis
        let mut result = String::new();
        let mut width = 0;
        for c in text.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            if width + cw > max_width - 1 {
                result.push('…');
                break;
            }
            result.push(c);
            width += cw;
        }
        result
    }
}
```

### Hyperlinks in Terminal

Many modern terminals support clickable hyperlinks via OSC 8:

```
\x1b]8;;<url>\x1b\\<text>\x1b]8;;\x1b\\
```

Example:
```rust
fn hyperlink(url: &str, text: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}
```

Supported by: iTerm2, GNOME Terminal, Windows Terminal, WezTerm, Kitty, Ghostty.

---

## Sources

### 24-bit Color and ANSI
- [ANSI escape code - Wikipedia](https://en.wikipedia.org/wiki/ANSI_escape_code)
- [I Just Wanted Emacs to Look Nice -- Using 24-Bit Color in Terminals](https://chadaustin.me/2024/01/truecolor-terminal-emacs/)
- [ANSI Escape Codes Gist](https://gist.github.com/fnky/458719343aabd01cfb17a3a4f7296797)
- [So you want to render colors in your terminal](https://marvinh.dev/blog/terminal-colors/)
- [True Colour support in various terminals](https://gist.github.com/splinedrive/0691befec6fc0bb21d9cc943f94b1282)
- [COLORTERM env var -- Windows Terminal discussion](https://github.com/microsoft/terminal/issues/13687)

### Gradient Text
- [gradient-string on GitHub](https://github.com/bokub/gradient-string)
- [lolcat -- Display Text in Rainbow Colors](https://itsfoss.gitlab.io/post/lolcat--display-text-in-rainbow-colors-in-linux-terminal/)
- [The Secrets of Colour Interpolation](https://www.alanzucconi.com/2016/01/06/colour-interpolation/)
- [Color Representation and Gradients](https://evoniuk.github.io/Color-Representation-and-Gradients/)

### Spinners
- [sindresorhus/cli-spinners on GitHub](https://github.com/sindresorhus/cli-spinners)
- [sindresorhus/ora on GitHub](https://github.com/sindresorhus/ora)
- [Spinners.txt reference](https://antofthy.gitlab.io/info/ascii/Spinners.txt)
- [Reverse Engineering Claude's ASCII Spinner Animation](https://medium.com/@kyletmartinez/reverse-engineering-claudes-ascii-spinner-animation-eec2804626e0)
- [Claude Code's thinking animation](https://blog.alexbeals.com/posts/claude-codes-thinking-animation)
- [Adding Colors and Spinners to Your CLI](https://medium.com/@sohail_saifi/adding-colors-and-spinners-to-your-cli-making-terminal-output-actually-pleasant-1f4110223b34)

### Block/Pixel Art and Unicode
- [drawille -- Pixel graphics with unicode braille](https://github.com/asciimoo/drawille)
- [TerminalImageViewer](https://github.com/stefanhaustein/TerminalImageViewer)
- [Drawing with Unicode block characters](https://www.johndcook.com/blog/2019/10/21/box-drawing-unicode/)
- [ASCII art, but in Unicode (Dernocua)](https://dernocua.github.io/notes/unicode-graphics.html)
- [Braille Patterns - Wikipedia](https://en.wikipedia.org/wiki/Braille_Patterns)
- [Alchemical Symbols Unicode block](https://www.unicode.org/charts/nameslist/n_1F700.html)

### Animation and Synchronized Output
- [Terminal Spec: Synchronized Output](https://gist.github.com/christianparpart/d8a62cc1ab659194337d73e399004036)
- [WezTerm Escape Sequences](https://wezterm.org/escape-sequences.html)
- [Windows Terminal DECSET 2026 PR](https://github.com/microsoft/terminal/pull/18826)
- [Alternate Screen Buffer - Terminal Guide](https://terminalguide.namepad.de/mode/p47/)

### Rust Crates
- [ratatui](https://github.com/ratatui/ratatui)
- [crossterm](https://github.com/crossterm-rs/crossterm)
- [indicatif on crates.io](https://crates.io/crates/indicatif)
- [owo-colors on GitHub](https://github.com/owo-colors/owo-colors)
- [ratatui-image on crates.io](https://crates.io/crates/ratatui-image)
- [Rain's Rust CLI recommendations -- managing colors](https://rust-cli-recommendations.sunshowers.io/managing-colors-in-rust.html)

### Graphics Protocols
- [Kitty graphics protocol specification](https://sw.kovidgoyal.net/kitty/graphics-protocol/)
- [libsixel](https://saitoha.github.io/libsixel/)
- [Sixel - Wikipedia](https://en.wikipedia.org/wiki/Sixel)
- [rasterm -- multi-protocol image rendering](https://github.com/BourgeoisBear/rasterm)
- [lsix -- ls for images](https://github.com/hackerb9/lsix)

### CLI Design and Engineering
- [From pixels to characters: GitHub Copilot CLI's animated ASCII banner](https://github.blog/engineering/from-pixels-to-characters-the-engineering-behind-github-copilot-clis-animated-ascii-banner/)
- [Turborepo Terminal UI RFC](https://github.com/vercel/turborepo/discussions/7802)
- [Vercel Geist Design System -- Spinner](https://vercel.com/geist/spinner)
- [Command line spinners: the tale of modern typewriters](https://odino.org/command-line-spinners-the-amazing-tale-of-modern-typewriters-and-digital-movies/)
