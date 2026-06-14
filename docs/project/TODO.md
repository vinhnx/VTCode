can we wrap the TUI container text for both edges of terminals? It looks weird when the text just abruptly ends at the edge without wrapping. Wrapping would make it look more polished and easier to read, especially for longer lines of text.

some components like quote, table are not wrapped properly and just get cut off at the edge of the terminal. Wrapping would ensure that all content is visible and the UI looks more polished. It would also improve readability, especially for users with smaller terminal windows.

--

https://ratatui.rs/highlights/v0301/

1. Text += Text for Future Text Assembly (Future Use)

The AddAssign impl for Text is available but the current codebase uses custom MarkdownLine/MarkdownSegment types rather than ratatui Text. This is a capability to keep in mind for future message rendering improvements.

2. UnicodeWidthStr on Text/Line/Span (Future Use)

Rtatui 0.30.0 adds .width() to Text, Line, and Span. The current codebase uses unicode_width::UnicodeWidthStr on tr values, not on ratatui types. New code working with Text/Line types can now use .width() directly.
