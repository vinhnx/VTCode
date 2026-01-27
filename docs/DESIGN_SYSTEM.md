---
title: VT Code Design System
audience: UI and CLI rendering
scope: Presentation-only (not agent behavior)
---

# VT Code Design System

This document defines the VT Code design system for terminal UI and CLI output.
It applies to user-facing rendering only and does not define agent behavior or
prompting rules.

## Color Roles

- Headers: bold text (keep Markdown `#` markers).
- Primary text: default terminal foreground (no forced color).
- Secondary text: dim.
- Tips, selections, and status indicators: cyan.
- Success and additions: green.
- Errors, failures, and deletions: red.
- Codex references: magenta.

## Constraints

- Avoid blue and yellow foreground colors.
- Avoid black and white foreground colors; use `reset` or default foreground.
- Avoid custom colors unless explicitly approved (shimmer.rs is the exception).

## Usage Notes

- Apply styles consistently across CLI, TUI, and rendered markdown.
- Prefer semantic roles over hard-coded color names.
