# Widget Builder Pattern Examples

## Current vs Improved API

### HeaderWidget

**Current (struct initialization):**
```rust
HeaderWidget {
    session: self.session,
    lines: header_lines,
}.render(header_area, buf);
```

**Improved (builder lite):**
```rust
HeaderWidget::new(self.session)
    .lines(header_lines)
    .style(header_style)
    .render(header_area, buf);
```

### TranscriptWidget

**Current:**
```rust
TranscriptWidget {
    session: self.session,
}.render(transcript_area, buf);
```

**Improved:**
```rust
TranscriptWidget::new(self.session)
    .show_scrollbar(true)
    .highlight_search("query")
    .render(transcript_area, buf);
```

### FilePaletteWidget

**Current:**
```rust
FilePaletteWidget {
    session: self.session,
    palette,
    viewport,
}.render(viewport, buf);
```

**Improved:**
```rust
FilePaletteWidget::new(self.session, palette)
    .viewport(viewport)
    .show_loading(true)
    .highlight_style(accent_style)
    .render(viewport, buf);
```

## Implementation Template

```rust
pub struct MyWidget<'a> {
    // Required fields
    session: &'a Session,

    // Optional fields (with defaults)
    show_border: bool,
    title: Option<String>,
    style: Style,
}

impl<'a> MyWidget<'a> {
    /// Create a new widget with required parameters
    pub fn new(session: &'a Session) -> Self {
        Self {
            session,
            show_border: true,
            title: None,
            style: Style::default(),
        }
    }

    /// Builder method - consumes self and returns modified self
    #[must_use]
    pub fn show_border(mut self, show: bool) -> Self {
        self.show_border = show;
        self
    }

    #[must_use]
    pub fn title<T: Into<String>>(mut self, title: T) -> Self {
        self.title = Some(title.into());
        self
    }

    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl<'a> Widget for MyWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Use self.show_border, self.title, self.style
    }
}
```

## Key Points

1. **#[must_use]**: Prevents accidentally calling builder methods without using the result
2. **Consume self**: Each builder method takes ownership and returns the modified struct
3. **Sensible defaults**: Required params in `new()`, optional params as builder methods
4. **Type conversions**: Use `Into<String>` or `Into<Style>` for flexibility
