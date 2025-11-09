# Quick Reference: Anstyle Crates

Fast lookup for `anstyle-git` and `anstyle-ls` syntax and usage.

## anstyle-git Syntax

Parse Git color configuration strings.

### Supported Keywords
```
bold, dim, italic, underline, reverse, strikethrough
```

### Supported Colors (Named)
```
black, red, green, yellow, blue, magenta, cyan, white
```

### Supported Colors (Hex)
```
#RRGGBB (e.g., #0000ee for blue)
```

### Syntax Rules
- Whitespace-separated words
- First color = foreground, second color = background
- Effects can appear anywhere
- Effect keywords: `bold`, `dim`, `italic`, `underline`, `reverse`, `strikethrough`

### Examples

```rust
use anstyle_git::parse;

// Named colors
parse("bold red").unwrap()              // bold red text
parse("red blue").unwrap()              // red text on blue bg
parse("green").unwrap()                 // green text

// Hex colors
parse("#0000ee").unwrap()               // RGB blue text
parse("#ff0000 #00ff00").unwrap()       // RGB red text on RGB green bg

// Combined
parse("bold #0000ee ul").unwrap()       // bold, blue, underlined
parse("italic yellow black").unwrap()   // italic yellow on black

// Invalid (these will error)
parse("unknown-color").unwrap()         // ✗ unknown-color not recognized
parse("#gg0000").unwrap()               // ✗ invalid hex
```

---

## anstyle-ls Syntax

Parse LS_COLORS environment variable format.

### ANSI Code Sequences (Semicolon-Separated)

```
01 = bold
02 = dim (faint)
03 = italic
04 = underline
05 = blink
07 = reverse
09 = strikethrough

30-37 = foreground colors (black, red, green, yellow, blue, magenta, cyan, white)
90-97 = bright foreground colors
40-47 = background colors (same order)
100-107 = bright background colors
```

### File Type Keys (from LS_COLORS env var)

```
di = directory
ln = symlink
so = socket
pi = pipe
ex = executable
bd = block device
cd = character device
su = setuid file
sg = setgid file
tw = sticky + world-writable dir
ow = world-writable dir (not sticky)
st = sticky dir
*.EXT = file extension (e.g., *.tar, *.jpg)
```

### LS_COLORS Format

```
KEY=CODE:KEY=CODE:...
```

### Examples

```rust
use anstyle_ls::parse;

// Basic colors
parse("34").unwrap()                    // blue foreground (34)
parse("01;34").unwrap()                 // bold blue (01=bold, 34=blue)
parse("01;31").unwrap()                 // bold red
parse("03;36").unwrap()                 // italic cyan

// Background
parse("30;47").unwrap()                 // black text on white bg

// Full LS_COLORS example
let colors = "di=01;34:ln=01;36:ex=01;32:*.tar=01;31:*.zip=01;31";
// Parse individual entries
parse("01;34").unwrap()                 // from di=...

// Invalid (will error)
parse("99").unwrap()                    // ✗ code 99 not valid
parse("invalid").unwrap()               // ✗ not numeric
```

---

## Git Config Color Syntax

From `.git/config`:

```ini
[color "diff"]
    new = green
    old = red
    context = default

[color "diff"]
    meta = bold yellow
    
[color "branch"]
    current = green bold
    local = green
    remote = red
```

Supported style values:
- `bold`, `dim`, `italic`, `underline`, `reverse`, `strikethrough` (modifiers)
- `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white` (colors)
- `bright` prefix for bright colors
- `default` for terminal default color

---

## Vtcode Integration Points

### 1. Theme Parser (Phase 1)

```rust
use vtcode_core::ui::tui::ThemeConfigParser;

// Parse Git style
let style = ThemeConfigParser::parse_git_style("bold red")?;

// Parse LS_COLORS style
let style = ThemeConfigParser::parse_ls_colors("01;34")?;

// Flexible parsing (try both)
let style = ThemeConfigParser::parse_flexible("bold red")?;
```

### 2. InlineTextStyle (Phase 1 - After Update)

```rust
use anstyle::Effects;

let style = InlineTextStyle {
    color: Some(color),
    bg_color: Some(bg),
    effects: Effects::BOLD | Effects::UNDERLINE,
};

// Or with builder
let style = InlineTextStyle::default()
    .bold()
    .underline()
    .with_color(color);
```

### 3. File Colorizer (Phase 3 Future)

```rust
use vtcode_core::ui::tui::FileColorizer;

let colorizer = FileColorizer::new();  // Reads LS_COLORS
let style = colorizer.style_for_file(&path)?;
```

---

## Cheat Sheet: Common Patterns

### Highlighting Error Messages

```rust
// Git style
"bold red"              // Bold red foreground

// LS_COLORS style
"01;31"                 // Bold (01) + red (31)

// In Vtcode
let error_style = ThemeConfigParser::parse_git_style("bold red")?;
```

### Highlighting Success Messages

```rust
// Git style
"green"                 // Green foreground

// LS_COLORS style
"32"                    // Green foreground

// In Vtcode
let success_style = ThemeConfigParser::parse_git_style("green")?;
```

### Dim/Secondary Text

```rust
// Git style
"dim white"             // Dim white

// LS_COLORS style
"02;37"                 // Dim (02) + white (37)

// In Vtcode
let dim_style = ThemeConfigParser::parse_git_style("dim white")?;
```

### Directory Listing Colors

```rust
// From environment LS_COLORS
let dir_style = ThemeConfigParser::parse_ls_colors("01;34")?;  // bold blue

let sym_style = ThemeConfigParser::parse_ls_colors("01;36")?;  // bold cyan

let exec_style = ThemeConfigParser::parse_ls_colors("01;32")?; // bold green
```

### Git Diff Colors

```rust
// From .git/config [color "diff"]
let added_style = ThemeConfigParser::parse_git_style("green")?;

let removed_style = ThemeConfigParser::parse_git_style("red")?;

let context_style = ThemeConfigParser::parse_git_style("default")?;
```

---

## Debugging Tips

### Check LS_COLORS Value

```bash
echo $LS_COLORS
# Output: di=01;34:ln=01;36:so=01;32:pi=40;33:...
```

### Check Git Colors

```bash
git config --list | grep color
# color.ui=auto
# color.diff.new=green
# color.diff.old=red
```

### Test Parsing in Rust

```rust
// Quick test in a binary or test
let test_input = "bold red";
match anstyle_git::parse(test_input) {
    Ok(style) => println!("Parsed successfully: {:?}", style),
    Err(e) => println!("Parse error: {:?}", e),
}
```

---

## Performance Notes

- Both parsers are **very fast** (microseconds per parse)
- Suitable for parsing on every render (cache optional)
- No allocations beyond result Style object
- Git config parsing should be **cached** per session
- LS_COLORS can be cached from environment var

---

## Compatibility Notes

### Platforms
- **Linux/macOS**: Full LS_COLORS support
- **Windows**: LS_COLORS support optional (may not be set)
- **Git**: Color config available on all platforms

### Terminal Support
- **Bold**: Universal
- **Italic**: Varies (TTY-dependent)
- **Underline**: Most terminals
- **Strikethrough**: Not widely supported in ratatui
- **Reverse**: Most terminals
- **Dim**: Varies (may show as bright on some terminals)

### Graceful Degradation
If a terminal doesn't support an effect, ratatui will safely ignore it (no error).

---

## Further Reading

- [anstyle-git crate](https://docs.rs/anstyle-git/latest/anstyle_git/)
- [anstyle-ls crate](https://docs.rs/anstyle-ls/latest/anstyle_ls/)
- [anstyle crate](https://docs.rs/anstyle/latest/anstyle/)
- [Git Color Config](https://git-scm.com/docs/git-config#color.ui)
- [DIRCOLORS man page](https://linux.die.net/man/5/dir_colors)
