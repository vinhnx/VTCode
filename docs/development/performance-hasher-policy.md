# Performance Hasher Policy

VT Code uses `rustc_hash` selectively, not globally.

## Default

Use `std::collections::{HashMap, HashSet}` by default.

## When `rustc_hash` Is Allowed

Use `FxHashMap` / `FxHashSet` only for measured hotspots where keys are internal and trusted.

Good candidates:

- Internal caches and indices.
- Tool-routing lookup maps built from internal metadata.
- Short-lived per-turn maps in hot loops.

## When It Is Not Allowed

Do not switch to `Fx*` in security-sensitive or attacker-controlled keyspaces.

Avoid for:

- Untrusted external input maps.
- Policy/security boundary logic where collision resistance matters.

## Migration Gate

Before switching a map/set:

1. Capture baseline with `./scripts/perf/baseline.sh baseline`.
2. Apply selective hasher change.
3. Capture latest with `./scripts/perf/baseline.sh latest`.
4. Compare with `./scripts/perf/compare.sh`.
5. Keep change only if there is a clear measured win and no behavioral regressions.
