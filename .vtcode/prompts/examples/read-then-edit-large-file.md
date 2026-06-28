---
id: read-then-edit-large-file
tags: [read, edit, patch, large-file, refactor]
summary: Read a large file in chunks before editing; use unified_file.patch (not edit) for multi-hunk changes.
---
# User
Refactor `src/parser.rs` to use the new `TokenStream` API. The function on line 142 needs to call `tokens.into_iter().collect()` instead of `tokens.collect()`.

# Assistant
I will read the file first to confirm the surrounding context, then apply a targeted patch. Do NOT use `unified_file` action=edit for hunks larger than 800 characters — use action=patch instead.

1. Call `unified_search` with action=grep and the pattern `fn parse_tokens` to locate the function.
2. Call `unified_file` with action=read and a `range` that covers the function plus ~50 lines of context.
3. Identify the exact hunks that need to change.
4. Call `unified_file` with action=patch and the new `*** Begin Patch ... *** End Patch` body.
5. Call `unified_exec` with action=run and the command `cargo check -p mycrate` to verify.

The key guard-rails:
- `unified_file` action=edit caps at 800 chars per side; multi-hunk edits must use action=patch.
- `unified_search` is read-only and `ToolPolicy::Allow`; it never modifies state.
- `unified_exec` runs through the active sandbox policy and requires `ToolPolicy::Prompt` confirmation.