# Rust Style Guide Adoption

VT Code applies compatible recommendations from [Google's Rust Style Guide][google], reviewed on 2026-10-22.
This is contributor guidance for maintained workspace Rust code and examples; generated and vendored code are excluded.
The project's safety contracts and established architecture take precedence. Adoption means applying a rule where it
improves the API, rather than mechanically rewriting every existing expression or adding speculative abstractions.

[google]: https://google.github.io/styleguide/rustguide.html

## Adoption matrix

The rows below cover every section of the upstream guide. Grouped recommendations share the stated disposition.
"Review" means a contextual contributor rule, not a newly enabled blanket lint or a claim of exhaustive conformance.

| Guide section / recommendations | Disposition and existing enforcement | Exceptions / migration policy |
| --- | --- | --- |
| Background: scope and goals | Adopt readability, maintainability, automation, and ecosystem consistency. | Google-specific authority does not apply to this Cargo workspace. |
| Follow public Rust style rules | Adopt Rust conventions; rustfmt, compiler warnings, Clippy, and crate instructions enforce the baseline. | Existing repository rules take precedence. |
| Apply API design rules in context | Adopt: reusable infrastructure needs flexible APIs; application code favors directness. | No speculative traits, wrappers, or generic parameters. |
| Code organization: small crates; flat `rust_library` layout | Retain existing focused workspace crates and Cargo module layout. | Chromium build-system layout is inapplicable; no wholesale extraction or flattening. |
| Code organization: name leaf modules after their primary item; keep non-leaf modules structural | Review when adding modules; keep established module names and facades. | Existing orchestration modules contain tightly coupled implementation; moving it solely for style creates churn. |
| Code organization: glob imports | Prefer explicit production imports; ACP tool configuration now names its constants. | Child-module `super::*`, compatibility re-exports, and established ratatui preludes remain intentional. |
| Code organization: imports from one module; local trait imports | Review: group related imports and keep one-off traits close to use. | Stable rustfmt does not enforce nightly import granularity; shared traits can stay at module scope. |
| Code organization: qualify free functions; unqualified type names | Review for clarity and ambiguity. | Established descriptive helper imports and qualified collision-prone types remain; no mass renaming. |
| Code organization: Chromium import mechanism | Not applicable. | Use Cargo dependencies and ordinary Rust imports. |
| Formatting: formatting tool; opting out | Adopt rustfmt and narrowly justified skips for generated/aligned content. | Preserve the project's 120-column configuration and stable toolchain. |
| Ownership: shared ownership; borrowing over deep copies | Prefer borrows and unique ownership; clone only for independent ownership. | Tokio tasks, shared provider state, tool catalogs, and callbacks require `Arc`; do not replace their ownership model. |
| Inputs/outputs: flexible immutable references | Adopt slices, `&str`, `&Path`, `&OsStr`, and optional inner borrows; `ptr_arg` and `borrowed_box` explicitly enabled. | External traits, Serde field adapters, and container-specific operations retain their required shapes. |
| Inputs/outputs: iterator returns and parameters | Review reusable iteration-only APIs. | Keep vectors for owned snapshots, sorting, indexing, serialization, or naturally collected application results; slices simplify bounded inputs. |
| Inputs/outputs: accepting `Into` | Retain ergonomic consuming builders where input conversion is useful. | Do not generalize ordinary application helpers without a caller need. |
| Inputs/outputs: enums over ambiguous booleans | Review for demonstrated call-site ambiguity. | Existing named configuration flags and straightforward predicates remain booleans; no serialization changes. |
| Inputs/outputs: error reporting | Adopt `Option` for absence and `Result` with context for recoverable failures. | Preserve production panic restrictions and fail-closed sandbox/approval behavior. |
| Types: newtypes versus aliases; named fields | Existing invocation IDs use newtypes; root guidance requires explicit structured shapes. | Local unambiguous tuples and semantic-free aliases remain; no wire-format migrations. |
| Traits: add when necessary; standard traits | Adopt; explicitly enable `should_implement_trait` and `wrong_self_convention`. | Traits required for dynamic provider/tool dispatch remain; no organizational-only traits. |
| Traits: derive when equivalent | Adopt; explicitly enable `derivable_impls`; metadata builder's empty default is now derived. | Custom defaults, semantic equality, and resource ownership must not change. |
| Traits: generics versus dynamic dispatch | Review reusable APIs; retain existing static dispatch where useful. | Heterogeneous runtime tools/providers require `dyn Trait`; avoid monomorphization solely for style. |
| Traits: extension traits and inherent implementations | Adopt purposeful traits and `Ext` naming for extensions. | Existing `RetryPolicyCoreExt` layers domain logic over the shared policy; preserve the dependency boundary. |
| Traits: important standard traits; constructors paired with `From`; options defaults | Add meaningful trait implementations and discoverable constructors as needed. | Do not derive `Copy`, `Clone`, ordering, or defaults that misrepresent resource or domain semantics. |
| Traits: `Default` for `new()` | Explicit `new_without_default` enforcement. | Fallible constructors or constructors requiring arguments do not imply a meaningful default. |
| Concurrency: poisoned locks | Review recovery against the protected invariant. | Retain existing defensive recovery boundaries; this style change must not alter runtime recovery behavior. |
| Concurrency: no async, no standard threads, Fuchsia mutex | Not applicable platform-specific rules. | VT Code uses Tokio, targeted blocking work, and established synchronization primitives. |
| Macros: prefer ordinary Rust, then declarative macros | Adopt for new APIs; retain shared declarative/code-generated contracts. | Serde, async traits, model tables, and existing procedural macros avoid duplication and remain. |
| Naming: methods, lifetimes, generic parameters, extensions | Adopt descriptive names and conventional `as_`/`to_`/`into_`; `wrong_self_convention` is explicit. | Short local lifetime relationships and generic `T` remain clear; keep established public names. |
| Naming: initialization functions and abbreviations | Review new APIs for clarity; retain ecosystem terms such as LLM, MCP, PTY, and ACP. | Do not rename established fallible `new` APIs solely to `try_new`, or force downstream churn for spelling. |
| Documentation: public APIs, examples, panic/unsafe contracts, formatting | Adopt rustdoc, meaningful ownership/error/invariant descriptions, and existing `missing_docs` warnings. | Comments should explain contracts; do not duplicate names or language basics. |
| Other features: unsafe code; unstable features | Existing workspace warnings, safety comments, and unsafe documentation gate enforce the baseline. | Necessary platform FFI stays isolated; this change adds no unsafe or unstable code. |
| Other features: getters/setters; closure accessors | Review new APIs using established naming and scoped access. | Preserve existing public accessors and lock boundaries instead of cosmetic renaming. |
| Other features: complete API sets | Explicit `len_without_is_empty` enforcement; existing nonempty collections expose their semantic emptiness contract. | Do not manufacture collection methods or zero-argument constructors for unrelated types. |
| Other features: hidden items, const functions, operator overloads | Treat foreign hidden items as private; keep const commitments and mathematical operator semantics. | No new const promises or operator overloads in this adoption. |
| Other features: non-exhaustive enums and public structs | Decide individually for externally evolving APIs. | Keep exhaustive runtime events, protocol enums, and public record construction; blanket attributes would break callers. |
| Other features: `must_use` on functions and types | Target pure formatters, policy predicates, retry/permission decisions, and consuming builders. | Avoid redundant annotations on `Result`, futures, or already annotated return types; no blanket pedantic lint rollout. |
| Other features: code before main | Retain explicit runtime initialization and lazy statics. | Do not add eager static constructors. |
| Implementation: type inference | Review for readable types at API boundaries and complex collections. | Keep obvious local inference; no blanket annotation churn. |
| Implementation: pattern matching over check-then-unwrap | Adopt; explicitly enable `unnecessary_unwrap`; the scanned production `is_some` branches need no equivalent rewrite. | Test assertions may unwrap; mutation between a check and access needs separate analysis. |
| Implementation: expect over unwrap | Existing production restrictions on both remain stricter than the guide. | Tests can use either; recoverable failures must preserve error context. |
| Implementation: match known non-exhaustive variants; assertion context | Review for actionable diagnostics and complete known-case handling. | Retain intentional compatibility fallbacks and safety gates. |
| Implementation: clear iteration; formatting strings | Adopt direct loops when clearer and existing `format!`/`write!` patterns. | Do not rewrite efficient buffer appends or short comprehensible iterator chains. |
| Implementation: avoid `catch_unwind` | Exception: retain established coarse defensive/plugin unwind boundaries. | Removing them changes runtime isolation; never use unwind catching instead of ordinary error handling. |
| Test framework | Use focused locked nextest suites, then workspace verification. | Do not replace the established test framework or add tests solely for annotation/import changes. |

## Rust API migration notes

These are source-level changes only. Serialized data, tool schemas, process approval, environment filtering, and runtime
outputs retain their existing semantics. No release version or dependency changes are required by this patch.

| Public Rust API | Previous input | New input / caller migration |
| --- | --- | --- |
| `vtcode_commons::validation::validate_optional_non_empty` | `value: &Option<String>` | `value: Option<&str>`; pass `value.as_deref()`, `Some("text")`, or `None`. |
| `vtcode_bash_runner::pipe::{spawn_process, spawn_process_no_stdin}` | `arg0: &Option<String>` | `arg0: Option<&str>`; pass `arg0.as_deref()` or `None`; aliases `spawn_pipe_process` and `spawn_pipe_process_no_stdin` change identically. |
| `vtcode_mcp::create_stdio_transport_with_stderr` | `program: &OsString`, `working_dir: Option<&PathBuf>` | `program: &OsStr`, `working_dir: Option<&Path>`; direct executable borrows coerce; use `working_dir.as_deref()`. |
| `vtcode_core::compaction::memory_envelope::build_session_memory_envelope` | `history_artifact_path: Option<&PathBuf>` | `history_artifact_path: Option<&Path>`; use `path.as_deref()` for `Option<PathBuf>`. |
| `vtcode_core::metrics::MetricsCollector::record_circuit_breaker_metrics` | `metrics: &Option<Arc<MetricsCollector>>` | `metrics: Option<&MetricsCollector>`; pass `metrics.as_deref()`. The helper does not retain or clone the collector. |
| `vtcode_core::subagents::request_prompt` | `message: &Option<String>` | `message: Option<&str>`; pass `message.as_deref()` or a borrowed literal. |

The core MCP/commons re-export facades inherit the same signature changes. Function-pointer users must update their
parameter types even where ordinary direct calls benefit from deref coercions. Optional containers do not perform those
coercions automatically; use `as_deref()`, or map an already borrowed container with `Vec::as_slice` / `PathBuf::as_path`.

Private helper migrations cover tool-catalog names, allowlist banners, command summaries, provider option adapters,
image serialization, skill metadata keywords, ACP terminal location display, and live PTY header matching.

Retry policies/decisions/steps, execution/permission decisions, and the audited consuming builders now warn when discarded.
Consume their result or explicitly discard it when that is intentional. The metadata builder's derived `Default` still
constructs empty file, line, and data collections. No `Default` or `is_empty` method was added without a semantic need.

## Audit scope and verification

The workspace pass covers borrowed container/optional input candidates, public builder and decision declarations,
standard-trait/constructor/collection lints, equivalent defaults, production glob imports, and check-then-unwrap branches.
Clippy checks all workspace targets, including maintained examples. `vtcode-eval` does not inherit the broader workspace
lint policy, so its manifest explicitly mirrors the eight API-style checks. The scan is a contextual adoption pass,
not a formal proof that every public function is pure or that every possible stylistic improvement has been made.

Keep container-shaped signatures when an external callback/trait requires them (for example `PartialEq<String>`,
dialoguer validators, and Serde field serialization adapters). Keep owned vectors/maps and references to them when
their actual container identity or ownership is part of the operation. Existing shared snapshots, lifecycle engines,
configuration snapshots, and usage-accounting option adapters are retained in this pass to preserve their established
cross-module contracts; they are not templates for new immutable input APIs.

The live binary spool hint formatter already accepts `&str`; it now has `must_use`. The detached
`src/agent/runloop/tool_output/spool.rs` is not in the module tree and is left unchanged rather than adding an unused API.

Focused regressions exercise optional/literal strings, array and subslice tool catalogs, ordering,
absence, empty input, Unix `arg0` borrowing, and optional borrowed working directories. Existing provider, process,
borrowed-path memory-envelope, and rendering tests cover the migrated call sites. Transport dump tests wait for successful
child completion under a timeout before reading output, with a partial-write regression; shell-based tests are Unix-only.
Annotation/import/derive changes rely on compilation and existing behavioral tests.

Run sequentially when sharing `target/`:

```sh
cargo fmt --all -- --check
./scripts/check-dev.sh
cargo nextest run --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
python3 scripts/check_markdown.py
python3 scripts/check_docs_links.py
git diff --check
```

Host checks do not prove platform-gated Windows/Linux code or every feature combination. Report test failures, skips,
leak warnings, and unavailable platforms with the actual command results.
