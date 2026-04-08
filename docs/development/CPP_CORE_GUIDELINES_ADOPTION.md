# C++ Core Guidelines Adoption

Last reviewed: 2026-04-08

This repository is primarily Rust. This policy exists to codify how we handle C/C++ code when it appears (FFI boundaries, native helpers, vendor integrations, tools, or future modules).

## Scope

- Applies to all C and C++ code introduced or modified in `vtcode`.
- For non-C++ code (including Rust), use the same intent: explicit ownership, strong interfaces, safety-first defaults, and minimal complexity.

## Mandatory C++ Rules

- Target ISO C++20 or newer. Avoid non-standard compiler extensions unless required by a platform boundary.
- Model ownership explicitly. Do not transfer ownership via raw `T*` or `T&`.
- Avoid naked `new`/`delete` and `malloc`/`free` in application code. Prefer RAII handles (`std::unique_ptr`, `std::shared_ptr`, standard containers, scoped wrappers).
- Prefer `std::span`, `std::string_view`, and strong types over pointer+length and primitive argument bundles.
- Avoid C-style casts. Prefer no cast; if unavoidable, use explicit named casts and isolate risky casts.
- Constructors must establish invariants; if that fails, fail construction (exception/factory error path).
- Keep mutable global state to an absolute minimum and document every exception.
- Keep interfaces explicit and strongly typed; avoid boolean flag arguments that obscure intent.
- Prefer standard-library algorithms/containers over hand-rolled low-level code.
- Mark truly non-throwing low-level operations `noexcept` where appropriate.

## Review and Enforcement

- PRs that touch C/C++ should be reviewed against this file and the C++ Core Guidelines.
- If a rule must be violated for ABI/platform/performance reasons, encapsulate the violation, add a focused code comment, and document rationale in the PR.
- Keep deviations local and do not leak unsafe patterns across module boundaries.
