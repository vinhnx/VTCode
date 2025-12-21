# Ghostty VT Overview (for VTCode)

## Why this exists

A quick pointer to Ghostty's VT docs so we keep our terminal support aligned with their guidance.

## Primary sources

-   Overview: https://ghostty.org/docs/vt
-   Concepts (control sequence families): https://ghostty.org/docs/vt/concepts/sequences
-   Reference (supported sequences list): https://ghostty.org/docs/vt/reference
-   External protocols (e.g., Kitty graphics via APC): https://ghostty.org/docs/vt/external

## Key notes for implementation

-   Control sequences are the terminal API surface. Use CSI for integer params, OSC for string payloads, DCS/APC for richer payloads, and treat SOS/PM as ignorable.
-   OSC termination: accept both ST (ESC \\) and BEL; echo the terminator we receive when replying.
-   CSI params: support both `;` and `:` separators; finals are bytes 0x40–0x7E.
-   Palette and dynamic colors: OSC 4/5 for palette (indices 0–255 plus special colors); OSC 10–12 for dynamic colors; OSC 104–119 for resets.
-   Hyperlinks/notifications/progress: OSC 8 hyperlinks; OSC 9 desktop notify; OSC 9;4 progress state.

## Status in vtcode

-   Parsers updated to strip/handle CSI colon params, OSC BEL/ST, and DCS/APC/SOS/PM terminated with ST.
-   Streaming stripper matches the same rules; tests cover colon CSI, OSC BEL, and SOS/PM ignore.
-   Remaining risk: Anthropic provider tests fail unrelated to VT; rerun VT tests once those fixtures are fixed.
