---
id: read-then-edit-large-file
tags: [read, edit, patch, large-file, refactor]
summary: Read the relevant file section before editing, then apply a focused patch.
---
# User
Refactor `src/parser.rs` to use the new `TokenStream` API. The function on line 142 needs to call `tokens.into_iter().collect()` instead of `tokens.collect()`.

# Assistant
I will locate and read the function first, then apply a targeted patch.

1. Call `code_search` with `query` set to `parse_tokens`, `path` set to `src/parser.rs`, and `result_types` set to `["definition"]`.
2. Call `exec_command` with a targeted `sed` command that covers the function and nearby context.
3. Identify the exact hunks that need to change.
4. Call `apply_patch` with the new `*** Begin Patch ... *** End Patch` body.
5. Call `exec_command` with `cargo check -p mycrate` to verify.

The key guard-rails:
- `code_search` is read-only and available in the advanced profile.
- `apply_patch` keeps the edit focused and workspace-bound.
- `exec_command` runs through the active sandbox and permission policy.
