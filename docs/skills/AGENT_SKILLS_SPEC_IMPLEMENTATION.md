# Agent Skills Spec Implementation

This document describes VT Code's current Agent Skills behavior.

## Implemented Behavior

- Strict `SKILL.md` frontmatter parsing
- Repository discovery through ancestor `.agents/skills` directories
- User discovery from `~/.agents/skills`
- Admin discovery from `/etc/codex/skills`
- Bundled system skills exposed as `system` scope
- Implicit routing based on `description`
- Disabled-skill filtering from `~/.codex/config.toml`

## Supported `SKILL.md` Fields

Required:

- `name`
- `description`

Optional:

- `license`
- `compatibility`
- `metadata`
- `allowed-tools`

Any other frontmatter key is rejected during parsing and validation.

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
- `allowed-tools`: normalized to a space-delimited string and limited to 16 tools

## Discovery Precedence

1. Closest repository `.agents/skills`
2. Higher ancestor repository `.agents/skills`
3. `~/.agents/skills`
4. `/etc/codex/skills`
5. Bundled system skills

## Deliberate Non-Support

VT Code does not support:

- legacy VT Code skill frontmatter extensions
- deprecated skill locations such as `.vtcode/skills`, `.claude/skills`, `.pi/skills`, `.codex/skills`, `.github/skills`, or `./skills`
- `agents/openai.yaml`

## Runtime Surface

- `skills list` and `skills info` render only strict-spec metadata
- skill prompts include only name, description, file path, and scope
- routing logic uses `description`; legacy trigger fields are not considered
