# Agent Skills Spec Implementation

This document describes VT Code's current Agent Skills behavior.

## Implemented Behavior

- Strict `SKILL.md` authoring validation, lenient loading (spec client guide)
- Repository discovery through ancestor `.agents/skills` directories
- User discovery from `~/.agents/skills`
- Admin discovery from `/etc/codex/skills`
- Bundled system skills exposed as `system` scope
- Implicit routing based on `description`
- Disabled-skill filtering from `~/.codex/config.toml`

## Supported `SKILL.md` Fields

Spec fields (per <https://agentskills.io/specification.md>):

Required:

- `name`
- `description`

Optional:

- `license`
- `compatibility`
- `metadata`
- `allowed-tools` (experimental per the spec)

Client extensions (parsed for cross-client compatibility, not part of the spec):

- `argument-hint` (slash-command style argument hint; non-string YAML values are coerced to a string)
- `disable-model-invocation` (hides the skill from the model-facing catalog; explicit activation still works)

Any other frontmatter key warns during parsing (forward-compatible, value ignored) and fails `vtcode skills validate`.

## Validation Rules

### `name`

- 1 to 64 characters
- lowercase letters, numbers, and hyphens only
- no leading or trailing hyphen
- no consecutive hyphens
- must match the skill directory name

### `description`

- required
- non-empty
- maximum 1024 characters

### Optional fields

- `license`: maximum 512 characters
- `compatibility`: 1 to 500 characters if present
- `allowed-tools`: space-delimited string (spec) or YAML list (Claude Code convention), normalized to a space-delimited
  string and limited to 16 tools; an empty value (empty string or empty list) is rejected at parse time
- `metadata`: string-to-string map per the spec; the engine additionally accepts arrays and nested maps so real-world
  skills keep loading
- `argument-hint`: slash-command style argument hint; non-string YAML values are coerced to a string

### Loading leniency (spec client guide)

Strict validation above applies to authoring (`vtcode skills validate`). At load time the engine is lenient per the
spec's client guide:

- directory-name mismatch warns and loads anyway
- malformed description colons fall back to block-scalar parsing
- missing/empty `description` or unparseable YAML still skips the skill (the description is the routing signal)

## Discovery Precedence

1. Closest repository `.agents/skills`
2. Higher ancestor repository `.agents/skills`
3. `~/.agents/skills`
4. `/etc/codex/skills`
5. Bundled system skills

Same-name collisions resolve by precedence (first discovery wins) with a startup warning naming the shadowed skill.
Walks skip `.git`/`.hg`/`.svn`/`node_modules`/`target`, stop past 10 levels, and stop after 2000 directories per root.
Plugin-consumed directories load as one entry and are never descended into.

## Deliberate Non-Support

VT Code does not support:

- legacy VT Code skill frontmatter extensions
- deprecated skill locations such as `.vtcode/skills`, `.claude/skills`, `.pi/skills`, `.codex/skills`,
  `.github/skills`, or `./skills`
- `agents/openai.yaml`

## Runtime Surface

- `skills list` and `skills info` render strict-spec metadata plus VT Code's explicit command-skill metadata
- skill prompts include only name, description, file path, and scope
- routing logic uses `description`; legacy trigger fields are not considered
- skills with `disable-model-invocation: true` are hidden from the model-facing startup catalog but remain available for
  explicit slash or `/skills use` activation
