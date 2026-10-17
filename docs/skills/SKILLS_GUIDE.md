# Agent Skills Guide

VT Code supports repository, user, admin, and bundled system skills using the open Agent Skills `SKILL.md` format.

## Discovery

VT Code discovers skills in this order:

1. Nearest ancestor `.agents/skills` from the current working directory up to the git repository root
2. `~/.agents/skills`
3. `/etc/codex/skills`
4. Bundled system skills

If multiple skills share the same name, the nearest repository skill wins, then user, then admin, then system.

VT Code also honors disabled entries from `~/.codex/config.toml`:

```toml
[[skills.config]]
path = "/path/to/skill/SKILL.md"
enabled = false
```

## Skill Structure

Each skill is a directory containing a required `SKILL.md` file.

```text
my-skill/
├── SKILL.md
├── scripts/
├── references/
└── assets/
```

`scripts/`, `references/`, and `assets/` are optional.

## SKILL.md

VT Code accepts the core Agent Skills frontmatter fields plus the client-side `disable-model-invocation` flag used to
hide a skill from the model-facing startup catalog while keeping it available for explicit harness activation:

```yaml
---
name: my-skill
description: Explain what this skill does and when to use it.
license: Apache-2.0
compatibility: Requires git and network access
allowed-tools: Read Write Bash
argument-hint: "[expected argument]"
metadata:
  owner: platform-team
---
```

Required fields:

- `name`
- `description`

Optional spec fields:

- `license`
- `compatibility`
- `metadata`
- `allowed-tools` (experimental per the spec)

Client extensions (parsed, not part of the spec):

- `argument-hint`
- `disable-model-invocation`

Legacy VT Code frontmatter such as `version`, `author`, `when-to-use`, `when-not-to-use`, `model`, `mode`, `context`,
`agent`, `network`, `permissions`, container flags, and similar extensions is rejected.

VT Code does not support `agents/openai.yaml`. That file is Codex-specific and ignored by design.

## Prompting Behavior

- Explicit mention wins: `Use the my-skill skill`
- Implicit matching uses `description`
- Full `SKILL.md` bodies are loaded only when a skill is selected
- Referenced resources are loaded on demand
- Catalog modes: `lean` (default, name + description + path + scope) and `full` (lean plus
  `compatibility` and `allowed-tools` when present). Up to 10 skills inline; overflow links to `list_skills`.

## Activation Context

Activated skills render as bounded `<untrusted_skill_instructions>` with:

- `Skill directory:` base path for resolving relative `scripts/`/`references/`/`assets/` paths
- Escaped body (fence-injection safe, 32 KiB cap with truncation marker)
- `<skill_resources>` listing (sorted, capped at 32) loaded on demand, never bulk-loaded

Skill content is untrusted resource data; host tool and sandbox policy remains authoritative.

## Sub-LLM Tool Execution

Skills run in a sub-conversation with their own model request. When the serving model emits no native function calls but
embeds textual `<tool_call>` markup in its reply (common on gateway-served models), VT Code parses the first unfenced,
cleanly named block and executes it as a native-equivalent tool call:

- Shell aliases (`bash`, `shell`, `run`, …) are canonicalized to `exec_command`.
- The parsed call flows through the same skill tool scope, permission policy, and loop detection as a native call —
  nothing bypasses approval.
- Markup inside fenced code blocks is documentation (skill docs, quoted examples) and is never executed; mid-prose
  mentions that do not resolve to a tool identifier are skipped.
- If no parseable markup remains, the reply is treated as the skill's final content and synthesized without tools.

## Commands

List skills:

```bash
vtcode skills list
```

Inspect one skill:

```bash
vtcode skills info my-skill
```

Create a new skill scaffold:

```bash
vtcode skills create my-skill
```

Validate a skill:

```bash
vtcode skills validate ./.agents/skills/my-skill
```

Show configured paths:

```bash
vtcode skills config
```

## Slash Command Skills

VT Code also exposes the interactive slash-command surface as skills.

- Canonical skill names use the `cmd-<slash-name>` form, for example `cmd-status` or `cmd-review`.
- The `/status` or `/review` slash command remains the primary interactive alias.
- Built-in session/UI commands are surfaced as built-in command skills.
- Prompt-oriented slash commands are shipped as bundled system skills in the release binary and installed under the
  system skill cache at runtime.
- Command skills are intentionally excluded from the default prompt-side skill index to avoid spending context on
  slash-command metadata that is already exposed elsewhere in the harness.
- Built-in command skills support `info` and `use`, but not `load`.

## Notes

- `vtcode skills create` generates a spec-first `SKILL.md` scaffold with optional commented guidance for
  `disable-model-invocation`.
- User-facing skill metadata in VT Code is limited to the strict `SKILL.md` fields above.
- Bundled system skills are surfaced as `system` scope.

## Authoring Patterns

Scaffold includes `Gotchas`, `Output Format`, `Checklist`, and `Validation` sections per agentskills.io
best-practices:

- Gotchas: non-obvious facts the agent will get wrong (soft-deletes, ID aliases, health-check quirks).
- Output Format: concrete template; store long templates under `assets/` and reference them.
- Checklist: trigger match, load only needed refs, prefer `scripts/`, validate before finishing.
- Validation loop: produce artifact, run validator, fix and re-run until passing.
- Prefer one default tool path with a brief escape hatch; describe reusable procedures, not single answers.

## Script Design

Bundled `scripts/` helpers must be non-interactive and agentic-friendly:

- `argparse` with `--help`, `--format json|text`, `--output -|FILE`, `--dry-run` for previews.
- Structured data to stdout (JSON), diagnostics to stderr, meaningful exit codes.
- Idempotent (`create if not exists`); PEP723 `# /// script` inline deps when needed.
- Pin one-off `uvx`/`npx` versions; state prerequisites in `compatibility`.
