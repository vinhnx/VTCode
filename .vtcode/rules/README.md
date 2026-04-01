# VT Code Rules

Place shared VT Code instruction files here.

- Add always-on rules as `*.md` files anywhere under `.vtcode/rules/`.
- Add path-scoped rules with YAML frontmatter, for example:

```md
---
paths:
  - "src/**/*.rs"
---

# Rust Rules
- Keep changes surgical.
```
