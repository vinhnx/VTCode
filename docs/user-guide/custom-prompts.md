# Custom Prompts

VT Code supports reusable custom prompts inspired by the [Codex CLI guidance](https://github.com/openai/codex/blob/main/docs/prompts.md).
Custom prompts let you codify repeatable instructions once and execute them with `/prompts:<name>` inside the chat surface.

## Quick start

VT Code ships with a starter prompt named `/prompts:vtcode` that guides kickoff conversations. It is always available, even if you have not created any files yet, and you can override it by dropping another `vtcode.md` into your custom prompt directory.

1. Create a Markdown file such as `~/.vtcode/prompts/review.md` with optional YAML frontmatter.
2. Restart VT Code (or start a new session) so the registry loads the file.
3. Run `/prompts` to confirm it appears with its description and argument hint.
4. Execute `/prompts:review FILE=src/lib.rs` (or substitute your placeholder values) to expand the template into the input box for final edits before sending.

The registry validates placeholders and size limits before sending the expanded content, ensuring you never dispatch a partially filled template.

## Where prompts live

- **Primary directory:** By default prompts are loaded from `~/.vtcode/prompts`. Set the `VTCODE_HOME` environment variable to point VT Code at a different home directory. Prompts are read from `$VTCODE_HOME/prompts` when the variable is set.
- **Built-in defaults:** VT Code includes a bundled `/prompts:vtcode` file for session kickoffs. You can override it with your own `vtcode.md` alongside other prompts.
- **Additional directories:** Configure extra search paths with `agent.custom_prompts.extra_directories` in `vtcode.toml`. Relative paths are resolved against the active workspace.
- **File type:** Only Markdown files (`.md`) are loaded. Non-Markdown files are ignored.
- **Naming:** The filename (without `.md`) becomes the prompt name. Avoid whitespace or colon characters in filenames. For example, `review.md` registers `/prompts:review`.
- **Refresh cycle:** Prompts are loaded during session startup. Restart VT Code (or open a new session) after creating or editing files.

## File format

Custom prompts support optional YAML frontmatter for metadata followed by the prompt body. The body is injected verbatim (after placeholder expansion) when you invoke the prompt.

```markdown
---
description: Request a concise git diff review
argument-hint: FILE=<path> [FOCUS=<section>]
---
Please review $FILE and highlight $FOCUS in the diff summary.
```

- `description` appears in `/prompts` output and helps differentiate prompts.
- `argument-hint` provides guidance on expected arguments (also surfaced by `/prompts`).
- Frontmatter is optional; omit it if you only need the prompt body.

## Placeholders and arguments

Custom prompts accept both positional and named arguments. VT Code follows the Codex placeholder rules:

- **Positional placeholders:** `$1` through `$9` insert the corresponding positional arguments. All referenced positions must be supplied.
- **All arguments:** `$ARGUMENTS` expands to every positional argument joined by a space (useful for free-form notes).
- **Named placeholders:** Tokens such as `$FILE` expand from `KEY=value` pairs. Keys are case-sensitive—`$FILE` requires `FILE=...` when invoking the prompt.
- **Literal dollars:** Use `$$` to emit a literal `$`.

Example invocation:

```text
/prompts:review critical FILE=src/lib.rs
```

- `critical` becomes `$1`.
- `FILE=src/lib.rs` populates `$FILE`.
- `$ARGUMENTS` expands to `critical`.

VT Code validates required placeholders. If a named or positional placeholder is missing, you will see a validation error instead of an incomplete prompt.

## Running prompts

1. Start a new session (or restart) so VT Code picks up the latest prompt files.
2. Type `/prompts` to list registered prompts, descriptions, and argument hints.
3. Execute a prompt with `/prompts:<name>` followed by any required arguments.
4. The expanded prompt is sent as your chat input, so you can preview and modify it before VT Code responds.

## Configuration reference

Configure prompts via the `[agent.custom_prompts]` section in `vtcode.toml`:

```toml
[agent.custom_prompts]
enabled = true
# Default directory (supports `~` and $VTCODE_HOME).
directory = "~/.vtcode/prompts"
# Optional extra directories (absolute or workspace-relative).
extra_directories = [".vtcode/project-prompts"]
# Maximum prompt size in kilobytes.
max_file_size_kb = 64
```

- Set `enabled = false` to disable custom prompts entirely.
- Use `extra_directories` to include workspace-specific prompts alongside your global collection.
- `max_file_size_kb` protects against accidentally loading large files. Files larger than the limit are skipped with a warning.

## Tips

- Store prompt files under version control in your dotfiles or workspace repo to share workflows with teammates.
- Combine argument placeholders with shell quoting (`KEY="value with spaces"`) when invoking prompts that expect multi-word inputs.
- Leverage `/prompts` alongside `/help` so teammates can quickly discover shared automation shortcuts.
