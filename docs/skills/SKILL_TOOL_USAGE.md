# Skill Tool Usage

VT Code exposes skills through the skills subsystem and related tooling.

## Discovery Queries

Skill listing and filtering operate on:

- `name`
- `description`

Legacy routing fields are not part of filtering.

## Prompt Integration

When VT Code surfaces skills to the model, it includes only:

- name
- description
- file path
- scope

The full `SKILL.md` body stays on disk until the skill is selected.

## Resource Loading

After a skill is selected:

1. Load `SKILL.md`
2. Load `scripts/`, `references/`, or `assets/` only when needed

## Storage Locations

- repository: ancestor `.agents/skills`
- user: `~/.agents/skills`
- admin: `/etc/codex/skills`
- system: bundled skills
