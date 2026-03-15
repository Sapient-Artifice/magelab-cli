# Terminal Animation Art — Techniques Reference

A reference for building beautiful CLI experiences. Covers color, animation, pixel art, and image rendering in modern terminals.

---

## 1. 24-Bit True Color ANSI

Most developer terminals (iTerm2, Kitty, Alacritty, WezTerm, GNOME Terminal 3.28+, Windows Terminal) support 16.7M colors.

```
Foreground:  \x1b[38;2;R;G;Bm
Background:  \x1b[48;2;R;G;Bm
Reset:       \x1b[0m
```

Example — purple text:
```
\x1b[38;2;139;92;246mHello\x1b[0m
```

**Detection:** Check `COLORTERM=truecolor` or `COLORTERM=24bit` env var. The `console` crate provides `colors_enabled()`. Always respect `NO_COLOR` env var.

**Fallback:** Terminals without true color map to nearest 256-color or 16-color ANSI equivalent.

---

## 2. Gradient Text

Assign each character a different RGB value along a curve. For a line of text "MageLab":

```
M → (139,92,246)   purple
a → (124,58,237)   deeper
g → (109,40,217)   violet
e → (99,102,241)   indigo
L → (79,70,229)    blue
a → (99,102,241)   indigo (return)
b → (139,92,246)   purple
```

Implementation: iterate chars, interpolate between gradient stops, wrap each char in `\x1b[38;2;R;G;Bm...\x1b[0m`.

**HSL interpolation** produces smoother hue rotation than RGB lerp. Convert start/end to HSL, interpolate H/S/L independently, convert back.

Tools using this: `lolcat`, `gradient-string`, `chalk`.

---

## 3. Animated Spinners — Survey

### Frame-based (what we use)
A sequence of strings displayed at fixed intervals (60-120ms). The last string in indicatif's `tick_strings` is the "finished" state.

### Notable designs

| Tool | Style | Distinctive feature |
|------|-------|-------------------|
| **Bun** | Custom braille | Brand magenta color |
| **Vercel/Turbo** | Dots (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`) | Clean, fast |
| **Claude Code** | Dots + thinking text | Updates message dynamically |
| **npm** | Classic bar `\|/-` | Retro simplicity |
| **Biome** | Arrow cycle | Minimal |
| **MageLab** | Alchemical symbols + 24-bit gradient | Color-shifting brand palette |

### Best practices (Evil Martians)
- Use spinners for quick sequential tasks (few seconds)
- Update message on meaningful events, not just time
- Green checkmark on success, red X on failure
- Clear spinner line on completion
- Respect `NO_COLOR` and `--no-color`

---

## 4. Unicode Art Symbol Categories

### Braille Patterns (U+2800–U+28FF)
```
⠁⠂⠃⠄⠅⠆⠇⠈⠉⠊⠋⠌⠍⠎⠏
⠐⠑⠒⠓⠔⠕⠖⠗⠘⠙⠚⠛⠜⠝⠞⠟
⠠⠡⠢⠣⠤⠥⠦⠧⠨⠩⠪⠫⠬⠭⠮⠯
⡀⡁⡂⡃⡄⡅⡆⡇⡈⡉⡊⡋⡌⡍⡎⡏
⣀⣁⣂⣃⣄⣅⣆⣇⣈⣉⣊⣋⣌⣍⣎⣏
⣰⣱⣲⣳⣴⣵⣶⣷⣸⣹⣺⣻⣼⣽⣾⣿
```
Each braille character is a 2x4 dot grid (8 bits). Used for pseudo-pixel rendering — each cell gives 8 pixels. Tools like `termimage` and `plotters` use this for graphing.

### Block Elements (U+2580–U+259F)
```
▀ ▁ ▂ ▃ ▄ ▅ ▆ ▇ █   (vertical fills)
▉ ▊ ▋ ▌ ▍ ▎ ▏        (horizontal fills)
░ ▒ ▓                  (shading)
▐ ▕                    (half blocks)
```
Half blocks (▀▄) with fg+bg coloring give 2 vertical pixels per cell. Combined with 24-bit color, this is the highest-resolution text-mode rendering.

### Box Drawing (U+2500–U+257F)
```
─ │ ┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼
═ ║ ╔ ╗ ╚ ╝ ╠ ╣ ╦ ╩ ╬
╭ ╮ ╯ ╰   (rounded corners)
```

### Geometric Shapes (U+25A0–U+25FF)
```
■ □ ▢ ▣ ▤ ▥ ▦ ▧ ▨ ▩
● ○ ◉ ◎ ◌ ◍
◆ ◇ ◈ ◊
▲ △ ▴ ▵ ▶ ▷ ▸ ▹ ► ▻
★ ☆ ✦ ✧ ✶ ✷ ✸ ✹
```

### Alchemical Symbols (U+1F700–U+1F77F)
```
🜁 🜂 🜃 🜄 🜅 🜆 🜇 🜈 🜉 🜊 🜋 🜌 🜍
🜎 🜏 🜐 🜑 🜒 🜓 🜔 🜕 🜖 🜗 🜘 🜙 🜚 🜛
```
Air, fire, earth, water, and dozens of alchemical process symbols. Unique, rarely seen in CLIs. We use 🜁🜂🜃🜄.

### Other useful symbols
```
♥ ♦ ♣ ♠               (card suits)
⚡ ⚙ ⚗ ⚘              (misc technical)
✓ ✗ ✔ ✘               (check/cross)
→ ← ↑ ↓ ⇒ ⇐          (arrows)
```

---

## 5. Cursor & Screen Control

```
\x1b[nA         Move cursor up n lines
\x1b[nB         Move cursor down n lines
\x1b[nC         Move cursor right n columns
\x1b[nD         Move cursor left n columns
\x1b[s          Save cursor position
\x1b[u          Restore cursor position
\x1b[2K         Clear entire line
\x1b[0J         Clear from cursor to end of screen
\x1b[2J         Clear entire screen
\x1b[?25l       Hide cursor
\x1b[?25h       Show cursor
\x1b[?1049h     Enter alternate screen buffer
\x1b[?1049l     Leave alternate screen buffer
```

### Flicker-free animation
1. Hide cursor (`\x1b[?25l`)
2. Save position (`\x1b[s`)
3. Write full frame
4. Restore position (`\x1b[u`)
5. Repeat
6. Show cursor on exit (`\x1b[?25h`)

### Synchronized output (DEC private mode 2026)
```
\x1b[?2026h     Begin synchronized update
... write frame ...
\x1b[?2026l     End synchronized update (terminal renders atomically)
```
Supported by: Kitty, WezTerm, Contour, foot, iTerm2 (3.5+). Prevents tearing on complex redraws.

---

## 6. Sixel Graphics

Protocol for rendering raster images inline in terminal. Each sixel is a column of 6 vertical pixels.

```
\x1bPq          Enter sixel mode
#0;2;100;0;0    Define color 0 as red (HLS)
#0~~@@vv        Draw pixels using color 0
\x1b\           Exit sixel mode
```

**Supported by:** xterm, mlterm, foot, WezTerm, Contour. **Not** iTerm2, Kitty, Alacritty.

Use the `sixel` Rust crate or shell out to `img2sixel` (from libsixel).

---

## 7. Kitty Graphics Protocol

More capable than sixel. Sends PNG/RGB data directly:

```
\x1b_Gf=100,a=T,t=d;BASE64_PNG_DATA\x1b\
```

Supports: PNG, RGB, RGBA, animation frames, z-layering, Unicode placeholders.

**Supported by:** Kitty, WezTerm (partial), Ghostty.

---

## 8. Rust Crates

| Crate | Purpose | True color |
|-------|---------|-----------|
| **ratatui** | Full TUI framework (widgets, layout) | Yes |
| **crossterm** | Low-level terminal control (cursor, color, events) | Yes |
| **indicatif** | Progress bars and spinners | Via embedded ANSI |
| **console** | Terminal utilities (used by indicatif) | `Color::TrueColor(r,g,b)` |
| **owo-colors** | Zero-alloc text coloring | `.truecolor(r,g,b)` |
| **colored** | Popular text coloring | `.truecolor(r,g,b)` |
| **ansi_rgb** | Lean RGB coloring | `.fg(RGB8::new(r,g,b))` |
| **nu-ansi-term** | Nushell's ansi_term fork | `Color::Rgb(r,g,b)` |
| **viuer** | Display images in terminal (sixel, kitty, block) | N/A |
| **termimage** | Image to braille/block art | N/A |

---

## 9. How MageLab CLI Does It

We embed 24-bit ANSI directly in indicatif `tick_strings`:

```rust
.tick_strings(&[
    "\x1b[38;2;139;92;246m🜁\x1b[0m",  // #8b5cf6 mage-500
    "\x1b[38;2;124;58;237m🜂\x1b[0m",  // #7c3aed mage-600
    "\x1b[38;2;109;40;217m✦\x1b[0m",   // #6d28d9 mage-700
    "\x1b[38;2;99;102;241m◈\x1b[0m",   // #6366f1 indigo-500
    "\x1b[38;2;79;70;229m◇\x1b[0m",    // #4f46e5 indigo-600
    // ... cycles back through purple
])
```

Key choices:
- **Alchemical symbols** — unique brand identity, no other CLI uses them
- **24-bit gradient** — smooth color shift matching MageLab's purple palette
- **Uniform glyph width** — all symbols are 1 terminal cell, no emoji scaling issues
- **80ms tick rate** — smooth but not CPU-heavy

---

## Sources

- [ANSI Escape Codes (fnky)](https://gist.github.com/fnky/458719343aabd01cfb17a3a4f7296797)
- [indicatif docs](https://docs.rs/indicatif/latest/indicatif/)
- [console crate Color enum](https://docs.rs/console/latest/console/enum.Color.html)
- [sindresorhus/cli-spinners](https://github.com/sindresorhus/cli-spinners)
- [Rain's Rust CLI color recommendations](https://rust-cli-recommendations.sunshowers.io/managing-colors-in-rust.html)
- [CLI UX best practices (Evil Martians)](https://evilmartians.com/chronicles/cli-ux-best-practices-3-patterns-for-improving-progress-displays)
- [Synchronized output spec](https://gitlab.freedesktop.org/terminal-wg/specifications/-/merge_requests/2)
- [Kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/)
- [Sixel graphics](https://en.wikipedia.org/wiki/Sixel)
