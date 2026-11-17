# Elicitation selection controls

`request_user_input` questions accept optional `multi_select: true`. Desktop displays standard checkboxes and
the terminal displays `[ ]` / `[x]`. Omit the flag for single-answer radio buttons.

Multi-select starts unchecked. Choose at least one option or enter a custom note; a note can accompany checked
options. Moving between questions preserves selections and notes. The response retains `selected` (original
option labels in displayed order) and optional `other` text.

## Quick reference

- Desktop: click a checkbox, or Tab to it and press Space to toggle it; use Next or Submit to commit.
- Terminal: arrows move focus, Space toggles, Enter continues, Tab opens notes, Escape cancels.
- Single-select: selecting another radio option replaces the previous answer.
