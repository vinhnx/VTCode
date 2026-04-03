# Rust Trait Reasoning

When working on Rust code in this workspace, reason about trait solving as if trait obligations were explicit evidence passed through the program.

- For each trait bound, identify where the proof comes from: a local bound, a concrete impl, or an explicit provider/context wiring step.
- Treat associated types and equality constraints as first-class obligations. Spell out the required equalities instead of assuming they follow "by magic".
- If a design depends on coherence or global impl uniqueness, call that out explicitly. Do not silently rely on it in code review or design reasoning.
- Prefer explicit seams over implicit trait tricks when designing VT Code extension points. Internal traits are fine, but public or cross-boundary extension should continue to favor providers, manifests, protocols, and other explicit wiring.
- When a trait-heavy abstraction feels hard to justify, try mentally elaborating it into a dictionary-passing model first. If the data flow or proof flow is unclear there, simplify the design.
