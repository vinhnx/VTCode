# Declarative Macro Hygiene — Reference Notes

Reference artifact for VT Code contributors and coding agents authoring or reviewing
`macro_rules!` macros across the workspace.

## Source (not vendored)

- Title: **Declarative Macros in Rust: A Simple and Practical Introduction**
- Author: Sigseis
- Published: 2026-10-06
- URL: <https://sigseis.dev/articles/2026/10/06>

The full text lives at the URL above and is **not reproduced here**. What follows is an original distillation plus a
small number of short attributed references for orientation. Read the original for the complete treatment.

## Why this matters to VT Code

VT Code generates a large share of its provider, UI-command, and CGP wiring code through exported `macro_rules!`
macros. Rust macros re-parse at their *invocation site*, with **mixed-site hygiene**: functions, types, and other named
items resolve at the call site, while local variables and labels resolve at the definition site. That makes two classes
of bug easy to introduce and hard to see in review:

1. An exported macro that names an unqualified item the caller may not import.
2. A macro that references a path relative to its own module (`super::`, `self::`), which silently changes meaning
   wherever it is expanded.

The article's practical guidance for macros that leave their defining module boils down to naming discipline plus one
language primitive (`$crate`).

## Applicable rules

- **Qualify definition-site items absolutely.** In an exported macro, name every helper by its full path. Prefer
  `$crate::...` for the defining crate; fall back to an absolute `crate::` path when the macro is `pub(crate)` and
  never crosses a crate boundary.
- **`$crate` for exported macros.** `$crate` expands to the defining crate regardless of the invoker's imports and is
  immune to the caller shadowing a name. Use it for *exported* macros (`#[macro_export]`). A crate-local `super::` /
  `self::` path is a definition-relative reference: it is fine only when the macro can never be expanded outside its
  defining module hierarchy. If it can move, make it `crate::` or `$crate::`.
- **Do not rely on call-site imports** for anything the macro itself needs (`HashSet`, `format!` helpers, sibling
  modules). The caller controls `use` statements; the macro must not.
- **Match delimiters intentionally.** `()`/`[]` invocations parse as expressions; `{}` invocations have special
  item/statement behavior and do not need a trailing semicolon. Keep the workspace convention: `{}` for impl/type
  generation, `()` for function-call-shaped macros.
- **Respect follow-set rules.** `expr`/`stmt` fragments may only be followed by `,`, `;`, or `=>`; `ty`/`path` have
  their own set. A matcher like `($e:expr + $rest:expr)` will not compile.

## Reviewing a macro

Ask, for every `macro_rules!` that is exported or used across modules:

1. Does the transcriber reference any sibling item, helper fn, or type without a `crate::` / `$crate::` prefix?
2. Does it use `super::`/`self::` in a position the caller can reach?
3. Does the matcher put a fragment specifier where the follow-set forbids the next token?

## Current state (audit 2026-10-11)

| Macro | Location | Reach | Status |
| --- | --- | --- | --- |
| `model_family!` | `vtcode-commons/src/model_family.rs` | exported, in-crate only | uses `$crate` ✅ |
| `delegate_components!` | `vtcode-commons/src/cgp.rs` | exported, cross-crate | uses `$crate` ✅ |
| `create_provider_builder!` | `vtcode-core/src/llm/provider_config.rs` | exported, `pub(crate)` | uses `$crate` ✅ |
| `impl_llm_client!` | `vtcode-llm/src/providers/common.rs` | crate-local, cross-module | fixed: `super::common::` → `crate::providers::common::` |
| `impl_inline_control_methods!` | `vtcode-ui/.../control_commands.rs` | crate-local, cross-module | types reachable via `$crate`; `vtcode_commons::` is an absolute crate path ✅ |
| `file_err!` / `ctx_err!` | `vtcode-commons/src/errors.rs` | exported | pure `format!`; shape tests added |
| `benchmark!` | `vtcode-core/src/core/performance_profiler.rs` | exported, unused | `std::` paths are call-site-independent; revisit if revived |

## Note on lint coverage

`ast-grep` cannot inspect macro transcriber bodies (token trees do not resolve to typed nodes), so there is no automated
rule for this class yet. The check is manual at review time; the table above is the frozen baseline of the files worth
looking at.
