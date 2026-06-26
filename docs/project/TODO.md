# Plan: CLI "Did you mean?" Subcommand Suggestions

## Problem

When a user types `vtcode ch` (or any mistyped/ambiguous subcommand), the `workspace_path` positional arg's `value_parser` catches it first and produces:

```
error: invalid value 'ch' for '[WORKSPACE]': Workspace path does not exist: ch
```

instead of recognizing it as a subcommand and suggesting the correct one.

## Root Cause

The `workspace_path` positional arg (`args.rs:82-88`) has `global = true` and a custom `value_parser = parse_workspace_directory`. Clap parses this before resolving the subcommand. When the value doesn't exist as a directory, clap's validation error fires with no awareness of available subcommands or global flags.

## Approach

Add a post-parse error interceptor in `bootstrap_main()` that:

1. Detects clap parse errors about invalid workspace paths
2. Extracts the invalid value from the error message
3. Uses Jaro-Winkler string similarity (via `strsim`, already a dependency) against:
   - All registered subcommand names (from `Cli::command().get_subcommands()`)
   - Key global flags (`--continue`, `--resume`, `--full-auto`, `--fork-session`)
4. Appends a "Did you mean: ..." suggestion to the error output

## Implementation

### File: `src/main_helpers/bootstrap.rs`

1. **Add `collect_suggestion_candidates()` function** (~15 lines):
   - Call `Cli::command().get_subcommands()` to get all subcommand names at runtime
   - For each subcommand, collect its `get_name()` and all `get_visible_aliases()`
   - Add key global flag names: `"--continue"`, `"--resume"`, `"--full-auto"`, `"--fork-session"`, `"--agent"`
   - Return a `Vec<&str>` of all candidate names

2. **Add `suggest_similar_commands()` function** (~20 lines):
   - Takes the invalid value and the candidate list
   - Scores each candidate using `strsim::jaro_winkler`
   - Returns top 1-3 candidates above threshold (0.6)
   - Prepends `--` to flag suggestions for clarity

3. **Add `try_enhance_clap_error()` function** (~15 lines):
   - Parse the clap error text to extract the invalid value from the pattern: `invalid value '{value}' for '[WORKSPACE]'`
   - If matched, call `suggest_similar_commands()` and return the enhanced error string
   - Return `None` if no match or no good suggestions

4. **Modify `bootstrap_main()`** (~10 lines):
   - Change `build_augmented_cli_command().get_matches()` to use `.try_get_matches()`
   - On error, call `try_enhance_clap_error()`
   - If a suggestion exists, print the enhanced error to stderr and exit with code 1
   - Otherwise, re-raise the original clap error (preserving current behavior)

### File: `src/main_helpers/bootstrap.rs` (tests)

5. **Add unit tests**:
   - `suggest_similar_commands_exact_match`: "continue" → suggests "continue"
   - `suggest_similar_commands_fuzzy_subcommand`: "chekc" → suggests "check"
   - `suggest_similar_commands_fuzzy_flag`: "contnue" → suggests "--continue"
   - `suggest_similar_commands_no_match`: "xyzzy" → empty suggestions
   - `try_enhance_clap_error_extracts_value`: verifies extraction from error text

## Files Modified

| File | Change |
|---|---|
| `src/main_helpers/bootstrap.rs` | Add suggestion functions, modify `bootstrap_main()` error handling |

## Verification

1. `cargo check --locked` passes
2. `cargo test -p vtcode` passes (including new tests)
3. Manual test: `cargo run -- ch` → suggests "chat"
4. Manual test: `cargo run -- contnue` → suggests "--continue"
5. Manual test: `cargo run -- xyzzy` → original error, no suggestion
6. Manual test: `cargo run -- continue` → resumes session (existing behavior unchanged)
