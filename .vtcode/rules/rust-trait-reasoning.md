---
paths:
  - "**/*.rs"
---

# Rust Trait Reasoning

- Treat trait obligations as explicit evidence flowing through the program.
- Spell out concrete impls, bounds, and associated-type equalities instead of assuming they hold implicitly.
- Prefer explicit seams over implicit trait tricks for extension points, especially across crates or public boundaries.
- If a trait-heavy design is hard to justify in a dictionary-passing mental model, simplify it.
